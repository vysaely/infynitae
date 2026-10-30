/*
 * infynitae-lock - Native Minimalist Wayland Screen Locker & Start Screen Greeter
 * 
 * Part of the Infynitae Compositor ecosystem.
 * Philosophy: Zero bloat, instant startup, deep obsidian canvas aesthetics,
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
#include <pwd.h>
#include <math.h>
#include <poll.h>

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
    memset(data->password, 0, sizeof(data->password));

    if (rc == PAM_SUCCESS) {
        app->auth_state = AUTH_STATE_SUCCESS;
    } else {
        app->auth_state = AUTH_STATE_FAILED;
        app->shake_frame = SHAKE_DURATION_FRAMES;
    }

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

    strncpy(data->username, app->username, sizeof(data->username) - 1);
    data->username[sizeof(data->username) - 1] = '\0';
    strncpy(data->password, app->password, sizeof(data->password) - 1);
    data->password[sizeof(data->password) - 1] = '\0';
    data->app = app;

    // Clear local buffer immediately
    memset(app->password, 0, sizeof(app->password));
    app->password_len = 0;

    app->pam_thread_active = true;
    if (pthread_create(&app->pam_thread, NULL, pam_worker_thread, data) != 0) {
        app->auth_state = AUTH_STATE_FAILED;
        app->pam_thread_active = false;
        free(data);
    }
}

/* ========================================================================== */
/* UI Rendering (Cairo & Pango)                                               */
/* ========================================================================== */
static void draw_rounded_rect(cairo_t *cr, double x, double y, double w, double h, double r) {
    cairo_new_sub_path(cr);
    cairo_arc(cr, x + w - r, y + r, r, -M_PI / 2, 0);
    cairo_arc(cr, x + w - r, y + h - r, r, 0, M_PI / 2);
    cairo_arc(cr, x + r, y + h - r, r, M_PI / 2, M_PI);
    cairo_arc(cr, x + r, y + r, r, M_PI, 3 * M_PI / 2);
    cairo_close_path(cr);
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

    cairo_move_to(cr, x, y);
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
    cairo_set_source_rgba(cr, 0.04, 0.06, 0.08, 0.92);
    cairo_rectangle(cr, 0, top, w, h - top);
    cairo_fill(cr);
    PangoFontDescription *desc = pango_font_description_from_string("Sans 15");
    pango_layout_set_font_description(layout, desc);
    pango_font_description_free(desc);
    for (int i = 0; i < n; i++) {
        bool pressed = app->osk_pressed == i && app->osk_pressed_output == output;
        bool lit = keys[i].kind == OSK_SHIFT && app->osk_shift;
        if (pressed)
            cairo_set_source_rgba(cr, 0.30, 0.55, 0.90, 0.85);
        else if (keys[i].kind == OSK_CHAR || keys[i].kind == OSK_SPACE)
            cairo_set_source_rgba(cr, 0.14, 0.18, 0.24, 0.95);
        else
            cairo_set_source_rgba(cr, lit ? 0.22 : 0.10, lit ? 0.32 : 0.13, lit ? 0.48 : 0.18, 0.95);
        draw_rounded_rect(cr, keys[i].x, keys[i].y, keys[i].w, keys[i].h, 8.0);
        cairo_fill(cr);
        char label[8];
        osk_label(app, &keys[i], label);
        cairo_set_source_rgba(cr, 0.90, 0.95, 1.0, 0.92);
        draw_text_centered(cr, layout, "Sans 15", label,
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

static void render_output(LockOutput *output) {
    if (!output->configured || output->width <= 0 || output->height <= 0) return;
    if (!allocate_output_buffer(output)) return;

    LockApp *app = output->app;
    cairo_surface_t *surf = cairo_image_surface_create_for_data(
        (unsigned char *)output->shm_data, CAIRO_FORMAT_ARGB32,
        output->width, output->height, output->width * 4);
    cairo_t *cr = cairo_create(surf);

    int w = output->width;
    int h = output->height;

    // 1. Background: Deep obsidian infinite canvas (#0b0f14) with subtle radial vignette
    cairo_pattern_t *pat = cairo_pattern_create_radial(w / 2.0, h / 2.0, 50, w / 2.0, h / 2.0, w / 1.2);
    cairo_pattern_add_color_stop_rgb(pat, 0.0, 0.063, 0.082, 0.106); // #10151b
    cairo_pattern_add_color_stop_rgb(pat, 1.0, 0.043, 0.059, 0.078); // #0b0f14
    cairo_set_source(cr, pat);
    cairo_paint(cr);
    cairo_pattern_destroy(pat);

    // Subtle background grid lattice matching Infynitae
    cairo_set_source_rgba(cr, 1.0, 1.0, 1.0, 0.015);
    cairo_set_line_width(cr, 1.0);
    for (int x = 0; x < w; x += 80) {
        cairo_move_to(cr, x, 0);
        cairo_line_to(cr, x, h);
    }
    for (int y = 0; y < h; y += 80) {
        cairo_move_to(cr, 0, y);
        cairo_line_to(cr, w, y);
    }
    cairo_stroke(cr);

    // Calculate center layout with shake offset
    double shake_offset = 0;
    if (app->shake_frame > 0) {
        double decay = (double)app->shake_frame / SHAKE_DURATION_FRAMES;
        shake_offset = sin(app->shake_frame * 1.5) * 16.0 * decay;
    }

    double cx = (w / 2.0) + shake_offset;
    double cy = h / 2.0;
    if (app->osk_visible) {
        /* the password stays above the keyboard */
        OskKey keys[OSK_MAX_KEYS];
        double kb_top;
        osk_layout(app, w, h, keys, &kb_top);
        if (cy + 140 > kb_top) cy = kb_top - 140;
    }

    // Pango layout initialization
    PangoLayout *layout = pango_cairo_create_layout(cr);

    // 2. Clock & Date (ink-centered so digit bearings never shift the clock off-axis)
    time_t now = time(NULL);
    struct tm *tm = localtime(&now);
    char time_str[64];
    char date_str[128];
    strftime(time_str, sizeof(time_str), "%H:%M", tm);
    strftime(date_str, sizeof(date_str), "%A, %B %d", tm);

    // Tabular numbers attribute so all digits have identical width and ':' stays centered
    PangoAttrList *clock_attrs = pango_attr_list_new();
    pango_attr_list_insert(clock_attrs, pango_attr_font_features_new("tnum=1"));
    pango_layout_set_attributes(layout, clock_attrs);
    cairo_set_source_rgba(cr, 0.95, 0.97, 1.0, 0.94);
    draw_text_centered(cr, layout, "Sans Light 76", time_str, cx, cy - 185.0, true);
    pango_layout_set_attributes(layout, NULL);
    pango_attr_list_unref(clock_attrs);

    // Date beneath clock (clean 32px separation below the clock's bottom edge)
    cairo_set_source_rgba(cr, 0.62, 0.68, 0.78, 0.78);
    draw_text_centered(cr, layout, "Sans 14", date_str, cx, cy - 110.0, false);

    // 3. User Avatar / Initial Badge (or Loading Spinner in status/verifying mode)
    double avatar_radius = 34.0;
    double avatar_y = cy - 28.0;

    // Avatar background circle
    cairo_set_source_rgba(cr, 0.10, 0.14, 0.20, 0.90);
    cairo_arc(cr, cx, avatar_y, avatar_radius, 0, 2 * M_PI);
    cairo_fill(cr);

    // Avatar ring + animated spinner arc when verifying, unlocking, or showing status_msg
    bool busy = (app->auth_state == AUTH_STATE_VERIFYING ||
                 app->auth_state == AUTH_STATE_SUCCESS ||
                 app->status_msg[0] != '\0');
    if (busy) {
        cairo_set_source_rgba(cr, 0.25, 0.35, 0.50, 0.35);
        cairo_set_line_width(cr, 2.2);
        cairo_arc(cr, cx, avatar_y, avatar_radius, 0, 2 * M_PI);
        cairo_stroke(cr);

        cairo_set_source_rgba(cr, 0.48, 0.64, 0.97, 0.95);
        cairo_set_line_width(cr, 2.5);
        cairo_set_line_cap(cr, CAIRO_LINE_CAP_ROUND);
        cairo_arc(cr, cx, avatar_y, avatar_radius, app->pulse_phase, app->pulse_phase + 1.8);
        cairo_stroke(cr);
    } else if (app->auth_state == AUTH_STATE_FAILED) {
        cairo_set_source_rgba(cr, 0.97, 0.32, 0.29, 0.90);
        cairo_set_line_width(cr, 2.0);
        cairo_arc(cr, cx, avatar_y, avatar_radius, 0, 2 * M_PI);
        cairo_stroke(cr);
    } else {
        cairo_set_source_rgba(cr, 0.25, 0.32, 0.45, 0.55);
        cairo_set_line_width(cr, 1.5);
        cairo_arc(cr, cx, avatar_y, avatar_radius, 0, 2 * M_PI);
        cairo_stroke(cr);
    }

    // Initial letter in avatar (ink-centered so capital letters sit dead-center in the circle)
    char initial[2] = { (char)(app->username[0] >= 'a' && app->username[0] <= 'z' ? app->username[0] - 32 : app->username[0]), '\0' };
    if (!initial[0]) initial[0] = 'I';
    cairo_set_source_rgba(cr, 0.92, 0.96, 1.0, 0.94);
    draw_text_centered(cr, layout, "Sans Bold 20", initial, cx, avatar_y, true);

    // Username label (centered 22px below the avatar circle)
    cairo_set_source_rgba(cr, 0.82, 0.87, 0.96, 0.88);
    draw_text_centered(cr, layout, "Sans Medium 13", app->username, cx, avatar_y + avatar_radius + 20.0, false);

    // 4. Password Input / Status Pill
    double pill_w = 280.0;
    double pill_h = 46.0;
    double pill_x = cx - (pill_w / 2.0);
    double pill_y = cy + 60.0;
    double pill_cy = pill_y + (pill_h / 2.0);

    // Pill background
    cairo_set_source_rgba(cr, 0.08, 0.11, 0.15, 0.92);
    draw_rounded_rect(cr, pill_x, pill_y, pill_w, pill_h, pill_h / 2.0);
    cairo_fill(cr);

    // Pill Border
    if (app->auth_state == AUTH_STATE_FAILED) {
        cairo_set_source_rgba(cr, 0.97, 0.32, 0.29, 0.90);
        cairo_set_line_width(cr, 1.8);
    } else if (busy) {
        cairo_set_source_rgba(cr, 0.48, 0.64, 0.97, 0.85);
        cairo_set_line_width(cr, 1.6);
    } else if (app->password_len > 0) {
        cairo_set_source_rgba(cr, 0.48, 0.64, 0.97, 0.80);
        cairo_set_line_width(cr, 1.5);
    } else {
        cairo_set_source_rgba(cr, 0.20, 0.26, 0.35, 0.60);
        cairo_set_line_width(cr, 1.2);
    }
    draw_rounded_rect(cr, pill_x, pill_y, pill_w, pill_h, pill_h / 2.0);
    cairo_stroke(cr);

    // Password indicator contents inside pill
    if (app->status_msg[0] != '\0') {
        cairo_set_source_rgba(cr, 0.78, 0.86, 0.98, 0.92);
        draw_text_centered(cr, layout, "Sans Medium 11", app->status_msg, cx, pill_cy, false);
    } else if (app->auth_state == AUTH_STATE_SUCCESS) {
        cairo_set_source_rgba(cr, 0.65, 0.88, 0.75, 0.92);
        draw_text_centered(cr, layout, "Sans Medium 11", "Starting session…", cx, pill_cy, false);
    } else if (app->auth_state == AUTH_STATE_VERIFYING) {
        cairo_set_source_rgba(cr, 0.65, 0.80, 0.98, 0.90);
        draw_text_centered(cr, layout, "Sans Medium 11", "Verifying…", cx, pill_cy, false);
    } else if (app->password_len > 0) {
        double dot_spacing = 14.0;
        size_t max_dots = 16;
        size_t visible_dots = app->password_len < max_dots ? app->password_len : max_dots;
        double total_dots_w = visible_dots > 0 ? (visible_dots - 1) * dot_spacing : 0;
        double start_dot_x = cx - (total_dots_w / 2.0);

        cairo_set_source_rgba(cr, 0.88, 0.93, 1.0, 0.92);
        for (size_t i = 0; i < visible_dots; ++i) {
            cairo_arc(cr, start_dot_x + (i * dot_spacing), pill_cy, 3.5, 0, 2 * M_PI);
            cairo_fill(cr);
        }
    } else {
        cairo_set_source_rgba(cr, 0.44, 0.50, 0.62, 0.65);
        draw_text_centered(cr, layout, "Sans 11", "Enter password", cx, pill_cy, false);
    }

    // 5. Status / Caps Lock / Error Badge below pill
    double badge_cy = pill_y + pill_h + 26.0;
    if (app->auth_state == AUTH_STATE_FAILED && app->password_len == 0) {
        cairo_set_source_rgba(cr, 0.97, 0.42, 0.39, 0.92);
        draw_text_centered(cr, layout, "Sans Medium 10", "Authentication failed · try again", cx, badge_cy, false);
    } else if (app->caps_lock) {
        double badge_w = 124.0;
        double badge_h = 26.0;
        double badge_x = cx - (badge_w / 2.0);
        double badge_y = badge_cy - (badge_h / 2.0);

        cairo_set_source_rgba(cr, 0.95, 0.60, 0.15, 0.16);
        draw_rounded_rect(cr, badge_x, badge_y, badge_w, badge_h, 13.0);
        cairo_fill(cr);

        cairo_set_source_rgba(cr, 0.95, 0.65, 0.20, 0.55);
        cairo_set_line_width(cr, 1.0);
        draw_rounded_rect(cr, badge_x, badge_y, badge_w, badge_h, 13.0);
        cairo_stroke(cr);

        cairo_set_source_rgba(cr, 1.0, 0.78, 0.32, 0.95);
        draw_text_centered(cr, layout, "Sans Bold 9", "CAPS LOCK ON", cx, badge_cy, true);
    }

    if (app->osk_visible) osk_draw(cr, layout, app, output, w, h);

    // 6. Bottom Brand Hint (under the keyboard when it is shown)
    if (!app->osk_visible) {
        cairo_set_source_rgba(cr, 0.42, 0.48, 0.60, 0.42);
        draw_text_centered(cr, layout, "Sans Light 10", "Infynitae Native", w / 2.0, h - 36.0, false);
    }

    g_object_unref(layout);
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

static void lock_locked(void *data, struct ext_session_lock_v1 *lock) {
    (void)lock;
    ((LockApp *)data)->locked = true;
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
        memset(app->password, 0, sizeof(app->password));
        app->password_len = 0;
        app->auth_state = AUTH_STATE_IDLE;
        damage_all_outputs(app);
        return;
    }

    // Ctrl+U clears line
    if ((sym == XKB_KEY_u || sym == XKB_KEY_U) &&
        xkb_state_mod_name_is_active(app->xkb_state, XKB_MOD_NAME_CTRL, XKB_STATE_MODS_DEPRESSED)) {
        memset(app->password, 0, sizeof(app->password));
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
            if (to_remove->frame_cb) wl_callback_destroy(to_remove->frame_cb);
            if (to_remove->lock_surface) ext_session_lock_surface_v1_destroy(to_remove->lock_surface);
            if (to_remove->surface) wl_surface_destroy(to_remove->surface);
            free_output_buffer(to_remove);
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
int main(int argc, char **argv) {
    LockApp app = {0};
    app.running = true;
    app.osk_pressed = -1;
    app.auth_state = AUTH_STATE_IDLE;
    strncpy(app.session_cmd, "infynitae-session", sizeof(app.session_cmd) - 1);

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
        }
    }

    // Resolve username
    if (app.username[0] == '\0') {
        const char *login_user = getenv("USER");
        if (!login_user) login_user = getenv("LOGNAME");
        if (!login_user) {
            struct passwd *pw = getpwuid(getuid());
            if (pw) login_user = pw->pw_name;
        }
        if (login_user) {
            strncpy(app.username, login_user, sizeof(app.username) - 1);
        } else {
            strncpy(app.username, "user", sizeof(app.username) - 1);
        }
    }

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

    memset(app.password, 0, sizeof(app.password));

    if (app.greeter_mode && app.auth_state == AUTH_STATE_SUCCESS) {
        printf("[infynitae-lock] Launching session: %s\n", app.session_cmd);
        execlp(app.session_cmd, app.session_cmd, NULL);
    }

    printf("[infynitae-lock] Unlocked successfully.\n");
    return 0;
}
