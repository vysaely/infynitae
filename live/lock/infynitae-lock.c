/*
 * infynitae-lock - the lock screen (and the start screen after login)
 *
 * It draws in the theme colors from the config (CANVAS_COLOR, SURFACE_COLOR, …),
 * standard Linux PAM authentication, ext-session-lock-v1: the compositor shows only this
 * client's surfaces and gives them every key until it unlocks, even if this client dies.
 */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>
#include <stdint.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <time.h>
#include <pthread.h>
#include <sys/prctl.h>
#include <pwd.h>
#include <math.h>
#include <poll.h>
#include <sys/socket.h>
#include <sys/un.h>

#include <wayland-client.h>
#include <cairo.h>
#include <pango/pangocairo.h>
#include <xkbcommon/xkbcommon.h>

#include <security/pam_appl.h>

#include "ext-session-lock-v1-protocol.h"

#define PASSWORD_MAX_LEN 256
#define SHAKE_DURATION_FRAMES 18

typedef enum {
    AUTH_STATE_IDLE,
    AUTH_STATE_TYPING,
    AUTH_STATE_VERIFYING,
    AUTH_STATE_FAILED,
    AUTH_STATE_SUCCESS
} AuthState;

struct LockOutput;

typedef struct {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct wl_seat *seat;
    struct ext_session_lock_manager_v1 *lock_manager;
    struct ext_session_lock_v1 *lock;
    bool locked;
    bool lock_refused;

    struct wl_keyboard *keyboard;
    struct xkb_context *xkb_ctx;
    struct xkb_keymap *xkb_keymap;
    struct xkb_state *xkb_state;

    struct LockOutput *outputs;
    bool running;
    bool caps_lock;

    /* the on-screen keyboard (touchscreens): shown, its layers, the key a finger holds */
    struct wl_touch *touch;
    bool osk_visible;
    bool osk_shift;
    bool osk_symbols;
    int osk_pressed;
    struct LockOutput *osk_pressed_output;
    int32_t osk_touch_id;
    double osk_touch_x, osk_touch_y;

    char username[128];
    char password[PASSWORD_MAX_LEN];
    size_t password_len;

    _Atomic AuthState auth_state;
    pthread_t pam_thread;
    _Atomic bool pam_thread_active;

    _Atomic int shake_frame;
    double pulse_phase;

    bool greeter_mode;
    char session_cmd[256];
    char status_msg[128];

    /* what the screen shows of the user (owner, 2026-11-06: "the lock screen is mid, like the
     * wallpaper on it and the user pfp"): the full name from the account (GECOS), else the login;
     * up to two initials when there is no picture */
    char display_name[128];
    char initials[8];
    /* the pictures the wallpaper helper prepared (live/wallpaper/lock-cache.h), loaded only once the
     * compositor confirmed the lock, so nothing before the lock depends on them; NULL: none */
    cairo_surface_t *background;
    cairo_surface_t *avatar;
    double corners; /* LOOK_CORNERS: the radius of the password field, as windows and the edge (0: square) */

    double canvas_rgb[3];
    double surface_rgb[3];
    double border_act_rgb[3];
    double border_inact_rgb[3];
    double text_rgb[3];
} LockApp;

typedef struct LockOutput {
    LockApp *app;
    struct wl_output *wl_output;
    uint32_t registry_name;

    struct wl_surface *surface;
    struct ext_session_lock_surface_v1 *lock_surface;
    struct wl_callback *frame_cb;

    int32_t width;
    int32_t height;
    int32_t scale;
    bool configured;

    struct wl_buffer *buffer;
    void *shm_data;
    size_t shm_size;

    /* the background drawn once at this screen's size (scaled, darkened) and reused for every key */
    cairo_surface_t *scene_bg;
    int scene_bg_w, scene_bg_h;
    cairo_surface_t *scene_bg_from;

    struct LockOutput *next;
} LockOutput;

/* ========================================================================== */
/* Forward Declarations                                                       */
/* ========================================================================== */
static void render_output(LockOutput *output);
static void request_frame(LockOutput *output);
static void damage_all_outputs(LockApp *app);

/* ========================================================================== */
/* Shared Memory Buffer Allocation                                            */
/* ========================================================================== */
static int create_shm_file(off_t size) {
    int fd = memfd_create("infynitae-lock-shm", MFD_CLOEXEC | MFD_ALLOW_SEALING);
    if (fd < 0) {
        char template[] = "/tmp/infynitae-shm-XXXXXX";
        fd = mkstemp(template);
        if (fd >= 0) unlink(template);
    }
    if (fd >= 0) {
        if (ftruncate(fd, size) < 0) {
            close(fd);
            return -1;
        }
    }
    return fd;
}

static void free_output_buffer(LockOutput *output) {
    if (output->buffer) {
        wl_buffer_destroy(output->buffer);
        output->buffer = NULL;
    }
    if (output->shm_data && output->shm_size > 0) {
        munmap(output->shm_data, output->shm_size);
        output->shm_data = NULL;
        output->shm_size = 0;
    }
}

static bool allocate_output_buffer(LockOutput *output) {
    int stride = output->width * 4;
    size_t size = (size_t)stride * output->height;

    if (output->buffer && output->shm_size == size) {
        return true;
    }

    free_output_buffer(output);

    int fd = create_shm_file(size);
    if (fd < 0) return false;

    void *data = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (data == MAP_FAILED) {
        close(fd);
        return false;
    }

    struct wl_shm_pool *pool = wl_shm_create_pool(output->app->shm, fd, size);
    output->buffer = wl_shm_pool_create_buffer(pool, 0, output->width, output->height,
                                               stride, WL_SHM_FORMAT_ARGB8888);
    wl_shm_pool_destroy(pool);
    close(fd);

    output->shm_data = data;
    output->shm_size = size;
    return true;
}

/* ========================================================================== */
/* PAM Authentication Worker Thread                                           */
/* ========================================================================== */
typedef struct {
    char username[128];
    char password[PASSWORD_MAX_LEN];
    LockApp *app;
} PamWorkerData;

static int pam_conv_func(int num_msg, const struct pam_message **msg,
                         struct pam_response **resp, void *appdata_ptr) {
    if (num_msg <= 0 || num_msg > 16) return PAM_CONV_ERR;

    struct pam_response *reply = calloc(num_msg, sizeof(struct pam_response));
    if (!reply) return PAM_BUF_ERR;

    const char *pass = (const char *)appdata_ptr;

    for (int i = 0; i < num_msg; ++i) {
        switch (msg[i]->msg_style) {
            case PAM_PROMPT_ECHO_OFF:
            case PAM_PROMPT_ECHO_ON:
                reply[i].resp = strdup(pass);
                reply[i].resp_retcode = 0;
                break;
            case PAM_ERROR_MSG:
            case PAM_TEXT_INFO:
                reply[i].resp = NULL;
                reply[i].resp_retcode = 0;
                break;
            default:
                for (int j = 0; j < i; ++j) {
                    if (reply[j].resp) {
                        explicit_bzero(reply[j].resp, strlen(reply[j].resp));
                        free(reply[j].resp);
                    }
                }
                free(reply);
                return PAM_CONV_ERR;
        }
    }
    *resp = reply;
    return PAM_SUCCESS;
}

static void *pam_worker_thread(void *arg) {
    PamWorkerData *data = (PamWorkerData *)arg;
    LockApp *app = data->app;

    struct pam_conv conv = {
        .conv = pam_conv_func,
        .appdata_ptr = data->password
    };

    // pam_start() succeeds even when /etc/pam.d/<service> is missing: PAM then silently uses
    // /etc/pam.d/other, which denies everything on Arch. So pick the service by whether its
    // file exists, and use "login" (the user's console password) when ours is not installed.
    const char *service = access("/etc/pam.d/infynitae", R_OK) == 0 ? "infynitae" : "login";
    pam_handle_t *pamh = NULL;
    int rc = pam_start(service, data->username, &conv, &pamh);

    if (rc == PAM_SUCCESS) {
        rc = pam_authenticate(pamh, 0);
        if (rc == PAM_SUCCESS) {
            rc = pam_acct_mgmt(pamh, 0);
        }
        pam_end(pamh, rc);
    }

    // Securely shred sensitive memory
    explicit_bzero(data->password, sizeof(data->password));

    if (rc == PAM_SUCCESS) {
        app->auth_state = AUTH_STATE_SUCCESS;
    } else {
        app->auth_state = AUTH_STATE_FAILED;
        app->shake_frame = SHAKE_DURATION_FRAMES;
    }

    explicit_bzero(data, sizeof(*data));
    munlock(data, sizeof(*data));
    free(data);
    app->pam_thread_active = false;
    return NULL;
}

static void trigger_auth(LockApp *app) {
    if (app->auth_state == AUTH_STATE_VERIFYING || app->pam_thread_active) return;
    if (app->password_len == 0) return;

    app->auth_state = AUTH_STATE_VERIFYING;

    PamWorkerData *data = malloc(sizeof(PamWorkerData));
    if (!data) return;
    // kept out of swap while it holds the password (best effort: RLIMIT_MEMLOCK may refuse)
    (void)mlock(data, sizeof(*data));

    strncpy(data->username, app->username, sizeof(data->username) - 1);
    data->username[sizeof(data->username) - 1] = '\0';
    strncpy(data->password, app->password, sizeof(data->password) - 1);
    data->password[sizeof(data->password) - 1] = '\0';
    data->app = app;

    // Clear local buffer immediately
    explicit_bzero(app->password, sizeof(app->password));
    app->password_len = 0;

    app->pam_thread_active = true;
    if (pthread_create(&app->pam_thread, NULL, pam_worker_thread, data) != 0) {
        app->auth_state = AUTH_STATE_FAILED;
        app->pam_thread_active = false;
        explicit_bzero(data, sizeof(*data));
        free(data);
        return;
    }
    // nothing waits for the worker: its resources go when it ends
    pthread_detach(app->pam_thread);
}

static void parse_hex_rgb(const char *s, double out[3]) {
    while (*s == ' ' || *s == '\t' || *s == '"' || *s == '\'') s++;
    if (*s == '#') s++;
    unsigned int r = 0, g = 0, b = 0;
    if (sscanf(s, "%02x%02x%02x", &r, &g, &b) == 3) {
        out[0] = (double)r / 255.0;
        out[1] = (double)g / 255.0;
        out[2] = (double)b / 255.0;
    }
}


/* The look the panels, widgets, notifications and menus share (docs/EDGE_RUST.md, stage 7), as
 * infynitae-settings resolved it into ~/.cache/infynitae/look: the card colour, its edge, the text,
 * the accent and the corners (0: square). Read after the config, so a colour scheme reaches the
 * lock screen too; without the file, the config's colours as before. */
static void read_look(LockApp *app, const char *home) {
    const char *cache = getenv("XDG_CACHE_HOME");
    char path[512];
    if (cache && *cache) snprintf(path, sizeof(path), "%s/infynitae/look", cache);
    else snprintf(path, sizeof(path), "%s/.cache/infynitae/look", home);
    FILE *f = fopen(path, "r");
    if (!f) return;
    char line[256];
    while (fgets(line, sizeof(line), f)) {
        if (strncmp(line, "LOOK_CARD=", 10) == 0) parse_hex_rgb(line + 10, app->surface_rgb);
        else if (strncmp(line, "LOOK_EDGE=", 10) == 0) parse_hex_rgb(line + 10, app->border_inact_rgb);
        else if (strncmp(line, "LOOK_TEXT=", 10) == 0) parse_hex_rgb(line + 10, app->text_rgb);
        else if (strncmp(line, "LOOK_ACCENT=", 12) == 0) parse_hex_rgb(line + 12, app->border_act_rgb);
        else if (strncmp(line, "LOOK_CORNERS=", 13) == 0) {
            const char *v = line + 13;
            while (*v == '"') v++;
            if (*v >= '0' && *v <= '9') app->corners = atoi(v) > 28 ? 28 : atoi(v);
        }
    }
    fclose(f);
}

static void load_theme_config(LockApp *app) {
    app->canvas_rgb[0] = 0.043; app->canvas_rgb[1] = 0.059; app->canvas_rgb[2] = 0.078;       /* #0b0f14 */
    app->surface_rgb[0] = 0.067; app->surface_rgb[1] = 0.075; app->surface_rgb[2] = 0.090;     /* #111317 */
    app->border_act_rgb[0] = 0.478; app->border_act_rgb[1] = 0.635; app->border_act_rgb[2] = 0.969; /* #7aa2f7 */
    app->border_inact_rgb[0] = 0.200; app->border_inact_rgb[1] = 0.231; app->border_inact_rgb[2] = 0.329; /* #333b54 */
    app->text_rgb[0] = 0.933; app->text_rgb[1] = 0.949; app->text_rgb[2] = 0.973;             /* #eef2f8 */
    app->corners = 8; /* the corners of windows and the edge: 8 unless the look file says */

    const char *home = getenv("HOME");
    if (!home) return;
    char path[512];
    snprintf(path, sizeof(path), "%s/.config/infynitae/config", home);
    FILE *f = fopen(path, "r");
    if (!f) return;
    char line[256];
    while (fgets(line, sizeof(line), f)) {
        char *p = line;
        while (*p == ' ' || *p == '\t') p++;
        if (*p == '#') continue;
        if (strncmp(p, "CANVAS_COLOR=", 13) == 0) parse_hex_rgb(p + 13, app->canvas_rgb);
        else if (strncmp(p, "SURFACE_COLOR=", 14) == 0) parse_hex_rgb(p + 14, app->surface_rgb);
        else if (strncmp(p, "BORDER_ACTIVE=", 14) == 0) parse_hex_rgb(p + 14, app->border_act_rgb);
        else if (strncmp(p, "BORDER_INACTIVE=", 16) == 0) parse_hex_rgb(p + 16, app->border_inact_rgb);
        else if (strncmp(p, "TEXT_COLOR=", 11) == 0) parse_hex_rgb(p + 11, app->text_rgb);
    }
    fclose(f);
    read_look(app, home);
}

/* ========================================================================== */
/* UI Rendering (Cairo & Pango)                                               */
/* ========================================================================== */
static void draw_sharp_rect(cairo_t *cr, double x, double y, double w, double h) {
    cairo_rectangle(cr, round(x) + 0.5, round(y) + 0.5, round(w) - 1.0, round(h) - 1.0);
}

/* Draws `text` centered at `(cx, cy)` using exact Pango sub-pixel extents.
 * When `use_ink_y` is true (single glyphs, uppercase badges, numerals), vertical centering uses
 * the actual ink bounding box so ascenders/descenders never push the glyph off-center.
 * When `use_ink_y` is false (mixed-case phrases), vertical centering uses the logical box while
 * horizontal centering still uses the ink box so left/right bearings never shift text sideways. */
static void draw_text_centered(cairo_t *cr, PangoLayout *layout, const char *font_str,
                               const char *text, double cx, double cy, bool use_ink_y) {
    PangoFontDescription *desc = pango_font_description_from_string(font_str);
    pango_layout_set_font_description(layout, desc);
    pango_font_description_free(desc);
    pango_layout_set_text(layout, text, -1);

    PangoRectangle ink, logical;
    pango_layout_get_extents(layout, &ink, &logical);

    double ix = (double)ink.x / PANGO_SCALE;
    double iw = (double)ink.width / PANGO_SCALE;
    double x = cx - (ix + iw / 2.0);

    double y;
    if (use_ink_y) {
        double iy = (double)ink.y / PANGO_SCALE;
        double ih = (double)ink.height / PANGO_SCALE;
        y = cy - (iy + ih / 2.0);
    } else {
        double ly = (double)logical.y / PANGO_SCALE;
        double lh = (double)logical.height / PANGO_SCALE;
        y = cy - (ly + lh / 2.0);
    }

    cairo_move_to(cr, round(x), round(y));  /* letters on whole pixels */
    pango_cairo_show_layout(cr, layout);
}

/* ========================================================================== */
/* On-screen keyboard (touchscreens, tablet mode)                             */
/* ========================================================================== */
/* A key grid drawn on the lock surface itself: the compositor gives the lock surface the taps
 * on its screen and nothing else sees them, so the password never leaves this program (no
 * input method or virtual keyboard reaches a lock screen). It appears at the first tap and
 * types characters directly, whatever the keyboard layout. A key acts when the finger lifts
 * on it. */
typedef enum { OSK_CHAR, OSK_SHIFT, OSK_BACKSPACE, OSK_SYMBOLS, OSK_SPACE, OSK_HIDE, OSK_ENTER } OskKind;

typedef struct {
    OskKind kind;
    char ch;
    double x, y, w, h;
} OskKey;

#define OSK_MAX_KEYS 48

static const char *osk_letters[4] = { "1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm" };
static const char *osk_symbols[4] = { "1234567890", "!@#$%^&*()", "-_=+;:'\"/", ".,?<>\\|" };

/* The grid for a w x h surface, bottom-centred: 10 units wide, 5 rows. Returns the key count and
 * the grid's top in *top. */
static int osk_layout(const LockApp *app, int w, int h, OskKey keys[OSK_MAX_KEYS], double *top) {
    const char **rows = app->osk_symbols ? osk_symbols : osk_letters;
    double u = fmin(w * 0.96 / 10.0, 76.0);
    double kh = fmin(u * 0.92, 64.0);
    double gap = 6.0;
    double x0 = (w - u * 10.0) / 2.0;
    double y0 = h - 24.0 - 5 * kh;
    int n = 0;
    *top = y0 - 12.0;
#define OSK_ADD(k, c, cx, units, row) do { \
        keys[n].kind = (k); keys[n].ch = (c); \
        keys[n].x = x0 + (cx) * u + gap / 2; keys[n].y = y0 + (row) * kh + gap / 2; \
        keys[n].w = (units) * u - gap; keys[n].h = kh - gap; n++; \
    } while (0)
    for (int r = 0; r < 3; r++) {
        size_t len = strlen(rows[r]);
        double start = (10.0 - (double)len) / 2.0;
        for (size_t i = 0; i < len; i++)
            OSK_ADD(OSK_CHAR, rows[r][i], start + (double)i, 1.0, r);
    }
    OSK_ADD(OSK_SHIFT, 0, 0.0, 1.5, 3);
    for (size_t i = 0; i < strlen(rows[3]); i++)
        OSK_ADD(OSK_CHAR, rows[3][i], 1.5 + (double)i, 1.0, 3);
    OSK_ADD(OSK_BACKSPACE, 0, 8.5, 1.5, 3);
    OSK_ADD(OSK_SYMBOLS, 0, 0.0, 1.5, 4);
    OSK_ADD(OSK_SPACE, ' ', 1.5, 5.0, 4);
    OSK_ADD(OSK_HIDE, 0, 6.5, 1.5, 4);
    OSK_ADD(OSK_ENTER, 0, 8.0, 2.0, 4);
#undef OSK_ADD
    return n;
}

static char osk_char(const LockApp *app, char c) {
    return (app->osk_shift && c >= 'a' && c <= 'z') ? (char)(c - 32) : c;
}

static void osk_label(const LockApp *app, const OskKey *k, char out[8]) {
    switch (k->kind) {
    case OSK_CHAR: out[0] = osk_char(app, k->ch); out[1] = '\0'; break;
    case OSK_SHIFT: strcpy(out, "⇧"); break;
    case OSK_BACKSPACE: strcpy(out, "⌫"); break;
    case OSK_SYMBOLS: strcpy(out, app->osk_symbols ? "ABC" : "?123"); break;
    case OSK_SPACE: strcpy(out, ""); break;
    case OSK_HIDE: strcpy(out, "⌄"); break;
    case OSK_ENTER: strcpy(out, "⏎"); break;
    default: out[0] = '\0'; break;
    }
}

static int osk_hit(const OskKey keys[], int n, double x, double y) {
    for (int i = 0; i < n; i++)
        if (x >= keys[i].x && x < keys[i].x + keys[i].w && y >= keys[i].y && y < keys[i].y + keys[i].h)
            return i;
    return -1;
}

static void osk_draw(cairo_t *cr, PangoLayout *layout, const LockApp *app, const LockOutput *output,
                     int w, int h) {
    OskKey keys[OSK_MAX_KEYS];
    double top;
    int n = osk_layout(app, w, h, keys, &top);
    cairo_set_source_rgba(cr, app->surface_rgb[0], app->surface_rgb[1], app->surface_rgb[2], 0.96);
    cairo_rectangle(cr, 0, top, w, h - top);
    cairo_fill(cr);
    cairo_set_source_rgba(cr, app->border_inact_rgb[0], app->border_inact_rgb[1], app->border_inact_rgb[2], 0.85);
    cairo_set_line_width(cr, 1.0);
    cairo_move_to(cr, 0, round(top) + 0.5);
    cairo_line_to(cr, w, round(top) + 0.5);
    cairo_stroke(cr);

    PangoFontDescription *desc = pango_font_description_from_string("Sans 14");
    pango_layout_set_font_description(layout, desc);
    pango_font_description_free(desc);
    for (int i = 0; i < n; i++) {
        bool pressed = app->osk_pressed == i && app->osk_pressed_output == output;
        bool lit = keys[i].kind == OSK_SHIFT && app->osk_shift;
        if (pressed || lit)
            cairo_set_source_rgba(cr, 0.16, 0.19, 0.25, 0.96);
        else
            cairo_set_source_rgba(cr, app->canvas_rgb[0], app->canvas_rgb[1], app->canvas_rgb[2], 0.95);
        cairo_rectangle(cr, round(keys[i].x), round(keys[i].y), round(keys[i].w), round(keys[i].h));
        cairo_fill(cr);

        if (pressed || lit)
            cairo_set_source_rgba(cr, app->border_act_rgb[0], app->border_act_rgb[1], app->border_act_rgb[2], 0.85);
        else
            cairo_set_source_rgba(cr, app->border_inact_rgb[0], app->border_inact_rgb[1], app->border_inact_rgb[2], 0.65);
        cairo_set_line_width(cr, 1.0);
        draw_sharp_rect(cr, keys[i].x, keys[i].y, keys[i].w, keys[i].h);
        cairo_stroke(cr);

        char label[8];
        osk_label(app, &keys[i], label);
        cairo_set_source_rgba(cr, app->text_rgb[0], app->text_rgb[1], app->text_rgb[2], 0.94);
        draw_text_centered(cr, layout, "Sans 14", label,
                           keys[i].x + keys[i].w / 2.0,
                           keys[i].y + keys[i].h / 2.0, true);
    }
}

static void osk_type(LockApp *app, char c) {
    if (app->password_len + 1 < PASSWORD_MAX_LEN - 1) {
        app->password[app->password_len++] = c;
        app->password[app->password_len] = '\0';
        app->auth_state = AUTH_STATE_TYPING;
    }
}

/* A key's action, when the finger lifts on it. */
static void osk_activate(LockApp *app, const OskKey *k) {
    if (app->auth_state == AUTH_STATE_VERIFYING || app->status_msg[0] != '\0') return;
    switch (k->kind) {
    case OSK_CHAR:
        osk_type(app, osk_char(app, k->ch));
        app->osk_shift = false; /* one capital, as phones do */
        break;
    case OSK_SPACE: osk_type(app, ' '); break;
    case OSK_SHIFT: app->osk_shift = !app->osk_shift; break;
    case OSK_SYMBOLS: app->osk_symbols = !app->osk_symbols; break;
    case OSK_HIDE: app->osk_visible = false; break;
    case OSK_BACKSPACE:
        if (app->password_len > 0) {
            app->password[--app->password_len] = '\0';
            app->auth_state = app->password_len > 0 ? AUTH_STATE_TYPING : AUTH_STATE_IDLE;
        }
        break;
    case OSK_ENTER:
        if (app->password_len > 0) trigger_auth(app);
        break;
    }
}

static LockOutput *output_of_surface(LockApp *app, struct wl_surface *surface) {
    for (LockOutput *o = app->outputs; o; o = o->next)
        if (o->surface == surface) return o;
    return NULL;
}

static void touch_down(void *data, struct wl_touch *touch, uint32_t serial, uint32_t time,
                       struct wl_surface *surface, int32_t id, wl_fixed_t x, wl_fixed_t y) {
    (void)touch; (void)serial; (void)time;
    LockApp *app = data;
    LockOutput *o = output_of_surface(app, surface);
    if (!o || app->osk_pressed >= 0 || app->status_msg[0] != '\0') return;
    if (!app->osk_visible) {
        /* the first tap brings the keyboard */
        app->osk_visible = true;
        damage_all_outputs(app);
        return;
    }
    OskKey keys[OSK_MAX_KEYS];
    double top;
    int n = osk_layout(app, o->width, o->height, keys, &top);
    int i = osk_hit(keys, n, wl_fixed_to_double(x), wl_fixed_to_double(y));
    if (i < 0) return;
    app->osk_pressed = i;
    app->osk_pressed_output = o;
    app->osk_touch_id = id;
    app->osk_touch_x = wl_fixed_to_double(x);
    app->osk_touch_y = wl_fixed_to_double(y);
    request_frame(o);
}

static void touch_motion(void *data, struct wl_touch *touch, uint32_t time, int32_t id,
                         wl_fixed_t x, wl_fixed_t y) {
    (void)touch; (void)time;
    LockApp *app = data;
    if (app->osk_pressed >= 0 && id == app->osk_touch_id) {
        app->osk_touch_x = wl_fixed_to_double(x);
        app->osk_touch_y = wl_fixed_to_double(y);
    }
}

static void touch_up(void *data, struct wl_touch *touch, uint32_t serial, uint32_t time, int32_t id) {
    (void)touch; (void)serial; (void)time;
    LockApp *app = data;
    if (app->osk_pressed < 0 || id != app->osk_touch_id) return;
    LockOutput *o = app->osk_pressed_output;
    OskKey keys[OSK_MAX_KEYS];
    double top;
    int n = osk_layout(app, o->width, o->height, keys, &top);
    int pressed = app->osk_pressed;
    app->osk_pressed = -1;
    app->osk_pressed_output = NULL;
    /* only if the finger lifts on the key it pressed */
    if (pressed < n && osk_hit(keys, n, app->osk_touch_x, app->osk_touch_y) == pressed)
        osk_activate(app, &keys[pressed]);
    damage_all_outputs(app);
}

static void touch_frame(void *data, struct wl_touch *touch) {
    (void)data; (void)touch;
}

static void touch_cancel(void *data, struct wl_touch *touch) {
    (void)touch;
    LockApp *app = data;
    app->osk_pressed = -1;
    app->osk_pressed_output = NULL;
    damage_all_outputs(app);
}

static const struct wl_touch_listener touch_listener = {
    .down = touch_down,
    .up = touch_up,
    .motion = touch_motion,
    .frame = touch_frame,
    .cancel = touch_cancel,
};

/* A rectangle rounded by the system's corners (square at 0), at most a pill, as the current path. */
static void scene_rect(cairo_t *cr, const LockApp *app, double x, double y, double w, double h) {
    double r = app->corners;
    if (r <= 0) {
        cairo_rectangle(cr, x, y, w, h);
        return;
    }
    if (r > h / 2) r = h / 2;
    cairo_new_sub_path(cr);
    cairo_arc(cr, x + w - r, y + r, r, -M_PI / 2, 0);
    cairo_arc(cr, x + w - r, y + h - r, r, 0, M_PI / 2);
    cairo_arc(cr, x + r, y + h - r, r, M_PI / 2, M_PI);
    cairo_arc(cr, x + r, y + r, r, M_PI, 3 * M_PI / 2);
    cairo_close_path(cr);
}

/* The background at w x h: the blurred wallpaper scaled to cover the screen and darkened toward
 * CANVAS_COLOR, or the canvas colour alone. Drawn once per screen size, not per key press. */
static void draw_background(cairo_t *cr, const LockApp *app, int w, int h) {
    cairo_set_source_rgb(cr, app->canvas_rgb[0], app->canvas_rgb[1], app->canvas_rgb[2]);
    cairo_paint(cr);
    if (app->background) {
        int iw = cairo_image_surface_get_width(app->background);
        int ih = cairo_image_surface_get_height(app->background);
        if (iw > 0 && ih > 0) {
            double sc = fmax((double)w / iw, (double)h / ih);
            cairo_save(cr);
            cairo_translate(cr, (w - iw * sc) / 2.0, (h - ih * sc) / 2.0);
            cairo_scale(cr, sc, sc);
            cairo_set_source_surface(cr, app->background, 0, 0);
            /* bilinear: cairo's GOOD scales up through a box filter, which showed blocks */
            cairo_pattern_set_filter(cairo_get_source(cr), CAIRO_FILTER_BILINEAR);
            cairo_pattern_set_extend(cairo_get_source(cr), CAIRO_EXTEND_PAD);
            cairo_paint(cr);
            cairo_restore(cr);
            cairo_set_source_rgba(cr, app->canvas_rgb[0], app->canvas_rgb[1], app->canvas_rgb[2], 0.55);
            cairo_paint(cr);
        }
    } else {
        /* a soft light from the centre in the surface colour */
        cairo_pattern_t *pat = cairo_pattern_create_radial(w / 2.0, h * 0.42, 0, w / 2.0, h * 0.42, fmax(w, h) * 0.75);
        cairo_pattern_add_color_stop_rgba(pat, 0.0, app->surface_rgb[0], app->surface_rgb[1], app->surface_rgb[2], 0.9);
        cairo_pattern_add_color_stop_rgba(pat, 1.0, app->surface_rgb[0], app->surface_rgb[1], app->surface_rgb[2], 0.0);
        cairo_set_source(cr, pat);
        cairo_paint(cr);
        cairo_pattern_destroy(pat);
    }
}

/* Everything on top of the background, for a screen of w x h. */
static void draw_scene(cairo_t *cr, LockApp *app, const LockOutput *output, int w, int h) {
    const double *tx = app->text_rgb;
    // Shake offset after a wrong password
    double shake_offset = 0;
    if (app->shake_frame > 0) {
        double decay = (double)app->shake_frame / SHAKE_DURATION_FRAMES;
        shake_offset = sin(app->shake_frame * 1.5) * 16.0 * decay;
    }
    double cx = w / 2.0;
    double clock_y = h * 0.30;
    double avatar_r = 48.0;
    double avatar_y = h * 0.56;
    if (app->osk_visible) {
        /* the password stays above the keyboard: everything moves up together */
        OskKey keys[OSK_MAX_KEYS];
        double kb_top;
        osk_layout(app, w, h, keys, &kb_top);
        double lowest = avatar_y + avatar_r + 16 + 26 + 16 + 44 + 30;
        if (lowest > kb_top) {
            double up = lowest - kb_top;
            avatar_y -= up;
            clock_y -= up;
            if (clock_y < 80) clock_y = 80;
        }
    }

    /* hinted, whole-pixel glyph advances (as the panels: the same text lines up the same way) */
    cairo_font_options_t *fo = cairo_font_options_create();
    cairo_font_options_set_hint_metrics(fo, CAIRO_HINT_METRICS_ON);
    cairo_font_options_set_hint_style(fo, CAIRO_HINT_STYLE_SLIGHT);
    cairo_font_options_set_antialias(fo, CAIRO_ANTIALIAS_GRAY);
    cairo_set_font_options(cr, fo);
    cairo_font_options_destroy(fo);
    PangoLayout *layout = pango_cairo_create_layout(cr);

    // Clock and date (tabular digits so the colon never moves)
    time_t now = time(NULL);
    struct tm *tm = localtime(&now);
    char time_str[64], date_str[128];
    strftime(time_str, sizeof(time_str), "%H:%M", tm);
    strftime(date_str, sizeof(date_str), "%A, %B %-d", tm);
    PangoAttrList *clock_attrs = pango_attr_list_new();
    pango_attr_list_insert(clock_attrs, pango_attr_font_features_new("tnum=1"));
    pango_layout_set_attributes(layout, clock_attrs);
    cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.96);
    draw_text_centered(cr, layout, "sans-serif Light 72", time_str, cx, clock_y, true);
    pango_layout_set_attributes(layout, NULL);
    pango_attr_list_unref(clock_attrs);
    cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.66);
    draw_text_centered(cr, layout, "sans-serif 13.5", date_str, cx, clock_y + 70.0, false);

    double ax = cx + shake_offset;

    // The user's picture, or initials on an accent disc
    if (app->avatar) {
        int iw = cairo_image_surface_get_width(app->avatar);
        int ih = cairo_image_surface_get_height(app->avatar);
        cairo_save(cr);
        cairo_new_path(cr);
        cairo_arc(cr, ax, avatar_y, avatar_r, 0, 2 * M_PI);
        cairo_clip(cr);
        if (iw > 0 && ih > 0) {
            double sc = (2 * avatar_r) / fmin(iw, ih);
            cairo_translate(cr, ax - iw * sc / 2.0, avatar_y - ih * sc / 2.0);
            cairo_scale(cr, sc, sc);
            cairo_set_source_surface(cr, app->avatar, 0, 0);
            cairo_pattern_set_filter(cairo_get_source(cr), CAIRO_FILTER_GOOD);
            cairo_paint(cr);
        }
        cairo_restore(cr);
    } else {
        cairo_set_source_rgba(cr, app->border_act_rgb[0], app->border_act_rgb[1], app->border_act_rgb[2], 0.22);
        cairo_new_path(cr);
        cairo_arc(cr, ax, avatar_y, avatar_r, 0, 2 * M_PI);
        cairo_fill(cr);
        cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.94);
        draw_text_centered(cr, layout, "sans-serif Medium 26", app->initials, ax, avatar_y, true);
    }

    // The ring: a hairline, a spinning arc while busy, red after a wrong password
    bool busy = (app->auth_state == AUTH_STATE_VERIFYING ||
                 app->auth_state == AUTH_STATE_SUCCESS ||
                 app->status_msg[0] != '\0');
    cairo_set_line_cap(cr, CAIRO_LINE_CAP_ROUND);
    if (busy) {
        cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.14);
        cairo_set_line_width(cr, 2.0);
        cairo_new_path(cr);
        cairo_arc(cr, ax, avatar_y, avatar_r + 3, 0, 2 * M_PI);
        cairo_stroke(cr);
        cairo_set_source_rgba(cr, app->border_act_rgb[0], app->border_act_rgb[1], app->border_act_rgb[2], 0.95);
        cairo_set_line_width(cr, 2.5);
        cairo_new_path(cr);
        cairo_arc(cr, ax, avatar_y, avatar_r + 3, app->pulse_phase, app->pulse_phase + 1.8);
        cairo_stroke(cr);
    } else if (app->auth_state == AUTH_STATE_FAILED) {
        cairo_set_source_rgba(cr, 0.97, 0.46, 0.56, 0.95);
        cairo_set_line_width(cr, 2.0);
        cairo_new_path(cr);
        cairo_arc(cr, ax, avatar_y, avatar_r + 3, 0, 2 * M_PI);
        cairo_stroke(cr);
    } else {
        cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.16);
        cairo_set_line_width(cr, 1.0);
        cairo_new_path(cr);
        cairo_arc(cr, ax, avatar_y, avatar_r + 2, 0, 2 * M_PI);
        cairo_stroke(cr);
    }

    // The name
    double name_y = avatar_y + avatar_r + 16.0 + 13.0;
    cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.92);
    draw_text_centered(cr, layout, "sans-serif Medium 13.5", app->display_name, ax, name_y, false);

    // The password field
    double field_w = 320.0, field_h = 44.0;
    double field_x = round(ax - field_w / 2.0);
    double field_y = round(name_y + 13.0 + 16.0);
    double field_cy = field_y + field_h / 2.0;
    cairo_new_path(cr);
    scene_rect(cr, app, field_x, field_y, field_w, field_h);
    cairo_set_source_rgba(cr, app->surface_rgb[0], app->surface_rgb[1], app->surface_rgb[2], app->background ? 0.70 : 0.92);
    cairo_fill_preserve(cr);
    cairo_set_line_width(cr, 1.0);
    if (app->auth_state == AUTH_STATE_FAILED) {
        cairo_set_source_rgba(cr, 0.97, 0.46, 0.56, 0.90);
    } else if (busy || app->password_len > 0) {
        cairo_set_source_rgba(cr, app->border_act_rgb[0], app->border_act_rgb[1], app->border_act_rgb[2], 0.90);
    } else {
        cairo_set_source_rgba(cr, app->border_inact_rgb[0], app->border_inact_rgb[1], app->border_inact_rgb[2], 0.90);
    }
    cairo_stroke(cr);

    if (app->status_msg[0] != '\0') {
        cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.94);
        draw_text_centered(cr, layout, "sans-serif 11", app->status_msg, ax, field_cy, false);
    } else if (app->auth_state == AUTH_STATE_SUCCESS) {
        cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.94);
        draw_text_centered(cr, layout, "sans-serif 11", "Unlocking", ax, field_cy, false);
    } else if (app->auth_state == AUTH_STATE_VERIFYING) {
        cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.80);
        draw_text_centered(cr, layout, "sans-serif 11", "Checking", ax, field_cy, false);
    } else if (app->password_len > 0) {
        double dot_spacing = 14.0;
        size_t max_dots = 18;
        size_t visible_dots = app->password_len < max_dots ? app->password_len : max_dots;
        double total_dots_w = visible_dots > 0 ? (visible_dots - 1) * dot_spacing : 0;
        double start_dot_x = ax - (total_dots_w / 2.0);
        cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.94);
        for (size_t i = 0; i < visible_dots; ++i) {
            cairo_new_path(cr);
            cairo_arc(cr, start_dot_x + (i * dot_spacing), field_cy, 3.5, 0, 2 * M_PI);
            cairo_fill(cr);
        }
    } else {
        cairo_set_source_rgba(cr, tx[0], tx[1], tx[2], 0.45);
        draw_text_centered(cr, layout, "sans-serif 11", "Password", ax, field_cy, false);
    }

    // One line under the field: the error, or caps lock
    double note_y = field_y + field_h + 22.0;
    if (app->auth_state == AUTH_STATE_FAILED && app->password_len == 0) {
        cairo_set_source_rgba(cr, 0.97, 0.46, 0.56, 0.95);
        draw_text_centered(cr, layout, "sans-serif 10.5", "Wrong password, try again", ax, note_y, false);
    } else if (app->caps_lock) {
        cairo_set_source_rgba(cr, 0.88, 0.69, 0.41, 0.95);
        draw_text_centered(cr, layout, "sans-serif 10.5", "Caps Lock is on", ax, note_y, false);
    }

    if (app->osk_visible) osk_draw(cr, layout, app, output, w, h);
    g_object_unref(layout);
}

static void render_output(LockOutput *output) {
    if (!output->configured || output->width <= 0 || output->height <= 0) return;
    if (!allocate_output_buffer(output)) return;

    LockApp *app = output->app;
    int w = output->width;
    int h = output->height;

    // the background for this size, drawn again only when the size or the picture changed
    if (!output->scene_bg || output->scene_bg_w != w || output->scene_bg_h != h ||
        output->scene_bg_from != app->background) {
        if (output->scene_bg) cairo_surface_destroy(output->scene_bg);
        output->scene_bg = cairo_image_surface_create(CAIRO_FORMAT_RGB24, w, h);
        cairo_t *bg_cr = cairo_create(output->scene_bg);
        draw_background(bg_cr, app, w, h);
        cairo_destroy(bg_cr);
        output->scene_bg_w = w;
        output->scene_bg_h = h;
        output->scene_bg_from = app->background;
    }

    cairo_surface_t *surf = cairo_image_surface_create_for_data(
        (unsigned char *)output->shm_data, CAIRO_FORMAT_ARGB32,
        output->width, output->height, output->width * 4);
    cairo_t *cr = cairo_create(surf);
    cairo_set_source_surface(cr, output->scene_bg, 0, 0);
    cairo_set_operator(cr, CAIRO_OPERATOR_SOURCE);
    cairo_paint(cr);
    cairo_set_operator(cr, CAIRO_OPERATOR_OVER);
    draw_scene(cr, app, output, w, h);
    cairo_destroy(cr);
    cairo_surface_destroy(surf);

    // Commit buffer to Wayland surface
    wl_surface_attach(output->surface, output->buffer, 0, 0);
    wl_surface_damage_buffer(output->surface, 0, 0, output->width, output->height);
    wl_surface_commit(output->surface);
}

static void on_frame_done(void *data, struct wl_callback *cb, uint32_t time_ms) {
    (void)time_ms;
    LockOutput *output = (LockOutput *)data;
    if (cb) wl_callback_destroy(cb);
    output->frame_cb = NULL;

    LockApp *app = output->app;
    if (!app->running) return;

    if (app->shake_frame > 0) {
        app->shake_frame--;
        render_output(output);
        request_frame(output);
    } else if (app->auth_state == AUTH_STATE_VERIFYING || app->status_msg[0] != '\0') {
        app->pulse_phase += 0.14;
        render_output(output);
        request_frame(output);
    } else {
        render_output(output);
    }
}

static const struct wl_callback_listener frame_listener = {
    .done = on_frame_done
};

static void request_frame(LockOutput *output) {
    // a lock surface must not commit before its first configure (protocol error)
    if (output->frame_cb != NULL || !output->configured) return;
    output->frame_cb = wl_surface_frame(output->surface);
    wl_callback_add_listener(output->frame_cb, &frame_listener, output);
    wl_surface_commit(output->surface);
}

static void damage_all_outputs(LockApp *app) {
    for (LockOutput *o = app->outputs; o != NULL; o = o->next) {
        render_output(o);
        if (app->shake_frame > 0 || app->auth_state == AUTH_STATE_VERIFYING || app->status_msg[0] != '\0') {
            request_frame(o);
        }
    }
}

/* ========================================================================== */
/* Session Lock (ext-session-lock-v1)                                         */
/* ========================================================================== */
static void lock_surface_configure(void *data, struct ext_session_lock_surface_v1 *lock_surface,
                                   uint32_t serial, uint32_t width, uint32_t height) {
    LockOutput *output = (LockOutput *)data;
    output->width = (int32_t)width;
    output->height = (int32_t)height;
    output->configured = true;

    ext_session_lock_surface_v1_ack_configure(lock_surface, serial);
    render_output(output);
    if (output->app->status_msg[0] != '\0') {
        request_frame(output);
    }
}

static const struct ext_session_lock_surface_v1_listener lock_surface_listener = {
    .configure = lock_surface_configure,
};

static void setup_lock_surface(LockOutput *output) {
    LockApp *app = output->app;
    if (!app->lock || output->lock_surface) return;
    output->surface = wl_compositor_create_surface(app->compositor);
    output->lock_surface = ext_session_lock_v1_get_lock_surface(app->lock, output->surface, output->wl_output);
    ext_session_lock_surface_v1_add_listener(output->lock_surface, &lock_surface_listener, output);
    // no commit here: the first commit waits for the configure
}

/* Tells infynitae-keyring that the screen is locked, so it forgets its key until the unlock (the PAM
 * service's pam_exec line hands it the password again). Nothing happens when it does not run: no socket. */
static void keyring_lock(void) {
    const char *run = getenv("XDG_RUNTIME_DIR");
    char path[sizeof(((struct sockaddr_un *)0)->sun_path)];
    int n = run && *run ? snprintf(path, sizeof(path), "%s/infynitae-keyring/control", run)
                        : snprintf(path, sizeof(path), "/run/user/%u/infynitae-keyring/control", (unsigned)getuid());
    if (n < 0 || (size_t)n >= sizeof(path)) return;
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return;
    struct sockaddr_un addr = { .sun_family = AF_UNIX };
    memcpy(addr.sun_path, path, (size_t)n + 1);
    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) == 0) {
        if (write(fd, "lock\n", 5) == 5) shutdown(fd, SHUT_WR);
    }
    close(fd);
}

static void lock_locked(void *data, struct ext_session_lock_v1 *lock) {
    (void)lock;
    ((LockApp *)data)->locked = true;
    keyring_lock();
}

static void lock_finished(void *data, struct ext_session_lock_v1 *lock) {
    (void)lock;
    // the compositor refused the lock (another locker holds it) or ended it
    ((LockApp *)data)->lock_refused = true;
}

static const struct ext_session_lock_v1_listener lock_listener = {
    .locked = lock_locked,
    .finished = lock_finished,
};

/* ========================================================================== */
/* Keyboard Input (XKB)                                                       */
/* ========================================================================== */
static void keyboard_keymap(void *data, struct wl_keyboard *keyboard,
                            uint32_t format, int32_t fd, uint32_t size) {
    (void)keyboard;
    LockApp *app = (LockApp *)data;
    if (format != WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1) {
        close(fd);
        return;
    }
    char *map_str = mmap(NULL, size, PROT_READ, MAP_SHARED, fd, 0);
    if (map_str == MAP_FAILED) {
        close(fd);
        return;
    }
    if (app->xkb_keymap) xkb_keymap_unref(app->xkb_keymap);
    if (app->xkb_state) xkb_state_unref(app->xkb_state);

    app->xkb_keymap = xkb_keymap_new_from_string(app->xkb_ctx, map_str,
        XKB_KEYMAP_FORMAT_TEXT_V1, XKB_KEYMAP_COMPILE_NO_FLAGS);
    munmap(map_str, size);
    close(fd);

    if (app->xkb_keymap) {
        app->xkb_state = xkb_state_new(app->xkb_keymap);
    }
}

static void keyboard_enter(void *data, struct wl_keyboard *keyboard,
                           uint32_t serial, struct wl_surface *surface, struct wl_array *keys) {
    (void)keyboard; (void)serial; (void)surface; (void)keys; (void)data;
}

static void keyboard_leave(void *data, struct wl_keyboard *keyboard,
                           uint32_t serial, struct wl_surface *surface) {
    (void)keyboard; (void)serial; (void)surface; (void)data;
}

static void keyboard_key(void *data, struct wl_keyboard *keyboard,
                         uint32_t serial, uint32_t time_ms, uint32_t key, uint32_t state) {
    (void)keyboard; (void)serial; (void)time_ms;
    LockApp *app = (LockApp *)data;

    if (state != WL_KEYBOARD_KEY_STATE_PRESSED || !app->xkb_state) return;
    if (app->auth_state == AUTH_STATE_VERIFYING || app->status_msg[0] != '\0') return;

    uint32_t keycode = key + 8; // evdev -> xkb translation offset
    xkb_keysym_t sym = xkb_state_key_get_one_sym(app->xkb_state, keycode);

    if (sym == XKB_KEY_Return || sym == XKB_KEY_KP_Enter) {
        if (app->password_len > 0) {
            trigger_auth(app);
            damage_all_outputs(app);
        }
        return;
    }

    if (sym == XKB_KEY_BackSpace) {
        if (app->password_len > 0) {
            app->password[--app->password_len] = '\0';
            app->auth_state = app->password_len > 0 ? AUTH_STATE_TYPING : AUTH_STATE_IDLE;
            damage_all_outputs(app);
        }
        return;
    }

    if (sym == XKB_KEY_Escape) {
        explicit_bzero(app->password, sizeof(app->password));
        app->password_len = 0;
        app->auth_state = AUTH_STATE_IDLE;
        damage_all_outputs(app);
        return;
    }

    // Ctrl+U clears line
    if ((sym == XKB_KEY_u || sym == XKB_KEY_U) &&
        xkb_state_mod_name_is_active(app->xkb_state, XKB_MOD_NAME_CTRL, XKB_STATE_MODS_DEPRESSED)) {
        explicit_bzero(app->password, sizeof(app->password));
        app->password_len = 0;
        app->auth_state = AUTH_STATE_IDLE;
        damage_all_outputs(app);
        return;
    }

    // Printable character input
    char buf[16] = {0};
    int len = xkb_state_key_get_utf8(app->xkb_state, keycode, buf, sizeof(buf));
    if (len > 0 && (unsigned char)buf[0] >= 32 && (unsigned char)buf[0] != 127) {
        if (app->password_len + (size_t)len < PASSWORD_MAX_LEN - 1) {
            memcpy(&app->password[app->password_len], buf, len);
            app->password_len += len;
            app->password[app->password_len] = '\0';
            app->auth_state = AUTH_STATE_TYPING;
            damage_all_outputs(app);
        }
    }
    explicit_bzero(buf, sizeof(buf));
}

static void keyboard_modifiers(void *data, struct wl_keyboard *keyboard,
                               uint32_t serial, uint32_t mods_depressed,
                               uint32_t mods_latched, uint32_t mods_locked, uint32_t group) {
    (void)keyboard; (void)serial;
    LockApp *app = (LockApp *)data;
    if (!app->xkb_state) return;

    xkb_state_update_mask(app->xkb_state, mods_depressed, mods_latched, mods_locked, 0, 0, group);
    bool prev_caps = app->caps_lock;
    app->caps_lock = xkb_state_mod_name_is_active(app->xkb_state, XKB_MOD_NAME_CAPS, XKB_STATE_MODS_LOCKED);
    if (app->caps_lock != prev_caps) {
        damage_all_outputs(app);
    }
}

static void keyboard_repeat_info(void *data, struct wl_keyboard *keyboard, int32_t rate, int32_t delay) {
    (void)data; (void)keyboard; (void)rate; (void)delay;
}

static const struct wl_keyboard_listener keyboard_listener = {
    .keymap = keyboard_keymap,
    .enter = keyboard_enter,
    .leave = keyboard_leave,
    .key = keyboard_key,
    .modifiers = keyboard_modifiers,
    .repeat_info = keyboard_repeat_info,
};

static void seat_capabilities(void *data, struct wl_seat *seat, uint32_t capabilities) {
    LockApp *app = (LockApp *)data;
    if ((capabilities & WL_SEAT_CAPABILITY_KEYBOARD) && !app->keyboard) {
        app->keyboard = wl_seat_get_keyboard(seat);
        wl_keyboard_add_listener(app->keyboard, &keyboard_listener, app);
    } else if (!(capabilities & WL_SEAT_CAPABILITY_KEYBOARD) && app->keyboard) {
        wl_keyboard_destroy(app->keyboard);
        app->keyboard = NULL;
    }
    if ((capabilities & WL_SEAT_CAPABILITY_TOUCH) && !app->touch) {
        app->touch = wl_seat_get_touch(seat);
        wl_touch_add_listener(app->touch, &touch_listener, app);
    } else if (!(capabilities & WL_SEAT_CAPABILITY_TOUCH) && app->touch) {
        wl_touch_destroy(app->touch);
        app->touch = NULL;
    }
}

static void seat_name(void *data, struct wl_seat *seat, const char *name) {
    (void)data; (void)seat; (void)name;
}

static const struct wl_seat_listener seat_listener = {
    .capabilities = seat_capabilities,
    .name = seat_name,
};

/* ========================================================================== */
/* Registry Listener                                                          */
/* ========================================================================== */
static void registry_global(void *data, struct wl_registry *registry,
                            uint32_t name, const char *interface, uint32_t version) {
    (void)version;
    LockApp *app = (LockApp *)data;

    if (strcmp(interface, wl_compositor_interface.name) == 0) {
        app->compositor = wl_registry_bind(registry, name, &wl_compositor_interface, 4);
    } else if (strcmp(interface, wl_shm_interface.name) == 0) {
        app->shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    } else if (strcmp(interface, ext_session_lock_manager_v1_interface.name) == 0) {
        app->lock_manager = wl_registry_bind(registry, name, &ext_session_lock_manager_v1_interface, 1);
    } else if (strcmp(interface, wl_seat_interface.name) == 0) {
        app->seat = wl_registry_bind(registry, name, &wl_seat_interface, 5);
        wl_seat_add_listener(app->seat, &seat_listener, app);
    } else if (strcmp(interface, wl_output_interface.name) == 0) {
        LockOutput *output = calloc(1, sizeof(LockOutput));
        output->app = app;
        output->registry_name = name;
        output->wl_output = wl_registry_bind(registry, name, &wl_output_interface, 4);
        output->scale = 1;
        output->next = app->outputs;
        app->outputs = output;
        // a screen connected while locked needs its own lock surface
        setup_lock_surface(output);
    }
}

static void registry_global_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)registry;
    LockApp *app = (LockApp *)data;
    LockOutput **curr = &app->outputs;
    while (*curr) {
        if ((*curr)->registry_name == name) {
            LockOutput *to_remove = *curr;
            *curr = to_remove->next;
            /* a finger still down on this screen's keyboard: its release must not read the
             * freed screen (a monitor unplugged mid-touch crashed the lock screen) */
            if (app->osk_pressed_output == to_remove) {
                app->osk_pressed_output = NULL;
                app->osk_pressed = -1;
            }
            if (to_remove->frame_cb) wl_callback_destroy(to_remove->frame_cb);
            if (to_remove->lock_surface) ext_session_lock_surface_v1_destroy(to_remove->lock_surface);
            if (to_remove->surface) wl_surface_destroy(to_remove->surface);
            free_output_buffer(to_remove);
        if (to_remove->scene_bg) cairo_surface_destroy(to_remove->scene_bg);
            if (to_remove->wl_output) wl_output_destroy(to_remove->wl_output);
            free(to_remove);
            break;
        }
        curr = &(*curr)->next;
    }
}

static const struct wl_registry_listener registry_listener = {
    .global = registry_global,
    .global_remove = registry_global_remove,
};

/* ========================================================================== */
/* Main Loop & Lifecycle                                                      */
/* ========================================================================== */
/* The name the screen shows and up to two initials (UTF-8 aware): the account's full name (GECOS
 * up to the first comma), else the login. */
static void resolve_display_name(LockApp *app) {
    struct passwd *pw = getpwnam(app->username);
    const char *full = NULL;
    char gecos[128] = "";
    if (pw && pw->pw_gecos && pw->pw_gecos[0]) {
        snprintf(gecos, sizeof(gecos), "%s", pw->pw_gecos);
        char *comma = strchr(gecos, ',');
        if (comma) *comma = '\0';
        if (gecos[0]) full = gecos;
    }
    snprintf(app->display_name, sizeof(app->display_name), "%s", full ? full : app->username);

    /* the first character of the first word, and of the last word when there are several */
    const char *src = app->display_name;
    const char *words[2] = { NULL, NULL };
    for (const char *q = src; *q; q++) {
        if (*q != ' ' && (q == src || q[-1] == ' ')) {
            if (!words[0]) words[0] = q;
            else words[1] = q;
        }
    }
    size_t n = 0;
    for (int k = 0; k < 2; k++) {
        const char *c = words[k];
        if (!c) continue;
        if ((unsigned char)*c < 0x80) {
            char ch = *c;
            if (ch >= 'a' && ch <= 'z') ch -= 32;
            if (n + 1 < sizeof(app->initials)) app->initials[n++] = ch;
        } else {
            size_t len = 1;
            while (c[len] && ((unsigned char)c[len] & 0xC0) == 0x80 && len < 4) len++;
            if (n + len < sizeof(app->initials)) {
                memcpy(app->initials + n, c, len);
                n += len;
            }
        }
    }
    if (n == 0) app->initials[n++] = '?';
    app->initials[n] = '\0';
}

/* One picture from the cache the wallpaper helper writes (lock-cache.h), only when it is a plain
 * file of sane size and decodes to at most max_side pixels a side. */
static cairo_surface_t *load_cached_picture(const char *name, int max_side) {
    const char *xdg = getenv("XDG_CACHE_HOME");
    const char *home = getenv("HOME");
    char path[4096];
    if (xdg && xdg[0] == '/') snprintf(path, sizeof(path), "%s/infynitae/%s", xdg, name);
    else if (home && home[0]) snprintf(path, sizeof(path), "%s/.cache/infynitae/%s", home, name);
    else return NULL;
    struct stat st;
    if (stat(path, &st) != 0 || !S_ISREG(st.st_mode) || st.st_size <= 0 || st.st_size > 16 * 1024 * 1024) return NULL;
    cairo_surface_t *img = cairo_image_surface_create_from_png(path);
    if (!img || cairo_surface_status(img) != CAIRO_STATUS_SUCCESS ||
        cairo_image_surface_get_width(img) <= 0 || cairo_image_surface_get_height(img) <= 0 ||
        cairo_image_surface_get_width(img) > max_side || cairo_image_surface_get_height(img) > max_side) {
        if (img) cairo_surface_destroy(img);
        return NULL;
    }
    return img;
}

static void load_lock_pictures(LockApp *app) {
    app->background = load_cached_picture("lock-background.png", 4096);
    app->avatar = load_cached_picture("avatar.png", 1024);
}

/* --render WxH OUT.png: one frame of the lock screen into a PNG, for tests and screenshots. It
 * connects to nothing and locks nothing. */
static int render_to_png(LockApp *app, const char *size, const char *out) {
    int w = 0, h = 0;
    if (sscanf(size, "%dx%d", &w, &h) != 2 || w <= 0 || h <= 0 || w > 8192 || h > 8192) {
        fprintf(stderr, "[infynitae-lock] --render needs WxH, e.g. 1920x1080\n");
        return 2;
    }
    load_lock_pictures(app);
    const char *state = getenv("INFYNITAE_LOCK_RENDER_STATE");
    if (state && strcmp(state, "typing") == 0) app->password_len = 8;
    else if (state && strcmp(state, "failed") == 0) app->auth_state = AUTH_STATE_FAILED;
    else if (state && strcmp(state, "caps") == 0) app->caps_lock = true;
    else if (state && strcmp(state, "verifying") == 0) app->auth_state = AUTH_STATE_VERIFYING;
    cairo_surface_t *surf = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, w, h);
    cairo_t *cr = cairo_create(surf);
    draw_background(cr, app, w, h);
    draw_scene(cr, app, NULL, w, h);
    cairo_destroy(cr);
    cairo_status_t st = cairo_surface_write_to_png(surf, out);
    cairo_surface_destroy(surf);
    if (app->background) cairo_surface_destroy(app->background);
    if (app->avatar) cairo_surface_destroy(app->avatar);
    return st == CAIRO_STATUS_SUCCESS ? 0 : 1;
}

int main(int argc, char **argv) {
    LockApp app = {0};
    app.running = true;
    app.osk_pressed = -1;
    app.auth_state = AUTH_STATE_IDLE;
    strncpy(app.session_cmd, "infynitae-session", sizeof(app.session_cmd) - 1);
    load_theme_config(&app);
    const char *render_size = NULL, *render_out = NULL;
    (void)mlock(app.password, sizeof(app.password));

    // Parse options
    for (int i = 1; i < argc; ++i) {
        if (strcmp(argv[i], "--greeter") == 0) {
            app.greeter_mode = true;
        } else if (strcmp(argv[i], "--cmd") == 0 && i + 1 < argc) {
            strncpy(app.session_cmd, argv[++i], sizeof(app.session_cmd) - 1);
        } else if (strcmp(argv[i], "--user") == 0 && i + 1 < argc) {
            strncpy(app.username, argv[++i], sizeof(app.username) - 1);
        } else if (strcmp(argv[i], "--status") == 0 && i + 1 < argc) {
            strncpy(app.status_msg, argv[++i], sizeof(app.status_msg) - 1);
        } else if (strcmp(argv[i], "--render") == 0 && i + 2 < argc) {
            render_size = argv[++i];
            render_out = argv[++i];
        }
    }

    // Resolve username. Locking, it is always the real account (getuid), never the environment
    // or an argument: only that account's password unlocks the session. `--user` picks the
    // account only for the greeter, which logs someone in.
    // the greeter is not finished: it starts the session without dropping to the account (no
    // setuid, no PAM session), so as root that session would be root's. Refused there.
    if (app.greeter_mode && geteuid() == 0) {
        fprintf(stderr, "[infynitae-lock] --greeter does not run as root (it would start the session as root)\n");
        return 1;
    }
    if (!app.greeter_mode) {
        if (app.username[0] != '\0')
            fprintf(stderr, "[infynitae-lock] --user is only for --greeter; locking for the real account\n");
        struct passwd *pw = getpwuid(getuid());
        if (!pw || !pw->pw_name || !pw->pw_name[0]) {
            fprintf(stderr, "[infynitae-lock] Error: cannot find the account of uid %u\n", (unsigned)getuid());
            return 1;
        }
        memset(app.username, 0, sizeof(app.username));
        strncpy(app.username, pw->pw_name, sizeof(app.username) - 1);
    } else if (app.username[0] == '\0') {
        const char *login_user = getenv("USER");
        if (!login_user) login_user = getenv("LOGNAME");
        if (!login_user) {
            struct passwd *pw = getpwuid(getuid());
            if (pw) login_user = pw->pw_name;
        }
        strncpy(app.username, login_user ? login_user : "user", sizeof(app.username) - 1);
    }

    resolve_display_name(&app);
    if (render_out) return render_to_png(&app, render_size, render_out);

    app.xkb_ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    if (!app.xkb_ctx) {
        fprintf(stderr, "[infynitae-lock] Error: Failed to create XKB context\n");
        return 1;
    }

    app.display = wl_display_connect(NULL);
    if (!app.display) {
        fprintf(stderr, "[infynitae-lock] Error: Cannot connect to Wayland display\n");
        xkb_context_unref(app.xkb_ctx);
        return 1;
    }

    app.registry = wl_display_get_registry(app.display);
    wl_registry_add_listener(app.registry, &registry_listener, &app);
    wl_display_roundtrip(app.display);
    // no core dump and no ptrace by the same user of a process that holds the password. Only now: a
    // non-dumpable process's /proc/<pid>/exe cannot be read, and the compositor knows the lock screen by that
    // file when it connects (an earlier call made it refuse the session lock; owner's PC, 2026-11-11). No
    // password has been typed yet.
    prctl(PR_SET_DUMPABLE, 0);

    if (!app.compositor || !app.shm || !app.lock_manager) {
        fprintf(stderr, "[infynitae-lock] Error: Wayland compositor missing required protocols (shm, ext-session-lock-v1)\n");
        return 1;
    }

    // Lock the session, then give every output its lock surface
    app.lock = ext_session_lock_manager_v1_lock(app.lock_manager);
    ext_session_lock_v1_add_listener(app.lock, &lock_listener, &app);
    for (LockOutput *o = app.outputs; o != NULL; o = o->next) {
        setup_lock_surface(o);
    }
    wl_display_roundtrip(app.display);
    wl_display_roundtrip(app.display);
    // the compositor confirms the lock once a locked frame is on screen (after a vblank)
    while (!app.locked && !app.lock_refused) {
        if (wl_display_dispatch(app.display) < 0) {
            fprintf(stderr, "[infynitae-lock] Error: lost the compositor connection\n");
            return 1;
        }
    }
    if (app.lock_refused) {
        fprintf(stderr, "[infynitae-lock] Error: the compositor refused the lock (is another lock screen running?)\n");
        return 1;
    }

    // session.sh waits for this line (START_LOCKED): printed only once the lock is confirmed
    printf("[infynitae-lock] Screen locked for user '%s'. Enter password to unlock.\n", app.username);
    fflush(stdout);

    // only now the pictures: the lock already holds with the plain background, so a missing,
    // broken or slow file can never stand between the request and the lock
    load_lock_pictures(&app);
    damage_all_outputs(&app);

    // Event poll loop
    time_t last_clock_tick = time(NULL);

    while (app.running) {
        // a lost connection: exit (the compositor keeps the session locked without its client)
        bool lost = false;
        while (wl_display_prepare_read(app.display) != 0) {
            if (wl_display_dispatch_pending(app.display) < 0) {
                lost = true;
                break;
            }
        }
        if (lost || wl_display_get_error(app.display) != 0) {
            fprintf(stderr, "[infynitae-lock] Error: lost the compositor connection\n");
            return 1;
        }
        wl_display_flush(app.display);

        struct pollfd pfd = {
            .fd = wl_display_get_fd(app.display),
            .events = POLLIN,
            .revents = 0
        };

        // Poll every 250ms for clock update and state transition
        int poll_timeout = (app.shake_frame > 0 || app.auth_state == AUTH_STATE_VERIFYING || app.status_msg[0] != '\0') ? 16 : 250;
        int ret = poll(&pfd, 1, poll_timeout);

        if (ret > 0 && (pfd.revents & POLLIN)) {
            if (wl_display_read_events(app.display) < 0 ||
                wl_display_dispatch_pending(app.display) < 0) {
                fprintf(stderr, "[infynitae-lock] Error: lost the compositor connection\n");
                return 1;
            }
        } else {
            wl_display_cancel_read(app.display);
            if (ret > 0 && (pfd.revents & (POLLHUP | POLLERR))) {
                fprintf(stderr, "[infynitae-lock] Error: lost the compositor connection\n");
                return 1;
            }
        }

        // Check auth status
        if (app.auth_state == AUTH_STATE_SUCCESS) {
            damage_all_outputs(&app);
            wl_display_roundtrip(app.display);
            app.running = false;
            break;
        }
        if (app.lock_refused) {
            fprintf(stderr, "[infynitae-lock] Error: the compositor ended the lock\n");
            return 1;
        }

        // Update clock every minute
        time_t cur = time(NULL);
        if (cur / 60 != last_clock_tick / 60) {
            last_clock_tick = cur;
            damage_all_outputs(&app);
        }
    }

    // Unlock (only after authentication), then destroy the lock surfaces
    if (app.lock && app.auth_state == AUTH_STATE_SUCCESS) {
        ext_session_lock_v1_unlock_and_destroy(app.lock);
        app.lock = NULL;
        wl_display_roundtrip(app.display);
    }
    LockOutput *curr = app.outputs;
    while (curr) {
        LockOutput *next = curr->next;
        if (curr->frame_cb) wl_callback_destroy(curr->frame_cb);
        if (curr->lock_surface) ext_session_lock_surface_v1_destroy(curr->lock_surface);
        if (curr->surface) wl_surface_destroy(curr->surface);
        free_output_buffer(curr);
        if (curr->scene_bg) cairo_surface_destroy(curr->scene_bg);
        if (curr->wl_output) wl_output_destroy(curr->wl_output);
        free(curr);
        curr = next;
    }

    if (app.keyboard) wl_keyboard_destroy(app.keyboard);
    if (app.seat) wl_seat_destroy(app.seat);
    if (app.lock_manager) ext_session_lock_manager_v1_destroy(app.lock_manager);
    if (app.shm) wl_shm_destroy(app.shm);
    if (app.compositor) wl_compositor_destroy(app.compositor);
    if (app.registry) wl_registry_destroy(app.registry);
    wl_display_flush(app.display);
    wl_display_disconnect(app.display);

    if (app.xkb_state) xkb_state_unref(app.xkb_state);
    if (app.xkb_keymap) xkb_keymap_unref(app.xkb_keymap);
    if (app.xkb_ctx) xkb_context_unref(app.xkb_ctx);

    explicit_bzero(app.password, sizeof(app.password));

    if (app.greeter_mode && app.auth_state == AUTH_STATE_SUCCESS) {
        printf("[infynitae-lock] Launching session: %s\n", app.session_cmd);
        execlp(app.session_cmd, app.session_cmd, NULL);
    }

    printf("[infynitae-lock] Unlocked.\n");
    return 0;
}
