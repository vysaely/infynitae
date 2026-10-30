/*
 * Infynitae Native Dynamic Island & Top-Edge Hybrid Shell (infynitae-island)
 *
 * Combines:
 *   1. Apple iOS/iPadOS split top-edge touchscreen gestures + rubber-band slide-down physics:
 *      - Swipe down from Top-Center (or tap the subtle top pill): Dynamic Island (Search + App
 *        Launcher + Open Window Tabs on the Infinite Canvas)
 *      - Swipe down from Top-Right corner: Quick Control Box (Clock, Battery, Wi-Fi, Bluetooth,
 *        DND, Tray Apps, Power Profile, Touch Volume Slider, Lock / Reboot / Power Off)
 *      - Keyboard shortcut (Super+W / `infynitae-island toggle`): Opens the Hybrid view (both
 *        Top-Center Island + Top-Right Control Box together)
 *   2. 100% out of your way when closed:
 *      - Only a 10px top-edge touch/click strip accepts input when collapsed; the rest of the
 *        screen passes all mouse/touch events straight through to windows.
 */

#define _GNU_SOURCE
#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

#include <cairo/cairo.h>
#include <pango/pangocairo.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>

#include "../wallpaper/wlr-layer-shell-protocol.h"

#define MAX_WINDOWS 24
#define MAX_APPS 128
#define VISIBLE_APPS 8

typedef enum {
    MODE_CLOSED = 0,
    MODE_CENTER = 1,  /* Top-Center Island only (swipe down from center) */
    MODE_CONTROL = 2, /* Top-Right Control Box only (swipe down from top-right) */
    MODE_HYBRID = 3   /* Both Center Island + Right Control Box (Super+W) */
} IslandMode;

typedef struct {
    uint64_t inst;
    uint64_t elem;
    char app_id[64];
    char title[128];
    bool focused;
} CanvasWindow;

typedef struct {
    char name[64];
    char exec_cmd[256];
} DesktopApp;

typedef struct {
    double x, y, w, h;
    int action_id;
    int index;
} HitBox;

#define MAX_HITBOXES 64

struct IslandApp;

typedef struct IslandOutput {
    struct IslandApp *app;
    struct wl_output *wl_output;
    uint32_t registry_name;
    int32_t width;
    int32_t height;
    int32_t scale;
    bool configured;

    struct wl_surface *surface;
    struct zwlr_layer_surface_v1 *layer_surface;
    struct wl_callback *frame_cb;

    struct wl_buffer *buffer;
    void *shm_data;
    size_t shm_size;
    int32_t buf_w;
    int32_t buf_h;

    struct IslandOutput *next;
} IslandOutput;

typedef struct IslandApp {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct zwlr_layer_shell_v1 *layer_shell;
    struct wl_seat *seat;
    struct wl_pointer *pointer;
    struct wl_keyboard *keyboard;
    struct wl_touch *touch;

    struct xkb_context *xkb_ctx;
    struct xkb_keymap *xkb_keymap;
    struct xkb_state *xkb_state;

    IslandOutput *outputs;
    IslandOutput *active_output;

    IslandMode mode;
    IslandMode target_mode;
    double anim_progress; /* 0.0 = collapsed, 1.0 = fully open */
    bool dragging;
    int32_t touch_id;
    double touch_start_x;
    double touch_start_y;
    double touch_cur_x;
    double touch_cur_y;

    double pointer_x;
    double pointer_y;
    IslandOutput *pointer_output;

    /* Search & selection state */
    char search[128];
    size_t search_len;
    int selected_idx; /* 0..filtered_win_count-1, then apps */

    /* Snapshot windows */
    CanvasWindow windows[MAX_WINDOWS];
    int win_count;
    int filtered_wins[MAX_WINDOWS];
    int filtered_win_count;

    /* Installed .desktop apps */
    DesktopApp apps[MAX_APPS];
    int app_count;
    int filtered_apps[MAX_APPS];
    int filtered_app_count;

    /* Control Center cached status */
    bool bottom_edge;     /* false = Top edge, true = Bottom edge */
    bool control_on_left; /* true = Left corner, false = Right corner */
    bool wifi_on;
    char wifi_label[48];
    bool bt_on;
    bool dnd_on;
    int volume_pct;
    bool volume_muted;
    int tray_count;
    char power_profile[32];
    int battery_pct;
    bool battery_charging;
    time_t last_status_refresh;

    /* Interactive hitboxes for the most recent render */
    HitBox hitboxes[MAX_HITBOXES];
    int hitbox_count;

    bool running;
} IslandApp;

enum {
    ACT_NONE = 0,
    ACT_SEARCH_CLEAR,
    ACT_OVERVIEW,
    ACT_TOGGLE_HYBRID,
    ACT_WIN_FOCUS,
    ACT_WIN_CLOSE,
    ACT_APP_LAUNCH,
    ACT_WIFI_TOGGLE,
    ACT_BT_TOGGLE,
    ACT_DND_TOGGLE,
    ACT_TRAY_MENU,
    ACT_POWER_PROFILE,
    ACT_VOL_DOWN,
    ACT_VOL_UP,
    ACT_VOL_MUTE,
    ACT_VOL_BAR,
    ACT_LOCK,
    ACT_REBOOT,
    ACT_POWEROFF,
    ACT_LOGOUT,
    ACT_PILL_TAP,
    ACT_SWAP_SIDE,
    ACT_SWAP_EDGE
};

static volatile sig_atomic_t g_sig_cmd = 0; /* 1 = toggle hybrid, 2 = toggle control, 3 = quit */

static void on_sigusr1(int sig) { (void)sig; g_sig_cmd = 1; }
static void on_sigusr2(int sig) { (void)sig; g_sig_cmd = 2; }
static void on_sigterm(int sig) { (void)sig; g_sig_cmd = 3; }

static void request_frame(IslandOutput *out);
static void damage_all(IslandApp *app);
static void set_mode(IslandApp *app, IslandMode new_mode);

/* ========================================================================== */
/* PID & Command Helper                                                       */
/* ========================================================================== */
static const char *pid_file_path(void) {
    static char path[512];
    const char *rt = getenv("XDG_RUNTIME_DIR");
    if (!rt) rt = "/tmp";
    snprintf(path, sizeof(path), "%s/infynitae-island.pid", rt);
    return path;
}

static void spawn_detached(const char *cmd) {
    if (!cmd || !*cmd) return;
    pid_t pid = fork();
    if (pid == 0) {
        setsid();
        int devnull = open("/dev/null", O_RDWR);
        if (devnull >= 0) {
            dup2(devnull, STDIN_FILENO);
            dup2(devnull, STDOUT_FILENO);
            dup2(devnull, STDERR_FILENO);
            if (devnull > 2) close(devnull);
        }
        execl("/bin/sh", "sh", "-c", cmd, (char *)NULL);
        _exit(127);
    }
}

static bool read_cmd_line(const char *cmd, char *out, size_t out_sz) {
    out[0] = '\0';
    FILE *fp = popen(cmd, "r");
    if (!fp) return false;
    if (fgets(out, (int)out_sz, fp)) {
        size_t len = strlen(out);
        while (len > 0 && (out[len - 1] == '\n' || out[len - 1] == '\r'))
            out[--len] = '\0';
    }
    pclose(fp);
    return out[0] != '\0';
}

/* ========================================================================== */
/* Data Collectors: Desktop Apps, Canvas Snapshot, System Status              */
/* ========================================================================== */
static void load_desktop_dir(IslandApp *app, const char *dir_path) {
    DIR *d = opendir(dir_path);
    if (!d) return;
    struct dirent *de;
    while ((de = readdir(d)) != NULL && app->app_count < MAX_APPS) {
        size_t nlen = strlen(de->d_name);
        if (nlen < 9 || strcmp(de->d_name + nlen - 8, ".desktop") != 0) continue;

        char path[512];
        snprintf(path, sizeof(path), "%s/%s", dir_path, de->d_name);
        FILE *f = fopen(path, "r");
        if (!f) continue;

        char line[512];
        char name[64] = {0};
        char exec_cmd[256] = {0};
        bool in_entry = false;
        bool no_display = false;

        while (fgets(line, sizeof(line), f)) {
            if (line[0] == '[') {
                in_entry = (strncmp(line, "[Desktop Entry]", 15) == 0);
                continue;
            }
            if (!in_entry) continue;
            if (strncmp(line, "NoDisplay=true", 14) == 0 || strncmp(line, "Hidden=true", 11) == 0) {
                no_display = true;
                break;
            }
            if (name[0] == '\0' && strncmp(line, "Name=", 5) == 0) {
                snprintf(name, sizeof(name), "%.*s", (int)(sizeof(name) - 1), line + 5);
                char *nl = strpbrk(name, "\r\n");
                if (nl) *nl = '\0';
            } else if (exec_cmd[0] == '\0' && strncmp(line, "Exec=", 5) == 0) {
                snprintf(exec_cmd, sizeof(exec_cmd), "%.*s", (int)(sizeof(exec_cmd) - 1), line + 5);
                char *nl = strpbrk(exec_cmd, "\r\n");
                if (nl) *nl = '\0';
                /* Strip %u, %F, %U, etc. field codes */
                char *pct;
                while ((pct = strstr(exec_cmd, " %")) != NULL) {
                    *pct = '\0';
                }
            }
        }
        fclose(f);

        if (no_display || name[0] == '\0' || exec_cmd[0] == '\0') continue;

        /* Deduplicate by name */
        bool dup = false;
        for (int i = 0; i < app->app_count; i++) {
            if (strcasecmp(app->apps[i].name, name) == 0) {
                dup = true;
                break;
            }
        }
        if (!dup) {
            snprintf(app->apps[app->app_count].name, sizeof(app->apps[0].name), "%s", name);
            snprintf(app->apps[app->app_count].exec_cmd, sizeof(app->apps[0].exec_cmd), "%s", exec_cmd);
            app->app_count++;
        }
    }
    closedir(d);
}

static void load_all_apps(IslandApp *app) {
    app->app_count = 0;
    const char *home = getenv("HOME");
    if (home) {
        char local_dir[512];
        snprintf(local_dir, sizeof(local_dir), "%s/.local/share/applications", home);
        load_desktop_dir(app, local_dir);
    }
    load_desktop_dir(app, "/usr/local/share/applications");
    load_desktop_dir(app, "/usr/share/applications");
}

static bool str_contains_ci(const char *hay, const char *needle) {
    if (!needle || !*needle) return true;
    if (!hay || !*hay) return false;
    size_t nlen = strlen(needle);
    for (const char *p = hay; *p; p++) {
        if (strncasecmp(p, needle, nlen) == 0) return true;
    }
    return false;
}

static void update_filters(IslandApp *app) {
    app->filtered_win_count = 0;
    for (int i = 0; i < app->win_count; i++) {
        if (str_contains_ci(app->windows[i].app_id, app->search) ||
            str_contains_ci(app->windows[i].title, app->search)) {
            app->filtered_wins[app->filtered_win_count++] = i;
        }
    }
    app->filtered_app_count = 0;
    for (int i = 0; i < app->app_count; i++) {
        if (str_contains_ci(app->apps[i].name, app->search) ||
            str_contains_ci(app->apps[i].exec_cmd, app->search)) {
            app->filtered_apps[app->filtered_app_count++] = i;
        }
    }
    int total = app->filtered_win_count + (app->filtered_app_count < VISIBLE_APPS ? app->filtered_app_count : VISIBLE_APPS);
    if (total <= 0) app->selected_idx = 0;
    else if (app->selected_idx >= total) app->selected_idx = total - 1;
}

static void refresh_snapshot(IslandApp *app) {
    app->win_count = 0;
    FILE *fp = popen("infynitae msg snapshot 2>/dev/null", "r");
    if (!fp) {
        update_filters(app);
        return;
    }
    static char buf[65536];
    size_t n = fread(buf, 1, sizeof(buf) - 1, fp);
    pclose(fp);
    buf[n] = '\0';

    const char *elems = strstr(buf, "\"elements\":[");
    if (!elems) {
        update_filters(app);
        return;
    }

    const char *p = elems + 12;
    while (*p && app->win_count < MAX_WINDOWS) {
        const char *id_pos = strstr(p, "\"id\":[");
        if (!id_pos) break;
        const char *next_obj = strstr(id_pos + 6, "\"id\":[");
        uint64_t inst = 0, elem = 0;
        if (sscanf(id_pos + 6, "%lu,%lu", &inst, &elem) == 2) {
            /* Check mapped state */
            const char *state_pos = strstr(id_pos, "\"state\":\"");
            if (state_pos && (!next_obj || state_pos < next_obj) &&
                strncmp(state_pos + 9, "mapped", 6) == 0) {
                CanvasWindow *w = &app->windows[app->win_count];
                memset(w, 0, sizeof(*w));
                w->inst = inst;
                w->elem = elem;
                w->focused = (app->win_count == 0);

                const char *app_pos = strstr(id_pos, "\"app_id\":\"");
                if (app_pos && (!next_obj || app_pos < next_obj)) {
                    app_pos += 10;
                    size_t i = 0;
                    while (*app_pos && *app_pos != '"' && i < sizeof(w->app_id) - 1) {
                        if (*app_pos == '\\' && *(app_pos + 1)) app_pos++;
                        w->app_id[i++] = *app_pos++;
                    }
                    w->app_id[i] = '\0';
                }
                const char *title_pos = strstr(id_pos, "\"title\":\"");
                if (title_pos && (!next_obj || title_pos < next_obj)) {
                    title_pos += 9;
                    size_t i = 0;
                    while (*title_pos && *title_pos != '"' && i < sizeof(w->title) - 1) {
                        if (*title_pos == '\\' && *(title_pos + 1)) title_pos++;
                        w->title[i++] = *title_pos++;
                    }
                    w->title[i] = '\0';
                }
                if (w->app_id[0] == '\0') strcpy(w->app_id, "Window");
                if (w->title[0] == '\0') strcpy(w->title, w->app_id);
                app->win_count++;
            }
        }
        p = id_pos + 6;
    }
    update_filters(app);
}

static void refresh_system_status(IslandApp *app) {
    time_t now = time(NULL);
    if (now - app->last_status_refresh < 2) return;
    app->last_status_refresh = now;

    /* DND flag */
    const char *rt = getenv("XDG_RUNTIME_DIR");
    char dnd_path[256];
    snprintf(dnd_path, sizeof(dnd_path), "%s/infynitae/do-not-disturb", rt ? rt : "/tmp");
    app->dnd_on = (access(dnd_path, F_OK) == 0);

    /* Battery */
    app->battery_pct = -1;
    app->battery_charging = false;
    for (int i = 0; i <= 1; i++) {
        char cap_path[128], st_path[128];
        snprintf(cap_path, sizeof(cap_path), "/sys/class/power_supply/BAT%d/capacity", i);
        snprintf(st_path, sizeof(st_path), "/sys/class/power_supply/BAT%d/status", i);
        FILE *f = fopen(cap_path, "r");
        if (f) {
            if (fscanf(f, "%d", &app->battery_pct) != 1) app->battery_pct = -1;
            fclose(f);
            FILE *sf = fopen(st_path, "r");
            if (sf) {
                char st[32] = {0};
                if (fgets(st, sizeof(st), sf)) {
                    app->battery_charging = ( strncmp(st, "Charging", 8) == 0 || strncmp(st, "Full", 4) == 0 );
                }
                fclose(sf);
            }
            break;
        }
    }

    /* Single lightweight shell status query for Wi-Fi, BT, Volume, Power profile */
    char line[256];
    if (read_cmd_line(
            "wifi=$(nmcli -t -f WIFI g 2>/dev/null); "
            "ssid=$(nmcli -t -f active,ssid dev wifi 2>/dev/null | sed -n 's/^yes://p' | head -1); "
            "bt=$(bluetoothctl show 2>/dev/null | grep -q 'Powered: yes' && echo on || echo off); "
            "vol=$(LC_ALL=C pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null | grep -o '[0-9]*%' | head -1 | tr -d '%'); "
            "mute=$(LC_ALL=C pactl get-sink-mute @DEFAULT_SINK@ 2>/dev/null | awk '{print $2}'); "
            "pp=$(powerprofilesctl get 2>/dev/null || echo balanced); "
            "printf '%s|%s|%s|%s|%s|%s\\n' \"$wifi\" \"$ssid\" \"$bt\" \"${vol:-75}\" \"${mute:-no}\" \"$pp\"",
            line, sizeof(line))) {
        char *save = NULL;
        char *wifi = strtok_r(line, "|", &save);
        char *ssid = strtok_r(NULL, "|", &save);
        char *bt = strtok_r(NULL, "|", &save);
        char *vol = strtok_r(NULL, "|", &save);
        char *mute = strtok_r(NULL, "|", &save);
        char *pp = strtok_r(NULL, "|", &save);

        /* When tokens are empty, strtok_r skips consecutive delimiters; handle carefully */
        (void)wifi; (void)ssid; (void)bt; (void)vol; (void)mute; (void)pp;
    }

    /* Parse each field reliably with a fixed format */
    if (read_cmd_line("nmcli -t -f active,ssid dev wifi 2>/dev/null | sed -n 's/^yes://p' | head -1",
                      app->wifi_label, sizeof(app->wifi_label))) {
        app->wifi_on = true;
    } else {
        char wstate[32];
        if (read_cmd_line("nmcli radio wifi 2>/dev/null", wstate, sizeof(wstate)) &&
            strcmp(wstate, "enabled") == 0) {
            app->wifi_on = true;
            strcpy(app->wifi_label, "On · Disconnected");
        } else {
            app->wifi_on = false;
            strcpy(app->wifi_label, "Off");
        }
    }

    char bt_st[16];
    app->bt_on = (read_cmd_line("bluetoothctl show 2>/dev/null | grep -q 'Powered: yes' && echo yes", bt_st, sizeof(bt_st)) &&
                  strcmp(bt_st, "yes") == 0);

    char vol_st[32];
    if (read_cmd_line("LC_ALL=C pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null | grep -o '[0-9]*%' | head -1 | tr -d '%'",
                      vol_st, sizeof(vol_st))) {
        app->volume_pct = atoi(vol_st);
        if (app->volume_pct < 0) app->volume_pct = 0;
        if (app->volume_pct > 100) app->volume_pct = 100;
    } else {
        app->volume_pct = 75;
    }

    char mute_st[16];
    app->volume_muted = (read_cmd_line("LC_ALL=C pactl get-sink-mute @DEFAULT_SINK@ 2>/dev/null | awk '{print $2}'",
                                       mute_st, sizeof(mute_st)) &&
                         strcmp(mute_st, "yes") == 0);

    if (!read_cmd_line("powerprofilesctl get 2>/dev/null", app->power_profile, sizeof(app->power_profile))) {
        strcpy(app->power_profile, "balanced");
    }
}

/* ========================================================================== */
/* Cairo + Pango Rendering Helpers                                            */
/* ========================================================================== */
static void draw_rounded_rect(cairo_t *cr, double x, double y, double w, double h, double r) {
    if (r > w / 2.0) r = w / 2.0;
    if (r > h / 2.0) r = h / 2.0;
    cairo_new_sub_path(cr);
    cairo_arc(cr, x + w - r, y + r, r, -M_PI / 2, 0);
    cairo_arc(cr, x + w - r, y + h - r, r, 0, M_PI / 2);
    cairo_arc(cr, x + r, y + h - r, r, M_PI / 2, M_PI);
    cairo_arc(cr, x + r, y + r, r, M_PI, 3 * M_PI / 2);
    cairo_close_path(cr);
}

static void draw_text_centered(cairo_t *cr, PangoLayout *layout, const char *font_str,
                               const char *text, double cx, double cy, bool use_ink_y) {
    PangoFontDescription *desc = pango_font_description_from_string(font_str);
    pango_layout_set_font_description(layout, desc);
    pango_font_description_free(desc);
    pango_layout_set_text(layout, text, -1);

    PangoRectangle ink, logical;
    pango_layout_get_extents(layout, &ink, &logical);
    double ox = ((double)ink.x + (double)ink.width / 2.0) / (double)PANGO_SCALE;
    double oy = use_ink_y
        ? ((double)ink.y + (double)ink.height / 2.0) / (double)PANGO_SCALE
        : ((double)logical.y + (double)logical.height / 2.0) / (double)PANGO_SCALE;
    cairo_move_to(cr, cx - ox, cy - oy);
    pango_cairo_show_layout(cr, layout);
}

static void draw_text_left(cairo_t *cr, PangoLayout *layout, const char *font_str,
                           const char *text, double x, double cy, double max_w) {
    PangoFontDescription *desc = pango_font_description_from_string(font_str);
    pango_layout_set_font_description(layout, desc);
    pango_font_description_free(desc);
    if (max_w > 0) {
        pango_layout_set_width(layout, (int)(max_w * PANGO_SCALE));
        pango_layout_set_ellipsize(layout, PANGO_ELLIPSIZE_END);
    } else {
        pango_layout_set_width(layout, -1);
        pango_layout_set_ellipsize(layout, PANGO_ELLIPSIZE_NONE);
    }
    pango_layout_set_text(layout, text, -1);

    PangoRectangle ink, logical;
    pango_layout_get_extents(layout, &ink, &logical);
    double oy = ((double)logical.y + (double)logical.height / 2.0) / (double)PANGO_SCALE;
    cairo_move_to(cr, x, cy - oy);
    pango_cairo_show_layout(cr, layout);
    pango_layout_set_width(layout, -1);
    pango_layout_set_ellipsize(layout, PANGO_ELLIPSIZE_NONE);
}

static void add_hitbox(IslandApp *app, double x, double y, double w, double h, int action_id, int index) {
    if (app->hitbox_count >= MAX_HITBOXES) return;
    HitBox *hb = &app->hitboxes[app->hitbox_count++];
    hb->x = x;
    hb->y = y;
    hb->w = w;
    hb->h = h;
    hb->action_id = action_id;
    hb->index = index;
}

static void load_position_config(IslandApp *app) {
    app->bottom_edge = false;
    app->control_on_left = false;
    const char *home = getenv("HOME");
    if (!home) return;
    char path[512];
    snprintf(path, sizeof(path), "%s/.config/infynitae/config", home);
    FILE *f = fopen(path, "r");
    if (!f) return;
    char line[256];
    while (fgets(line, sizeof(line), f)) {
        if (strncmp(line, "ISLAND_EDGE=", 12) == 0) {
            if (strstr(line + 12, "bottom")) app->bottom_edge = true;
            else if (strstr(line + 12, "top")) app->bottom_edge = false;
        } else if (strncmp(line, "ISLAND_CONTROL_SIDE=", 20) == 0) {
            if (strstr(line + 20, "left")) app->control_on_left = true;
            else if (strstr(line + 20, "right")) app->control_on_left = false;
        }
    }
    fclose(f);
}

static void save_position_config(const IslandApp *app) {
    char cmd[512];
    const char *edge = app->bottom_edge ? "bottom" : "top";
    const char *side = app->control_on_left ? "left" : "right";
    snprintf(cmd, sizeof(cmd),
             "cfg=\"$HOME/.config/infynitae/config\"; "
             "if [ -f \"$cfg\" ]; then "
             "  if grep -q '^ISLAND_EDGE=' \"$cfg\"; then sed -i 's/^ISLAND_EDGE=.*/ISLAND_EDGE=\"%s\"/' \"$cfg\"; else echo 'ISLAND_EDGE=\"%s\"' >> \"$cfg\"; fi; "
             "  if grep -q '^ISLAND_CONTROL_SIDE=' \"$cfg\"; then sed -i 's/^ISLAND_CONTROL_SIDE=.*/ISLAND_CONTROL_SIDE=\"%s\"/' \"$cfg\"; else echo 'ISLAND_CONTROL_SIDE=\"%s\"' >> \"$cfg\"; fi; "
             "fi",
             edge, edge, side, side);
    spawn_detached(cmd);
}

static void update_surface_input_region(IslandOutput *out) {
    IslandApp *app = out->app;
    if (!out->surface || !app->compositor) return;

    struct wl_region *reg = wl_compositor_create_region(app->compositor);
    if (app->mode == MODE_CLOSED && app->anim_progress <= 0.02 && !app->dragging) {
        /* Only two small 8px-tall touch/click zones on the configured edge (top or bottom):
         * 1. Center pill (180px wide around center)
         * 2. Corner gesture strip (180px wide on the configured left or right side)
         * Everything else—including window [X] buttons and browser tabs—is 100% unblocked! */
        int strip_y = app->bottom_edge ? (out->height - 8) : 0;
        int pill_w = 180;
        wl_region_add(reg, (out->width - pill_w) / 2, strip_y, pill_w, 8);
        int corner_w = 180;
        if (app->control_on_left) {
            wl_region_add(reg, 0, strip_y, corner_w, 8);
        } else {
            wl_region_add(reg, out->width - corner_w, strip_y, corner_w, 8);
        }
        zwlr_layer_surface_v1_set_keyboard_interactivity(
            out->layer_surface, ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE);
    } else {
        /* Full surface captures outside-click/tap to dismiss cleanly */
        wl_region_add(reg, 0, 0, out->width, out->height);
        zwlr_layer_surface_v1_set_keyboard_interactivity(
            out->layer_surface, ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_ON_DEMAND);
    }
    wl_surface_set_input_region(out->surface, reg);
    wl_region_destroy(reg);
}

/* ========================================================================== */
/* Main Island + Control Box Renderer                                         */
/* ========================================================================== */
static bool allocate_buffer(IslandOutput *out) {
    if (out->buffer && out->buf_w == out->width && out->buf_h == out->height) return true;
    if (out->buffer) {
        wl_buffer_destroy(out->buffer);
        out->buffer = NULL;
    }
    if (out->shm_data) {
        munmap(out->shm_data, out->shm_size);
        out->shm_data = NULL;
    }

    int stride = out->width * 4;
    size_t size = (size_t)stride * out->height;
    char tmpl[] = "/tmp/infynitae-island-XXXXXX";
    int fd = mkstemp(tmpl);
    if (fd < 0) return false;
    unlink(tmpl);
    if (ftruncate(fd, size) < 0) {
        close(fd);
        return false;
    }
    void *data = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (data == MAP_FAILED) {
        close(fd);
        return false;
    }
    struct wl_shm_pool *pool = wl_shm_create_pool(out->app->shm, fd, size);
    out->buffer = wl_shm_pool_create_buffer(pool, 0, out->width, out->height, stride, WL_SHM_FORMAT_ARGB8888);
    wl_shm_pool_destroy(pool);
    close(fd);

    out->shm_data = data;
    out->shm_size = size;
    out->buf_w = out->width;
    out->buf_h = out->height;
    return true;
}

static void render_output(IslandOutput *out) {
    if (!out->configured || out->width <= 0 || out->height <= 0) return;
    if (!allocate_buffer(out)) return;

    IslandApp *app = out->app;
    int w = out->width;
    int h = out->height;

    memset(out->shm_data, 0, out->shm_size);
    cairo_surface_t *surf = cairo_image_surface_create_for_data(
        (unsigned char *)out->shm_data, CAIRO_FORMAT_ARGB32, w, h, w * 4);
    cairo_t *cr = cairo_create(surf);
    PangoLayout *layout = pango_cairo_create_layout(cr);

    app->hitbox_count = 0;
    double p = app->anim_progress;
    if (p < 0.0) p = 0.0;
    if (p > 1.0) p = 1.0;
    /* Smooth cubic ease-out curve for crisp Apple-style spring feel */
    double ease = 1.0 - pow(1.0 - p, 3.0);

    /* 1. Collapsed subtle Dynamic Island handle when closed (at Top or Bottom edge) */
    if (p < 0.15) {
        double pill_w = 116.0;
        double pill_h = 5.0;
        double pill_x = (w - pill_w) / 2.0;
        double pill_y = app->bottom_edge ? (h - 8.0) : 3.0;
        double hit_y = app->bottom_edge ? (h - 12.0) : 0.0;
        cairo_set_source_rgba(cr, 0.75, 0.85, 1.0, 0.24 * (1.0 - p * 5.0));
        draw_rounded_rect(cr, pill_x, pill_y, pill_w, pill_h, 2.5);
        cairo_fill(cr);
        add_hitbox(app, w * 0.25, hit_y, w * 0.47, 12, ACT_PILL_TAP, 0);
    }

    if (p > 0.01) {
        /* Subtle backdrop dim when open */
        cairo_set_source_rgba(cr, 0.02, 0.03, 0.05, 0.28 * ease);
        cairo_rectangle(cr, 0, 0, w, h);
        cairo_fill(cr);

        IslandMode active_m = (app->mode != MODE_CLOSED) ? app->mode : app->target_mode;
        bool show_center = (active_m == MODE_CENTER || active_m == MODE_HYBRID);
        bool show_control = (active_m == MODE_CONTROL || active_m == MODE_HYBRID);

        /* Calculate placement (Top or Bottom edge, Left or Right control box) */
        double ctrl_w = 356.0;
        double ctrl_h = 376.0;
        double ctrl_x = app->control_on_left ? 18.0 : (w - ctrl_w - 18.0);
        double ctrl_y = app->bottom_edge
            ? (h + ctrl_h * (1.0 - ease) - (ctrl_h + 14.0) * ease)
            : (-ctrl_h * (1.0 - ease) + 14.0 * ease);

        double isl_w = 720.0;
        double isl_h = 336.0;
        double isl_x = (w - isl_w) / 2.0;
        if (show_control) {
            if (app->control_on_left) {
                double min_x = ctrl_x + ctrl_w + 16.0;
                if (isl_x < min_x) {
                    isl_x = min_x;
                    if (isl_x + isl_w > w - 18.0) isl_w = (w - 18.0) - isl_x;
                }
            } else {
                if (isl_x + isl_w + 16.0 > ctrl_x) {
                    isl_x = ctrl_x - isl_w - 16.0;
                    if (isl_x < 18.0) {
                        isl_x = 18.0;
                        isl_w = ctrl_x - 34.0;
                    }
                }
            }
            if (isl_w < 460.0) isl_w = 460.0;
        }
        double isl_y = app->bottom_edge
            ? (h + isl_h * (1.0 - ease) - (isl_h + 14.0) * ease)
            : (-isl_h * (1.0 - ease) + 14.0 * ease);

        /* ─────────────────────────────────────────────────────────────────── */
        /* A. TOP-CENTER DYNAMIC ISLAND (Search + Open Tabs + App Launcher)    */
        /* ─────────────────────────────────────────────────────────────────── */
        if (show_center) {
            /* Outer card shadow & glass body */
            cairo_set_source_rgba(cr, 0.05, 0.07, 0.10, 0.95 * ease);
            draw_rounded_rect(cr, isl_x, isl_y, isl_w, isl_h, 24.0);
            cairo_fill(cr);

            cairo_set_source_rgba(cr, 0.35, 0.52, 0.82, 0.42 * ease);
            cairo_set_line_width(cr, 1.4);
            draw_rounded_rect(cr, isl_x, isl_y, isl_w, isl_h, 24.0);
            cairo_stroke(cr);

            /* 1. Search Bar + Overview & Control buttons */
            double sb_x = isl_x + 18.0;
            double sb_y = isl_y + 16.0;
            double btn_ov_w = 96.0;
            double btn_ct_w = 88.0;
            double sb_w = isl_w - 36.0 - btn_ov_w - btn_ct_w - 16.0;
            double sb_h = 40.0;

            cairo_set_source_rgba(cr, 0.09, 0.13, 0.19, 0.95);
            draw_rounded_rect(cr, sb_x, sb_y, sb_w, sb_h, 14.0);
            cairo_fill(cr);

            cairo_set_source_rgba(cr, 0.42, 0.62, 0.96, 0.55);
            cairo_set_line_width(cr, 1.2);
            draw_rounded_rect(cr, sb_x, sb_y, sb_w, sb_h, 14.0);
            cairo_stroke(cr);

            if (app->search_len > 0) {
                cairo_set_source_rgba(cr, 0.94, 0.97, 1.0, 0.96);
                draw_text_left(cr, layout, "Sans Medium 11", app->search, sb_x + 16.0, sb_y + sb_h / 2.0, sb_w - 48.0);
                /* Clear [×] button */
                cairo_set_source_rgba(cr, 0.65, 0.75, 0.90, 0.80);
                draw_text_centered(cr, layout, "Sans Bold 11", "×", sb_x + sb_w - 20.0, sb_y + sb_h / 2.0, true);
                add_hitbox(app, sb_x + sb_w - 36.0, sb_y, 36.0, sb_h, ACT_SEARCH_CLEAR, 0);
            } else {
                cairo_set_source_rgba(cr, 0.52, 0.60, 0.74, 0.75);
                draw_text_left(cr, layout, "Sans 11", "Search open tabs or launch apps…", sb_x + 16.0, sb_y + sb_h / 2.0, sb_w - 24.0);
            }

            /* Overview Button */
            double ov_x = sb_x + sb_w + 8.0;
            cairo_set_source_rgba(cr, 0.12, 0.17, 0.25, 0.92);
            draw_rounded_rect(cr, ov_x, sb_y, btn_ov_w, sb_h, 14.0);
            cairo_fill(cr);
            cairo_set_source_rgba(cr, 0.85, 0.91, 1.0, 0.92);
            draw_text_centered(cr, layout, "Sans Medium 10", "Overview", ov_x + btn_ov_w / 2.0, sb_y + sb_h / 2.0, false);
            add_hitbox(app, ov_x, sb_y, btn_ov_w, sb_h, ACT_OVERVIEW, 0);

            /* Toggle Control Box Button */
            double ct_x = ov_x + btn_ov_w + 8.0;
            bool ctrl_lit = show_control;
            cairo_set_source_rgba(cr, ctrl_lit ? 0.22 : 0.12, ctrl_lit ? 0.38 : 0.17, ctrl_lit ? 0.65 : 0.25, 0.92);
            draw_rounded_rect(cr, ct_x, sb_y, btn_ct_w, sb_h, 14.0);
            cairo_fill(cr);
            cairo_set_source_rgba(cr, 0.90, 0.95, 1.0, 0.94);
            draw_text_centered(cr, layout, "Sans Medium 10", "Controls", ct_x + btn_ct_w / 2.0, sb_y + sb_h / 2.0, false);
            add_hitbox(app, ct_x, sb_y, btn_ct_w, sb_h, ACT_TOGGLE_HYBRID, 0);

            /* 2. Open Canvas Tabs (Windows Taskbar-style horizontal cards) */
            double sec1_y = sb_y + sb_h + 16.0;
            char tab_hdr[64];
            snprintf(tab_hdr, sizeof(tab_hdr), "OPEN ON CANVAS (%d)", app->filtered_win_count);
            cairo_set_source_rgba(cr, 0.56, 0.66, 0.82, 0.78);
            draw_text_left(cr, layout, "Sans Bold 8.5", tab_hdr, isl_x + 20.0, sec1_y, 300.0);

            double tab_y = sec1_y + 14.0;
            double tab_h = 62.0;
            int max_tabs = 4;
            double gap = 10.0;
            double tab_w = (isl_w - 36.0 - gap * (max_tabs - 1)) / max_tabs;

            if (app->filtered_win_count == 0) {
                cairo_set_source_rgba(cr, 0.08, 0.11, 0.16, 0.70);
                draw_rounded_rect(cr, isl_x + 18.0, tab_y, isl_w - 36.0, tab_h, 14.0);
                cairo_fill(cr);
                cairo_set_source_rgba(cr, 0.50, 0.58, 0.70, 0.75);
                draw_text_centered(cr, layout, "Sans 10", "No matching open windows on canvas", isl_x + isl_w / 2.0, tab_y + tab_h / 2.0, false);
            } else {
                int count = app->filtered_win_count < max_tabs ? app->filtered_win_count : max_tabs;
                for (int i = 0; i < count; i++) {
                    CanvasWindow *cw = &app->windows[app->filtered_wins[i]];
                    double tx = isl_x + 18.0 + i * (tab_w + gap);
                    bool sel = (app->selected_idx == i);

                    if (sel || cw->focused) {
                        cairo_set_source_rgba(cr, 0.16, 0.25, 0.40, 0.94);
                    } else {
                        cairo_set_source_rgba(cr, 0.10, 0.14, 0.20, 0.90);
                    }
                    draw_rounded_rect(cr, tx, tab_y, tab_w, tab_h, 14.0);
                    cairo_fill(cr);

                    if (sel) {
                        cairo_set_source_rgba(cr, 0.48, 0.68, 0.98, 0.90);
                        cairo_set_line_width(cr, 1.5);
                        draw_rounded_rect(cr, tx, tab_y, tab_w, tab_h, 14.0);
                        cairo_stroke(cr);
                    }

                    /* Active indicator dot */
                    if (cw->focused) {
                        cairo_set_source_rgba(cr, 0.38, 0.78, 0.98, 0.95);
                        cairo_arc(cr, tx + 13.0, tab_y + 20.0, 3.5, 0, 2 * M_PI);
                        cairo_fill(cr);
                    }

                    cairo_set_source_rgba(cr, 0.94, 0.97, 1.0, 0.95);
                    draw_text_left(cr, layout, "Sans Bold 10", cw->app_id,
                                   tx + (cw->focused ? 22.0 : 12.0), tab_y + 20.0, tab_w - 46.0);

                    /* Window [×] close button on the right of each tab */
                    double cx_btn = tx + tab_w - 24.0;
                    double cy_btn = tab_y + 6.0;
                    cairo_set_source_rgba(cr, 0.22, 0.14, 0.18, 0.85);
                    draw_rounded_rect(cr, cx_btn, cy_btn, 18.0, 18.0, 6.0);
                    cairo_fill(cr);
                    cairo_set_source_rgba(cr, 0.95, 0.65, 0.68, 0.92);
                    draw_text_centered(cr, layout, "Sans Bold 9", "×", cx_btn + 9.0, cy_btn + 9.0, true);

                    cairo_set_source_rgba(cr, 0.64, 0.72, 0.84, 0.85);
                    draw_text_left(cr, layout, "Sans 9", cw->title, tx + 12.0, tab_y + 42.0, tab_w - 20.0);

                    add_hitbox(app, tx, tab_y, tab_w, tab_h, ACT_WIN_FOCUS, app->filtered_wins[i]);
                    add_hitbox(app, cx_btn - 4.0, cy_btn - 4.0, 26.0, 26.0, ACT_WIN_CLOSE, app->filtered_wins[i]);
                }
            }

            /* 3. Applications Launcher Grid (2 rows × 4 columns) */
            double sec2_y = tab_y + tab_h + 18.0;
            cairo_set_source_rgba(cr, 0.56, 0.66, 0.82, 0.78);
            draw_text_left(cr, layout, "Sans Bold 8.5", "APPLICATIONS", isl_x + 20.0, sec2_y, 240.0);

            double grid_y = sec2_y + 14.0;
            int cols = 4;
            int rows = 2;
            double cell_w = (isl_w - 36.0 - gap * (cols - 1)) / cols;
            double cell_h = 46.0;
            int shown_wins = app->filtered_win_count < max_tabs ? app->filtered_win_count : max_tabs;
            int shown_apps = app->filtered_app_count < (cols * rows) ? app->filtered_app_count : (cols * rows);

            for (int i = 0; i < shown_apps; i++) {
                DesktopApp *da = &app->apps[app->filtered_apps[i]];
                int r = i / cols;
                int c = i % cols;
                double ax = isl_x + 18.0 + c * (cell_w + gap);
                double ay = grid_y + r * (cell_h + 8.0);
                bool sel = (app->selected_idx == shown_wins + i);

                cairo_set_source_rgba(cr, sel ? 0.20 : 0.10, sel ? 0.32 : 0.14, sel ? 0.52 : 0.20, 0.90);
                draw_rounded_rect(cr, ax, ay, cell_w, cell_h, 12.0);
                cairo_fill(cr);

                if (sel) {
                    cairo_set_source_rgba(cr, 0.48, 0.68, 0.98, 0.90);
                    cairo_set_line_width(cr, 1.4);
                    draw_rounded_rect(cr, ax, ay, cell_w, cell_h, 12.0);
                    cairo_stroke(cr);
                }

                /* App initial badge */
                cairo_set_source_rgba(cr, 0.18, 0.26, 0.38, 0.95);
                cairo_arc(cr, ax + 21.0, ay + cell_h / 2.0, 12.0, 0, 2 * M_PI);
                cairo_fill(cr);

                char init[2] = { (char)toupper((unsigned char)da->name[0]), '\0' };
                cairo_set_source_rgba(cr, 0.88, 0.94, 1.0, 0.95);
                draw_text_centered(cr, layout, "Sans Bold 9", init, ax + 21.0, ay + cell_h / 2.0, true);

                cairo_set_source_rgba(cr, 0.92, 0.96, 1.0, 0.94);
                draw_text_left(cr, layout, "Sans Medium 9.5", da->name, ax + 40.0, ay + cell_h / 2.0, cell_w - 48.0);

                add_hitbox(app, ax, ay, cell_w, cell_h, ACT_APP_LAUNCH, app->filtered_apps[i]);
            }
        }

        /* ─────────────────────────────────────────────────────────────────── */
        /* B. TOP-RIGHT CONTROL BOX (Windows 11 / iPadOS Quick Settings)       */
        /* ─────────────────────────────────────────────────────────────────── */
        if (show_control) {
            cairo_set_source_rgba(cr, 0.05, 0.07, 0.10, 0.95 * ease);
            draw_rounded_rect(cr, ctrl_x, ctrl_y, ctrl_w, ctrl_h, 24.0);
            cairo_fill(cr);

            cairo_set_source_rgba(cr, 0.35, 0.52, 0.82, 0.42 * ease);
            cairo_set_line_width(cr, 1.4);
            draw_rounded_rect(cr, ctrl_x, ctrl_y, ctrl_w, ctrl_h, 24.0);
            cairo_stroke(cr);

            /* 1. Clock, Date, Position Switchers (Top/Bot & Left/Right) & Battery Header */
            time_t now = time(NULL);
            struct tm *tm_info = localtime(&now);
            char t_str[32], d_str[64];
            strftime(t_str, sizeof(t_str), "%H:%M", tm_info);
            strftime(d_str, sizeof(d_str), "%a, %b %d", tm_info);

            cairo_set_source_rgba(cr, 0.95, 0.98, 1.0, 0.96);
            draw_text_left(cr, layout, "Sans Bold 17", t_str, ctrl_x + 18.0, ctrl_y + 30.0, 90.0);

            cairo_set_source_rgba(cr, 0.62, 0.70, 0.84, 0.86);
            draw_text_left(cr, layout, "Sans Medium 9.5", d_str, ctrl_x + 86.0, ctrl_y + 31.0, 96.0);

            /* Position toggle pills: [⇅ Top/Bot] and [⇄ L/R] */
            double sw_h = 24.0;
            double sw_y = ctrl_y + 18.0;
            double sw_edge_w = 46.0;
            double sw_side_w = 46.0;
            double right_cursor = ctrl_x + ctrl_w - 18.0;

            if (app->battery_pct >= 0) {
                char bat_str[32];
                snprintf(bat_str, sizeof(bat_str), "%s%d%%", app->battery_charging ? "⚡" : "", app->battery_pct);
                double bw = 52.0;
                right_cursor -= bw;
                cairo_set_source_rgba(cr, 0.12, 0.18, 0.26, 0.92);
                draw_rounded_rect(cr, right_cursor, sw_y, bw, sw_h, 12.0);
                cairo_fill(cr);
                cairo_set_source_rgba(cr, 0.78, 0.92, 0.84, 0.95);
                draw_text_centered(cr, layout, "Sans Bold 8.5", bat_str, right_cursor + bw / 2.0, sw_y + sw_h / 2.0, true);
                right_cursor -= 6.0;
            }

            /* [⇄ Left / Right] button */
            right_cursor -= sw_side_w;
            cairo_set_source_rgba(cr, 0.14, 0.20, 0.30, 0.92);
            draw_rounded_rect(cr, right_cursor, sw_y, sw_side_w, sw_h, 12.0);
            cairo_fill(cr);
            cairo_set_source_rgba(cr, 0.85, 0.92, 1.0, 0.92);
            draw_text_centered(cr, layout, "Sans Bold 8.5", app->control_on_left ? "⇄ Left" : "⇄ Right",
                               right_cursor + sw_side_w / 2.0, sw_y + sw_h / 2.0, false);
            add_hitbox(app, right_cursor, sw_y, sw_side_w, sw_h, ACT_SWAP_SIDE, 0);
            right_cursor -= 6.0;

            /* [⇅ Top / Bot] button */
            right_cursor -= sw_edge_w;
            cairo_set_source_rgba(cr, 0.14, 0.20, 0.30, 0.92);
            draw_rounded_rect(cr, right_cursor, sw_y, sw_edge_w, sw_h, 12.0);
            cairo_fill(cr);
            cairo_set_source_rgba(cr, 0.85, 0.92, 1.0, 0.92);
            draw_text_centered(cr, layout, "Sans Bold 8.5", app->bottom_edge ? "⇅ Bot" : "⇅ Top",
                               right_cursor + sw_edge_w / 2.0, sw_y + sw_h / 2.0, false);
            add_hitbox(app, right_cursor, sw_y, sw_edge_w, sw_h, ACT_SWAP_EDGE, 0);

            /* 2. 2×3 Quick Toggle Tiles */
            double t_start_y = ctrl_y + 56.0;
            double t_gap = 10.0;
            double tw = (ctrl_w - 36.0 - t_gap) / 2.0;
            double th = 54.0;

            struct {
                const char *title;
                const char *sub;
                bool active;
                int action;
            } tiles[6] = {
                { "Wi-Fi", app->wifi_label, app->wifi_on, ACT_WIFI_TOGGLE },
                { "Bluetooth", app->bt_on ? "On" : "Off", app->bt_on, ACT_BT_TOGGLE },
                { "Do Not Disturb", app->dnd_on ? "Active" : "Off", app->dnd_on, ACT_DND_TOGGLE },
                { "Tray & Apps", "Manage apps", false, ACT_TRAY_MENU },
                { "Power Mode", app->power_profile, strcmp(app->power_profile, "performance") == 0, ACT_POWER_PROFILE },
                { "Canvas Radar", "Overview", false, ACT_OVERVIEW }
            };

            for (int i = 0; i < 6; i++) {
                int r = i / 2;
                int c = i % 2;
                double tx = ctrl_x + 18.0 + c * (tw + t_gap);
                double ty = t_start_y + r * (th + t_gap);

                if (tiles[i].active) {
                    cairo_set_source_rgba(cr, 0.20, 0.42, 0.78, 0.92);
                } else {
                    cairo_set_source_rgba(cr, 0.10, 0.14, 0.20, 0.92);
                }
                draw_rounded_rect(cr, tx, ty, tw, th, 14.0);
                cairo_fill(cr);

                cairo_set_source_rgba(cr, 0.95, 0.98, 1.0, 0.96);
                draw_text_left(cr, layout, "Sans Bold 10", tiles[i].title, tx + 14.0, ty + 19.0, tw - 24.0);

                cairo_set_source_rgba(cr, tiles[i].active ? 0.82 : 0.58, tiles[i].active ? 0.90 : 0.66, 0.98, 0.85);
                draw_text_left(cr, layout, "Sans 9", tiles[i].sub, tx + 14.0, ty + 37.0, tw - 24.0);

                add_hitbox(app, tx, ty, tw, th, tiles[i].action, i);
            }

            /* 3. Interactive Volume Bar */
            double vy = t_start_y + 3 * (th + t_gap) + 4.0;
            double vh = 42.0;
            double vx = ctrl_x + 18.0;
            double vw = ctrl_w - 36.0;

            cairo_set_source_rgba(cr, 0.09, 0.13, 0.19, 0.94);
            draw_rounded_rect(cr, vx, vy, vw, vh, 14.0);
            cairo_fill(cr);

            /* [-] button */
            add_hitbox(app, vx, vy, 38.0, vh, ACT_VOL_DOWN, 0);
            cairo_set_source_rgba(cr, 0.85, 0.92, 1.0, 0.90);
            draw_text_centered(cr, layout, "Sans Bold 13", "−", vx + 19.0, vy + vh / 2.0, true);

            /* Volume track */
            double bar_x = vx + 42.0;
            double bar_w = vw - 132.0;
            double bar_y = vy + (vh - 10.0) / 2.0;
            cairo_set_source_rgba(cr, 0.16, 0.22, 0.30, 0.95);
            draw_rounded_rect(cr, bar_x, bar_y, bar_w, 10.0, 5.0);
            cairo_fill(cr);

            double fill_w = (bar_w * (app->volume_muted ? 0 : app->volume_pct)) / 100.0;
            if (fill_w > 6.0) {
                cairo_set_source_rgba(cr, 0.38, 0.68, 0.98, 0.95);
                draw_rounded_rect(cr, bar_x, bar_y, fill_w, 10.0, 5.0);
                cairo_fill(cr);
            }
            add_hitbox(app, bar_x, vy, bar_w, vh, ACT_VOL_BAR, (int)bar_x);

            /* [+] button */
            double plus_x = bar_x + bar_w + 4.0;
            add_hitbox(app, plus_x, vy, 36.0, vh, ACT_VOL_UP, 0);
            cairo_set_source_rgba(cr, 0.85, 0.92, 1.0, 0.90);
            draw_text_centered(cr, layout, "Sans Bold 13", "+", plus_x + 18.0, vy + vh / 2.0, true);

            /* Mute / % pill */
            char v_lbl[16];
            if (app->volume_muted) strcpy(v_lbl, "Mute");
            else snprintf(v_lbl, sizeof(v_lbl), "%d%%", app->volume_pct);
            double mute_x = plus_x + 38.0;
            add_hitbox(app, mute_x, vy, 48.0, vh, ACT_VOL_MUTE, 0);
            cairo_set_source_rgba(cr, app->volume_muted ? 0.95 : 0.72, app->volume_muted ? 0.42 : 0.82, 0.42, 0.92);
            draw_text_centered(cr, layout, "Sans Bold 9.5", v_lbl, mute_x + 22.0, vy + vh / 2.0, true);

            /* 4. Session & Power Strip: Lock · Reboot · Shut Down · Log Out */
            double py = vy + vh + 12.0;
            double ph = 38.0;
            double p_gap = 8.0;
            double pw = (vw - 3 * p_gap) / 4.0;
            const char *p_labels[4] = { "Lock", "Reboot", "Power Off", "Log Out" };
            int p_acts[4] = { ACT_LOCK, ACT_REBOOT, ACT_POWEROFF, ACT_LOGOUT };

            for (int i = 0; i < 4; i++) {
                double px = vx + i * (pw + p_gap);
                bool danger = (i == 2);
                cairo_set_source_rgba(cr, danger ? 0.24 : 0.11, danger ? 0.11 : 0.15, danger ? 0.13 : 0.22, 0.92);
                draw_rounded_rect(cr, px, py, pw, ph, 11.0);
                cairo_fill(cr);

                cairo_set_source_rgba(cr, danger ? 0.98 : 0.86, danger ? 0.68 : 0.91, danger ? 0.68 : 0.98, 0.94);
                draw_text_centered(cr, layout, "Sans Medium 9", p_labels[i], px + pw / 2.0, py + ph / 2.0, false);
                add_hitbox(app, px, py, pw, ph, p_acts[i], 0);
            }
        }
    }

    g_object_unref(layout);
    cairo_destroy(cr);
    cairo_surface_destroy(surf);

    update_surface_input_region(out);
    wl_surface_attach(out->surface, out->buffer, 0, 0);
    wl_surface_damage_buffer(out->surface, 0, 0, w, h);
    wl_surface_commit(out->surface);
}

/* ========================================================================== */
/* Actions & Hit Testing                                                      */
/* ========================================================================== */
static void set_mode(IslandApp *app, IslandMode new_mode) {
    if (new_mode != MODE_CLOSED) {
        app->target_mode = new_mode;
        app->mode = new_mode;
        app->search[0] = '\0';
        app->search_len = 0;
        app->selected_idx = 0;
        refresh_snapshot(app);
        refresh_system_status(app);
    } else {
        app->mode = MODE_CLOSED;
    }
    damage_all(app);
}

static void activate_selected(IslandApp *app) {
    int max_tabs = 4;
    int shown_wins = app->filtered_win_count < max_tabs ? app->filtered_win_count : max_tabs;
    if (app->selected_idx < shown_wins && app->filtered_win_count > 0) {
        CanvasWindow *cw = &app->windows[app->filtered_wins[app->selected_idx]];
        char cmd[256];
        snprintf(cmd, sizeof(cmd),
                 "infynitae msg navigate element %lu %lu && infynitae msg focus %lu %lu",
                 cw->inst, cw->elem, cw->inst, cw->elem);
        spawn_detached(cmd);
        set_mode(app, MODE_CLOSED);
    } else {
        int app_idx = app->selected_idx - shown_wins;
        if (app_idx >= 0 && app_idx < app->filtered_app_count) {
            DesktopApp *da = &app->apps[app->filtered_apps[app_idx]];
            spawn_detached(da->exec_cmd);
            set_mode(app, MODE_CLOSED);
        }
    }
}

static bool handle_click_at(IslandApp *app, IslandOutput *out, double x, double y) {
    (void)out;
    for (int i = app->hitbox_count - 1; i >= 0; i--) {
        HitBox *hb = &app->hitboxes[i];
        if (x >= hb->x && x <= hb->x + hb->w && y >= hb->y && y <= hb->y + hb->h) {
            switch (hb->action_id) {
            case ACT_PILL_TAP:
                set_mode(app, MODE_HYBRID);
                return true;
            case ACT_SEARCH_CLEAR:
                app->search[0] = '\0';
                app->search_len = 0;
                update_filters(app);
                damage_all(app);
                return true;
            case ACT_OVERVIEW:
                set_mode(app, MODE_CLOSED);
                spawn_detached("infynitae msg overview");
                return true;
            case ACT_TOGGLE_HYBRID:
                app->mode = (app->mode == MODE_HYBRID) ? MODE_CENTER : MODE_HYBRID;
                app->target_mode = app->mode;
                refresh_system_status(app);
                damage_all(app);
                return true;
            case ACT_WIN_FOCUS: {
                CanvasWindow *cw = &app->windows[hb->index];
                char cmd[256];
                snprintf(cmd, sizeof(cmd),
                         "infynitae msg navigate element %lu %lu && infynitae msg focus %lu %lu",
                         cw->inst, cw->elem, cw->inst, cw->elem);
                set_mode(app, MODE_CLOSED);
                spawn_detached(cmd);
                return true;
            }
            case ACT_WIN_CLOSE: {
                CanvasWindow *cw = &app->windows[hb->index];
                char cmd[256];
                snprintf(cmd, sizeof(cmd),
                         "infynitae msg focus %lu %lu && infynitae msg shortcut close; sleep 0.1",
                         cw->inst, cw->elem);
                spawn_detached(cmd);
                usleep(90000);
                refresh_snapshot(app);
                damage_all(app);
                return true;
            }
            case ACT_SWAP_SIDE:
                app->control_on_left = !app->control_on_left;
                save_position_config(app);
                damage_all(app);
                return true;
            case ACT_SWAP_EDGE:
                app->bottom_edge = !app->bottom_edge;
                save_position_config(app);
                damage_all(app);
                return true;
            case ACT_APP_LAUNCH: {
                DesktopApp *da = &app->apps[hb->index];
                set_mode(app, MODE_CLOSED);
                spawn_detached(da->exec_cmd);
                return true;
            }
            case ACT_WIFI_TOGGLE:
                if (app->wifi_on) spawn_detached("nmcli radio wifi off");
                else spawn_detached("nmcli radio wifi on");
                app->wifi_on = !app->wifi_on;
                strcpy(app->wifi_label, app->wifi_on ? "Turning on…" : "Off");
                damage_all(app);
                return true;
            case ACT_BT_TOGGLE:
                if (app->bt_on) spawn_detached("bluetoothctl power off");
                else spawn_detached("bluetoothctl power on");
                app->bt_on = !app->bt_on;
                damage_all(app);
                return true;
            case ACT_DND_TOGGLE: {
                const char *rt = getenv("XDG_RUNTIME_DIR");
                char dnd_path[256];
                snprintf(dnd_path, sizeof(dnd_path), "%s/infynitae/do-not-disturb", rt ? rt : "/tmp");
                if (app->dnd_on) {
                    unlink(dnd_path);
                    spawn_detached("makoctl mode -r do-not-disturb 2>/dev/null || swaync-client --dnd-off 2>/dev/null");
                    app->dnd_on = false;
                } else {
                    char dir_cmd[300];
                    snprintf(dir_cmd, sizeof(dir_cmd), "mkdir -p '%s/infynitae' && : > '%s'", rt ? rt : "/tmp", dnd_path);
                    spawn_detached(dir_cmd);
                    spawn_detached("makoctl mode -a do-not-disturb 2>/dev/null || swaync-client --dnd-on 2>/dev/null");
                    app->dnd_on = true;
                }
                damage_all(app);
                return true;
            }
            case ACT_TRAY_MENU:
                set_mode(app, MODE_CLOSED);
                spawn_detached("sh \"$HOME/infynitae/live/control-menu\"");
                return true;
            case ACT_POWER_PROFILE: {
                const char *next_p = "balanced";
                if (strcmp(app->power_profile, "balanced") == 0) next_p = "performance";
                else if (strcmp(app->power_profile, "performance") == 0) next_p = "power-saver";
                strncpy(app->power_profile, next_p, sizeof(app->power_profile) - 1);
                char cmd[128];
                snprintf(cmd, sizeof(cmd), "powerprofilesctl set %s 2>/dev/null", next_p);
                spawn_detached(cmd);
                damage_all(app);
                return true;
            }
            case ACT_VOL_DOWN:
                app->volume_pct = (app->volume_pct >= 5) ? app->volume_pct - 5 : 0;
                spawn_detached("pactl set-sink-volume @DEFAULT_SINK@ -5%");
                damage_all(app);
                return true;
            case ACT_VOL_UP:
                app->volume_pct = (app->volume_pct <= 95) ? app->volume_pct + 5 : 100;
                spawn_detached("pactl set-sink-volume @DEFAULT_SINK@ +5% && pactl set-sink-mute @DEFAULT_SINK@ 0");
                app->volume_muted = false;
                damage_all(app);
                return true;
            case ACT_VOL_MUTE:
                app->volume_muted = !app->volume_muted;
                spawn_detached("pactl set-sink-mute @DEFAULT_SINK@ toggle");
                damage_all(app);
                return true;
            case ACT_VOL_BAR: {
                int pct = (int)(((x - hb->x) / hb->w) * 100.0 + 0.5);
                if (pct < 0) pct = 0;
                if (pct > 100) pct = 100;
                app->volume_pct = pct;
                app->volume_muted = false;
                char cmd[128];
                snprintf(cmd, sizeof(cmd), "pactl set-sink-volume @DEFAULT_SINK@ %d%% && pactl set-sink-mute @DEFAULT_SINK@ 0", pct);
                spawn_detached(cmd);
                damage_all(app);
                return true;
            }
            case ACT_LOCK:
                set_mode(app, MODE_CLOSED);
                spawn_detached("~/.local/bin/infynitae-lock");
                return true;
            case ACT_REBOOT:
                set_mode(app, MODE_CLOSED);
                spawn_detached(": > ~/.local/state/infynitae/stop; setsid -f ~/.local/bin/infynitae-lock --status 'Rebooting…'; sleep 0.25; systemctl reboot");
                return true;
            case ACT_POWEROFF:
                set_mode(app, MODE_CLOSED);
                spawn_detached(": > ~/.local/state/infynitae/stop; setsid -f ~/.local/bin/infynitae-lock --status 'Shutting down…'; sleep 0.25; systemctl poweroff");
                return true;
            case ACT_LOGOUT:
                set_mode(app, MODE_CLOSED);
                spawn_detached("~/.local/bin/infynitae-session stop");
                return true;
            }
        }
    }

    /* Clicked outside the floating cards -> dismiss */
    if (app->mode != MODE_CLOSED) {
        set_mode(app, MODE_CLOSED);
        return true;
    }
    return false;
}

/* ========================================================================== */
/* Frame Animation & Wayland Input Listeners                                  */
/* ========================================================================== */
static void on_frame_done(void *data, struct wl_callback *cb, uint32_t time_ms) {
    (void)time_ms;
    IslandOutput *out = data;
    if (cb) wl_callback_destroy(cb);
    out->frame_cb = NULL;

    IslandApp *app = out->app;
    if (app->dragging) {
        render_output(out);
        return;
    }

    double target = (app->mode == MODE_CLOSED) ? 0.0 : 1.0;
    double diff = target - app->anim_progress;
    if (fabs(diff) > 0.015) {
        app->anim_progress += diff * 0.34;
        render_output(out);
        request_frame(out);
    } else {
        app->anim_progress = target;
        render_output(out);
    }
}

static const struct wl_callback_listener frame_listener = {
    .done = on_frame_done,
};

static void request_frame(IslandOutput *out) {
    if (out->frame_cb || !out->configured) return;
    out->frame_cb = wl_surface_frame(out->surface);
    wl_callback_add_listener(out->frame_cb, &frame_listener, out);
    wl_surface_commit(out->surface);
}

static void damage_all(IslandApp *app) {
    for (IslandOutput *o = app->outputs; o; o = o->next) {
        render_output(o);
        double target = (app->mode == MODE_CLOSED) ? 0.0 : 1.0;
        if (fabs(target - app->anim_progress) > 0.01) {
            request_frame(o);
        }
    }
}

static IslandOutput *output_for_surface(IslandApp *app, struct wl_surface *s) {
    for (IslandOutput *o = app->outputs; o; o = o->next)
        if (o->surface == s) return o;
    return app->outputs;
}

/* Touch Listener: Apple-style split top-edge swipe-down & swipe-up */
static void touch_down(void *data, struct wl_touch *touch, uint32_t serial, uint32_t time,
                       struct wl_surface *surface, int32_t id, wl_fixed_t fx, wl_fixed_t fy) {
    (void)touch; (void)serial; (void)time;
    IslandApp *app = data;
    IslandOutput *out = output_for_surface(app, surface);
    double x = wl_fixed_to_double(fx);
    double y = wl_fixed_to_double(fy);

    app->touch_id = id;
    app->touch_start_x = x;
    app->touch_start_y = y;
    app->touch_cur_x = x;
    app->touch_cur_y = y;

    if (app->mode == MODE_CLOSED) {
        /* Split edge zones (respecting Left or Right Control Box setting):
         * - Corner strip (Left 28% or Right 28%) -> pulls out Control Box
         * - Center strip -> pulls out Dynamic Island (Search + Tabs + Apps) */
        app->dragging = true;
        bool in_ctrl_zone = false;
        if (out) {
            in_ctrl_zone = app->control_on_left
                ? (x <= out->width * 0.28)
                : (x >= out->width * 0.72);
        }
        app->target_mode = in_ctrl_zone ? MODE_CONTROL : MODE_CENTER;
        refresh_snapshot(app);
        refresh_system_status(app);
    }
}

static void touch_motion(void *data, struct wl_touch *touch, uint32_t time, int32_t id,
                         wl_fixed_t fx, wl_fixed_t fy) {
    (void)touch; (void)time;
    IslandApp *app = data;
    if (id != app->touch_id) return;
    app->touch_cur_x = wl_fixed_to_double(fx);
    app->touch_cur_y = wl_fixed_to_double(fy);

    /* Positive pull_dist = pulling inward from the active edge (down from top, or up from bottom) */
    double raw_dy = app->touch_cur_y - app->touch_start_y;
    double pull_dist = app->bottom_edge ? (-raw_dy) : raw_dy;

    if (app->mode == MODE_CLOSED && app->dragging) {
        double prog = pull_dist / 140.0;
        if (prog < 0.0) prog = 0.0;
        if (prog > 1.0) prog = 1.0;
        app->anim_progress = prog;
        for (IslandOutput *o = app->outputs; o; o = o->next) request_frame(o);
    } else if (app->mode != MODE_CLOSED && pull_dist < -30.0) {
        /* Swiping back toward the edge while open tracks dismissal */
        app->dragging = true;
        double prog = 1.0 + (pull_dist / 160.0);
        if (prog < 0.0) prog = 0.0;
        if (prog > 1.0) prog = 1.0;
        app->anim_progress = prog;
        for (IslandOutput *o = app->outputs; o; o = o->next) request_frame(o);
    }
}

static void touch_up(void *data, struct wl_touch *touch, uint32_t serial, uint32_t time, int32_t id) {
    (void)touch; (void)serial; (void)time;
    IslandApp *app = data;
    if (id != app->touch_id) return;

    double dx = app->touch_cur_x - app->touch_start_x;
    double raw_dy = app->touch_cur_y - app->touch_start_y;
    double pull_dist = app->bottom_edge ? (-raw_dy) : raw_dy;
    bool was_dragging = app->dragging;
    app->dragging = false;

    if (app->mode == MODE_CLOSED) {
        if (pull_dist > 36.0) {
            /* Completed swipe inward from edge -> snap open! */
            app->mode = app->target_mode;
            damage_all(app);
        } else if (hypot(dx, raw_dy) < 16.0) {
            /* Single tap on the edge pill -> open that zone */
            set_mode(app, app->target_mode);
        } else {
            set_mode(app, MODE_CLOSED);
        }
    } else {
        if (was_dragging && pull_dist < -45.0) {
            /* Swiped back toward edge -> dismiss */
            set_mode(app, MODE_CLOSED);
        } else if (hypot(dx, raw_dy) < 20.0) {
            handle_click_at(app, app->outputs, app->touch_cur_x, app->touch_cur_y);
        } else {
            damage_all(app);
        }
    }
}

static void touch_frame(void *data, struct wl_touch *t) { (void)data; (void)t; }
static void touch_cancel(void *data, struct wl_touch *t) {
    (void)t;
    IslandApp *app = data;
    app->dragging = false;
    damage_all(app);
}

static const struct wl_touch_listener touch_listener = {
    .down = touch_down,
    .up = touch_up,
    .motion = touch_motion,
    .frame = touch_frame,
    .cancel = touch_cancel,
};

/* Pointer Listener */
static void pointer_enter(void *data, struct wl_pointer *p, uint32_t s,
                          struct wl_surface *surface, wl_fixed_t fx, wl_fixed_t fy) {
    (void)p; (void)s;
    IslandApp *app = data;
    app->pointer_output = output_for_surface(app, surface);
    app->pointer_x = wl_fixed_to_double(fx);
    app->pointer_y = wl_fixed_to_double(fy);
}

static void pointer_leave(void *data, struct wl_pointer *p, uint32_t s, struct wl_surface *surface) {
    (void)data; (void)p; (void)s; (void)surface;
}

static void pointer_motion(void *data, struct wl_pointer *p, uint32_t t, wl_fixed_t fx, wl_fixed_t fy) {
    (void)p; (void)t;
    IslandApp *app = data;
    app->pointer_x = wl_fixed_to_double(fx);
    app->pointer_y = wl_fixed_to_double(fy);
}

static void pointer_button(void *data, struct wl_pointer *p, uint32_t s, uint32_t t,
                           uint32_t button, uint32_t state) {
    (void)p; (void)s; (void)t; (void)button;
    IslandApp *app = data;
    if (state != WL_POINTER_BUTTON_STATE_PRESSED) return;
    if (app->mode == MODE_CLOSED) {
        IslandOutput *out = app->pointer_output ? app->pointer_output : app->outputs;
        bool in_ctrl_zone = false;
        if (out) {
            in_ctrl_zone = app->control_on_left
                ? (app->pointer_x <= out->width * 0.28)
                : (app->pointer_x >= out->width * 0.72);
        }
        set_mode(app, in_ctrl_zone ? MODE_CONTROL : MODE_HYBRID);
        return;
    }
    handle_click_at(app, app->pointer_output, app->pointer_x, app->pointer_y);
}

static void pointer_axis(void *d, struct wl_pointer *p, uint32_t t, uint32_t a, wl_fixed_t v) {
    (void)d; (void)p; (void)t; (void)a; (void)v;
}
static void pointer_frame(void *d, struct wl_pointer *p) { (void)d; (void)p; }
static void pointer_axis_source(void *d, struct wl_pointer *p, uint32_t s) { (void)d; (void)p; (void)s; }
static void pointer_axis_stop(void *d, struct wl_pointer *p, uint32_t t, uint32_t a) { (void)d; (void)p; (void)t; (void)a; }
static void pointer_axis_discrete(void *d, struct wl_pointer *p, uint32_t a, int32_t v) { (void)d; (void)p; (void)a; (void)v; }

static const struct wl_pointer_listener pointer_listener = {
    .enter = pointer_enter,
    .leave = pointer_leave,
    .motion = pointer_motion,
    .button = pointer_button,
    .axis = pointer_axis,
    .frame = pointer_frame,
    .axis_source = pointer_axis_source,
    .axis_stop = pointer_axis_stop,
    .axis_discrete = pointer_axis_discrete,
};

/* Keyboard Listener */
static void keyboard_keymap(void *data, struct wl_keyboard *kb, uint32_t format, int32_t fd, uint32_t size) {
    (void)kb;
    IslandApp *app = data;
    if (format != WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1) { close(fd); return; }
    char *map_str = mmap(NULL, size, PROT_READ, MAP_SHARED, fd, 0);
    if (map_str == MAP_FAILED) { close(fd); return; }
    if (app->xkb_keymap) xkb_keymap_unref(app->xkb_keymap);
    if (app->xkb_state) xkb_state_unref(app->xkb_state);
    app->xkb_keymap = xkb_keymap_new_from_string(app->xkb_ctx, map_str,
        XKB_KEYMAP_FORMAT_TEXT_V1, XKB_KEYMAP_COMPILE_NO_FLAGS);
    munmap(map_str, size);
    close(fd);
    if (app->xkb_keymap) app->xkb_state = xkb_state_new(app->xkb_keymap);
}

static void keyboard_enter(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *surf, struct wl_array *keys) {
    (void)d; (void)k; (void)s; (void)surf; (void)keys;
}
static void keyboard_leave(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *surf) {
    (void)d; (void)k; (void)s; (void)surf;
}

static void keyboard_key(void *data, struct wl_keyboard *kb, uint32_t serial, uint32_t time,
                         uint32_t key, uint32_t state) {
    (void)kb; (void)serial; (void)time;
    IslandApp *app = data;
    if (state != WL_KEYBOARD_KEY_STATE_PRESSED || !app->xkb_state || app->mode == MODE_CLOSED) return;

    uint32_t keycode = key + 8;
    xkb_keysym_t sym = xkb_state_key_get_one_sym(app->xkb_state, keycode);

    if (sym == XKB_KEY_Escape) {
        set_mode(app, MODE_CLOSED);
        return;
    }
    if (sym == XKB_KEY_Return || sym == XKB_KEY_KP_Enter) {
        activate_selected(app);
        return;
    }
    if (sym == XKB_KEY_BackSpace) {
        if (app->search_len > 0) {
            app->search[--app->search_len] = '\0';
            app->selected_idx = 0;
            update_filters(app);
            damage_all(app);
        }
        return;
    }

    int shown_wins = app->filtered_win_count < 4 ? app->filtered_win_count : 4;
    int shown_apps = app->filtered_app_count < VISIBLE_APPS ? app->filtered_app_count : VISIBLE_APPS;
    int total = shown_wins + shown_apps;

    if (sym == XKB_KEY_Right || sym == XKB_KEY_Tab) {
        if (total > 0) app->selected_idx = (app->selected_idx + 1) % total;
        damage_all(app);
        return;
    }
    if (sym == XKB_KEY_Left || sym == XKB_KEY_ISO_Left_Tab) {
        if (total > 0) app->selected_idx = (app->selected_idx - 1 + total) % total;
        damage_all(app);
        return;
    }
    if (sym == XKB_KEY_Down) {
        if (total > 0) {
            int next = app->selected_idx + 4;
            app->selected_idx = (next < total) ? next : total - 1;
        }
        damage_all(app);
        return;
    }
    if (sym == XKB_KEY_Up) {
        if (total > 0) {
            int prev = app->selected_idx - 4;
            app->selected_idx = (prev >= 0) ? prev : 0;
        }
        damage_all(app);
        return;
    }

    char utf8[16] = {0};
    int len = xkb_state_key_get_utf8(app->xkb_state, keycode, utf8, sizeof(utf8));
    if (len > 0 && (unsigned char)utf8[0] >= 32 && (unsigned char)utf8[0] != 127) {
        if (app->search_len + (size_t)len < sizeof(app->search) - 1) {
            memcpy(app->search + app->search_len, utf8, (size_t)len);
            app->search_len += (size_t)len;
            app->search[app->search_len] = '\0';
            app->selected_idx = 0;
            update_filters(app);
            damage_all(app);
        }
    }
}

static void keyboard_modifiers(void *data, struct wl_keyboard *kb, uint32_t s,
                               uint32_t dep, uint32_t lat, uint32_t lock, uint32_t grp) {
    (void)kb; (void)s;
    IslandApp *app = data;
    if (app->xkb_state) xkb_state_update_mask(app->xkb_state, dep, lat, lock, 0, 0, grp);
}

static void keyboard_repeat_info(void *d, struct wl_keyboard *k, int32_t r, int32_t del) {
    (void)d; (void)k; (void)r; (void)del;
}

static const struct wl_keyboard_listener keyboard_listener = {
    .keymap = keyboard_keymap,
    .enter = keyboard_enter,
    .leave = keyboard_leave,
    .key = keyboard_key,
    .modifiers = keyboard_modifiers,
    .repeat_info = keyboard_repeat_info,
};

static void seat_capabilities(void *data, struct wl_seat *seat, uint32_t caps) {
    IslandApp *app = data;
    if ((caps & WL_SEAT_CAPABILITY_POINTER) && !app->pointer) {
        app->pointer = wl_seat_get_pointer(seat);
        wl_pointer_add_listener(app->pointer, &pointer_listener, app);
    }
    if ((caps & WL_SEAT_CAPABILITY_KEYBOARD) && !app->keyboard) {
        app->keyboard = wl_seat_get_keyboard(seat);
        wl_keyboard_add_listener(app->keyboard, &keyboard_listener, app);
    }
    if ((caps & WL_SEAT_CAPABILITY_TOUCH) && !app->touch) {
        app->touch = wl_seat_get_touch(seat);
        wl_touch_add_listener(app->touch, &touch_listener, app);
    }
}

static void seat_name(void *d, struct wl_seat *s, const char *n) { (void)d; (void)s; (void)n; }

static const struct wl_seat_listener seat_listener = {
    .capabilities = seat_capabilities,
    .name = seat_name,
};

/* ========================================================================== */
/* Layer Surface Setup                                                        */
/* ========================================================================== */
static void layer_surface_configure(void *data, struct zwlr_layer_surface_v1 *ls,
                                    uint32_t serial, uint32_t width, uint32_t height) {
    IslandOutput *out = data;
    zwlr_layer_surface_v1_ack_configure(ls, serial);
    if (width > 0) out->width = (int32_t)width;
    if (height > 0) out->height = (int32_t)height;
    out->configured = true;
    render_output(out);
}

static void layer_surface_closed(void *data, struct zwlr_layer_surface_v1 *ls) {
    (void)data; (void)ls;
}

static const struct zwlr_layer_surface_v1_listener layer_surface_listener = {
    .configure = layer_surface_configure,
    .closed = layer_surface_closed,
};

static void setup_layer_surface(IslandOutput *out) {
    IslandApp *app = out->app;
    if (!app->layer_shell || !app->compositor || out->layer_surface) return;

    out->surface = wl_compositor_create_surface(app->compositor);
    out->layer_surface = zwlr_layer_shell_v1_get_layer_surface(
        app->layer_shell, out->surface, out->wl_output,
        ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY, "infynitae-island");

    zwlr_layer_surface_v1_set_anchor(
        out->layer_surface,
        ZWLR_LAYER_SURFACE_V1_ANCHOR_TOP |
        ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM |
        ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT |
        ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT);
    zwlr_layer_surface_v1_set_exclusive_zone(out->layer_surface, -1);
    zwlr_layer_surface_v1_set_keyboard_interactivity(
        out->layer_surface, ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE);
    zwlr_layer_surface_v1_add_listener(out->layer_surface, &layer_surface_listener, out);
    wl_surface_commit(out->surface);
}

static void registry_global(void *data, struct wl_registry *reg, uint32_t name,
                            const char *interface, uint32_t version) {
    (void)version;
    IslandApp *app = data;
    if (strcmp(interface, wl_compositor_interface.name) == 0) {
        app->compositor = wl_registry_bind(reg, name, &wl_compositor_interface, 4);
    } else if (strcmp(interface, wl_shm_interface.name) == 0) {
        app->shm = wl_registry_bind(reg, name, &wl_shm_interface, 1);
    } else if (strcmp(interface, zwlr_layer_shell_v1_interface.name) == 0) {
        app->layer_shell = wl_registry_bind(reg, name, &zwlr_layer_shell_v1_interface, 1);
    } else if (strcmp(interface, wl_seat_interface.name) == 0) {
        app->seat = wl_registry_bind(reg, name, &wl_seat_interface, 5);
        wl_seat_add_listener(app->seat, &seat_listener, app);
    } else if (strcmp(interface, wl_output_interface.name) == 0) {
        IslandOutput *out = calloc(1, sizeof(IslandOutput));
        out->app = app;
        out->registry_name = name;
        out->wl_output = wl_registry_bind(reg, name, &wl_output_interface, 4);
        out->scale = 1;
        out->next = app->outputs;
        app->outputs = out;
        setup_layer_surface(out);
    }
}

static void registry_global_remove(void *data, struct wl_registry *reg, uint32_t name) {
    (void)reg;
    IslandApp *app = data;
    IslandOutput **curr = &app->outputs;
    while (*curr) {
        if ((*curr)->registry_name == name) {
            IslandOutput *rm = *curr;
            *curr = rm->next;
            if (rm->frame_cb) wl_callback_destroy(rm->frame_cb);
            if (rm->layer_surface) zwlr_layer_surface_v1_destroy(rm->layer_surface);
            if (rm->surface) wl_surface_destroy(rm->surface);
            if (rm->buffer) wl_buffer_destroy(rm->buffer);
            if (rm->shm_data) munmap(rm->shm_data, rm->shm_size);
            if (rm->wl_output) wl_output_destroy(rm->wl_output);
            free(rm);
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
/* Main Entrypoint & Signal IPC                                               */
/* ========================================================================== */
int main(int argc, char **argv) {
    const char *pfile = pid_file_path();

    if (argc > 1) {
        FILE *pf = fopen(pfile, "r");
        pid_t running_pid = 0;
        if (pf) {
            if (fscanf(pf, "%d", &running_pid) != 1) running_pid = 0;
            fclose(pf);
        }
        if (strcmp(argv[1], "toggle") == 0 || strcmp(argv[1], "center") == 0) {
            if (running_pid > 0 && kill(running_pid, SIGUSR1) == 0) return 0;
        } else if (strcmp(argv[1], "control") == 0) {
            if (running_pid > 0 && kill(running_pid, SIGUSR2) == 0) return 0;
        } else if (strcmp(argv[1], "stop") == 0) {
            if (running_pid > 0) kill(running_pid, SIGTERM);
            return 0;
        }
    }

    /* Replace any older running instance */
    FILE *old_pf = fopen(pfile, "r");
    if (old_pf) {
        pid_t old_pid = 0;
        if (fscanf(old_pf, "%d", &old_pid) == 1 && old_pid > 0 && old_pid != getpid()) {
            kill(old_pid, SIGTERM);
            usleep(50000);
        }
        fclose(old_pf);
    }

    FILE *pf = fopen(pfile, "w");
    if (pf) {
        fprintf(pf, "%d\n", getpid());
        fclose(pf);
    }

    signal(SIGUSR1, on_sigusr1);
    signal(SIGUSR2, on_sigusr2);
    signal(SIGTERM, on_sigterm);
    signal(SIGINT, on_sigterm);
    signal(SIGCHLD, SIG_IGN);

    IslandApp app = {0};
    app.running = true;
    app.mode = MODE_CLOSED;
    app.target_mode = MODE_HYBRID;
    app.touch_id = -1;
    app.volume_pct = 75;
    app.battery_pct = -1;
    strcpy(app.power_profile, "balanced");
    strcpy(app.wifi_label, "On");

    load_position_config(&app);
    load_all_apps(&app);

    app.xkb_ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    app.display = wl_display_connect(NULL);
    if (!app.display) {
        fprintf(stderr, "[infynitae-island] Cannot connect to Wayland display\n");
        return 1;
    }

    app.registry = wl_display_get_registry(app.display);
    wl_registry_add_listener(app.registry, &registry_listener, &app);
    wl_display_roundtrip(app.display);
    for (IslandOutput *o = app.outputs; o; o = o->next) setup_layer_surface(o);
    wl_display_roundtrip(app.display);

    if (argc > 1 && (strcmp(argv[1], "toggle") == 0 || strcmp(argv[1], "open") == 0)) {
        set_mode(&app, MODE_HYBRID);
    } else if (argc > 1 && strcmp(argv[1], "control") == 0) {
        set_mode(&app, MODE_CONTROL);
    }

    while (app.running) {
        if (g_sig_cmd == 1) {
            g_sig_cmd = 0;
            set_mode(&app, app.mode == MODE_CLOSED ? MODE_HYBRID : MODE_CLOSED);
        } else if (g_sig_cmd == 2) {
            g_sig_cmd = 0;
            set_mode(&app, app.mode == MODE_CONTROL ? MODE_CLOSED : MODE_CONTROL);
        } else if (g_sig_cmd == 3) {
            break;
        }

        while (wl_display_prepare_read(app.display) != 0) {
            if (wl_display_dispatch_pending(app.display) < 0) {
                app.running = false;
                break;
            }
        }
        if (!app.running) break;
        wl_display_flush(app.display);

        struct pollfd pfd = {
            .fd = wl_display_get_fd(app.display),
            .events = POLLIN,
            .revents = 0
        };
        int timeout = (app.mode != MODE_CLOSED) ? 1000 : -1;
        int ret = poll(&pfd, 1, timeout);
        if (ret > 0 && (pfd.revents & POLLIN)) {
            if (wl_display_read_events(app.display) < 0 ||
                wl_display_dispatch_pending(app.display) < 0) {
                break;
            }
        } else {
            wl_display_cancel_read(app.display);
        }
    }

    unlink(pfile);
    wl_display_disconnect(app.display);
    return 0;
}
