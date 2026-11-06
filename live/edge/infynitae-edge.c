/*
 * Infynitae Native App Launcher (Super+W), Taskbar (Super+L) & System Control / StatusNotifier Tray (Super+A)
 *
 * Drawn in the theme colors from the config (CANVAS_COLOR, …): square corners,
 * 1px subtle border, zero nested buttons, zero visual bloat):
 *   - Position (`EDGE_POSITION="bottom|top"`, `EDGE_CONTROL_SIDE="right|left"`) is read
 *     strictly from `~/.config/infynitae/config` (no UI buttons for it).
 *   - Non-blocking input region: only the drawn bar/panel captures pointer events;
 *     clicking/panning the canvas works freely while open.
 *   - Super+Shift+C / Delete / Middle-Click closes the selected/hovered window (in Super+L)
 *     or quits the selected/hovered tray app (in Super+A).
 *   - Built-in org.kde.StatusNotifierWatcher + com.canonical.dbusmenu host:
 *     Right-clicking a tray app fetches and displays the app's own native context menu
 *     items (e.g. Steam, Discord, Telegram, OBS) inline, or falls back to Open / Quit / Kill.
 */

#define _GNU_SOURCE
#include <ctype.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <linux/input-event-codes.h>
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
#include <gio/gio.h>
#include <pango/pangocairo.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>

#include "app-search.h"
#include "config-value.h"
#include "child-signals.h"
#include "../wallpaper/wlr-layer-shell-protocol.h"
#include "wlr-foreign-toplevel-protocol.h"

#define MAX_WINDOWS 24
#define VISIBLE_APPS 8
#define MAX_CLIP_ITEMS 24
#define VISIBLE_CLIP_ITEMS 8
#define MAX_TRAY_APPS 8
#define MAX_SNI_ITEMS 16
#define MAX_CTX_ITEMS 10
#define MAX_SUB_ITEMS 8
#define MAX_NOTIFS 4
#define MAX_HITBOXES 64

typedef enum {
    MODE_CLOSED = 0,
    MODE_CENTER = 1,    /* Super+L: Search + Open Window Tabs, windows only */
    MODE_CONTROL = 2,   /* Super+A: System Control + Tray Apps */
    MODE_CLIPBOARD = 3, /* Super+V: Clipboard History */
    MODE_APPS = 4       /* Super+W: Search + Apps */
} EdgeMode;

typedef enum {
    SUB_NONE = 0,
    SUB_WIFI = 1,
    SUB_BT = 2,
    SUB_AUDIO = 3
} ControlSubpanel;

typedef struct {
    uint64_t inst;
    uint64_t elem;
    char app_id[64];
    char title[128];
    bool focused;
    bool attention; /* opened without the focus or refused an activation: Super+Tab goes there first */
} CanvasWindow;

typedef struct {
    char bus_name[128];
    char obj_path[128];
    char menu_path[128];
    char id[64];
    char title[64];
    uint32_t pid;
} SniItem;

typedef struct {
    char name[64];
    char proc_name[64];
    char mem[24];
    uint32_t pid;
    int sni_idx;      /* >= 0 if matched with a registered StatusNotifierItem */
} TrayApp;

typedef struct {
    int32_t id;       /* >= 0 for native dbusmenu item ID; -1=Open, -2=Quit, -3=Force Kill */
    char label[64];
} CtxMenuItem;

typedef struct {
    char id[128];
    char label[128];
    char meta[32];
    bool active;
} SubItem;

typedef struct {
    uint32_t id;
    char app[32];
    char text[128];
} NotifItem;

typedef struct {
    char raw_line[256];
    char preview[128];
} ClipItem;

typedef struct {
    double x, y, w, h;
    int action_id;
    int index;
    int nav_idx;
} HitBox;

struct EdgeApp;

typedef struct EdgeOutput {
    struct EdgeApp *app;
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

    int prev_dx, prev_dy, prev_dw, prev_dh;
    int input_x, input_y, input_w, input_h;

    /* Closed over a fullscreen window: no buffer at all, so the window's frames go to the
     * screen as they are (direct scanout, tearing) and the edge strips take no clicks */
    bool unmapped;

    struct EdgeOutput *next;
} EdgeOutput;

/* A toplevel as wlr-foreign-toplevel reports it: fullscreen, and on which outputs */
#define FS_MAX_OUTPUTS 8
typedef struct FsToplevel {
    struct zwlr_foreign_toplevel_handle_v1 *handle;
    bool fullscreen, pending_fullscreen;
    struct wl_output *outputs[FS_MAX_OUTPUTS];
    int n_outputs;
    struct FsToplevel *next;
} FsToplevel;

typedef struct EdgeApp {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct zwlr_layer_shell_v1 *layer_shell;
    struct zwlr_foreign_toplevel_manager_v1 *toplevel_manager;
    FsToplevel *toplevels;
    struct wl_seat *seat;
    struct wl_pointer *pointer;
    struct wl_keyboard *keyboard;
    struct wl_touch *touch;

    struct xkb_context *xkb_ctx;
    struct xkb_keymap *xkb_keymap;
    struct xkb_state *xkb_state;

    /* D-Bus StatusNotifierWatcher + dbusmenu host */
    GDBusConnection *dbus;
    guint sni_watcher_reg_id;
    guint sni_bus_owner_id;
    SniItem sni_items[MAX_SNI_ITEMS];
    int sni_count;

    EdgeOutput *outputs;

    EdgeMode mode;
    EdgeMode target_mode;
    ControlSubpanel subpanel;
    bool kb_grab;
    double anim_progress;
    /* CLOCK_MONOTONIC of the last animation step (0: none yet): the panel opens in OPEN_MS and
     * closes in CLOSE_MS whatever the refresh rate and however many screens ask for frames */
    int64_t anim_last_ns;
    /* which opening the notification list belongs to (fetch_mako_notifications) */
    unsigned notif_generation;
    bool dragging;
    int32_t touch_id;
    uint32_t touch_down_ms;
    double touch_start_x;
    double touch_start_y;
    double touch_cur_x;
    double touch_cur_y;

    double pointer_x;
    double pointer_y;
    EdgeOutput *pointer_output;

    char search[128];
    size_t search_len;
    int selected_idx;

    CanvasWindow windows[MAX_WINDOWS];
    int win_count;
    int filtered_wins[MAX_WINDOWS];
    int filtered_win_count;

    DesktopApp apps[MAX_APPS];
    int app_count;
    int filtered_apps[MAX_APPS];
    int filtered_app_count;
    RecentApps recent_apps;

    /* Tray & Background Apps + Native Context Menu */
    TrayApp tray_apps[MAX_TRAY_APPS];
    int tray_count;
    int tray_ctx_idx;
    CtxMenuItem ctx_items[MAX_CTX_ITEMS];
    int ctx_count;

    SubItem sub_items[MAX_SUB_ITEMS];
    int sub_count;

    NotifItem notifs[MAX_NOTIFS];
    int notif_count;
    uint32_t cleared_notif_id;
    uint32_t max_notif_id;

    ClipItem clip_items[MAX_CLIP_ITEMS];
    int clip_count;
    int filtered_clips[MAX_CLIP_ITEMS];
    int filtered_clip_count;

    /* Config & Status state */
    bool bottom_edge;
    bool control_on_left;
    double surface_rgb[3];
    double border_rgb[3];
    double text_rgb[3];
    /* BORDER_INACTIVE: every panel outline and divider (the accent, border_rgb, only marks what is
     * selected or on); SQUARE_CORNERS: no rounding anywhere */
    double hair_rgb[3];
    bool hair_set;
    bool square;
    /* the first row shown of a list longer than LIST_ROWS (launcher, windows, clipboard) */
    int list_scroll;
    /* Restart / Shut down pressed once: the action and until when a second press does it */
    int power_confirm_action;
    int64_t power_confirm_until;
    bool wifi_on;
    char wifi_label[48];
    bool bt_on;
    char bt_label[48];
    bool dnd_on;
    bool replay_on;
    int volume_pct;
    bool volume_muted;
    char power_profile[32];
    int battery_pct;
    bool battery_charging;

    HitBox hitboxes[MAX_HITBOXES];
    int hitbox_count;

    int async_fd;
    pid_t async_pid;
    int async_kind; /* 1=snapshot, 2=control, 3=subpanel */
    char async_buf[65536];
    size_t async_len;

    bool running;
} EdgeApp;

enum {
    ACT_NONE = 0,
    ACT_WIN_ITEM,
    ACT_APP_LAUNCH,
    ACT_WIFI_TOGGLE,
    ACT_BT_TOGGLE,
    ACT_DND_TOGGLE,
    ACT_POWER_PROFILE,
    ACT_REPLAY_TOGGLE,
    ACT_REPLAY_SAVE,
    ACT_NOTIF_ITEM,
    ACT_NOTIF_CLEAR,
    ACT_SUB_SELECT,
    ACT_TRAY_ITEM,
    ACT_TRAY_CTX_CLICK,
    ACT_VOL_DOWN,
    ACT_VOL_UP,
    ACT_VOL_MUTE,
    ACT_VOL_BAR,
    ACT_REBOOT,
    ACT_POWEROFF,
    ACT_LOGOUT,
    ACT_CLIP_ITEM,
    ACT_CLIP_CLEAR,
    ACT_CENTER_EDGE,
    ACT_CORNER_EDGE,
    ACT_SEARCH_BOX    /* a click there keeps the panel open */
};

static volatile sig_atomic_t g_sig_cmd = 0;

/* The signals (Super+W, Super+L, Super+A, Super+V) reach any thread that does not block them, GLib's own
 * (gdbus, pool) as well: a byte in this pipe wakes the main loop's poll whichever thread ran the
 * handler, where the flag alone waited for the next unrelated event (the keys did nothing). */
static int g_wake_pipe[2] = {-1, -1};
static void wake_main(void) {
    if (g_wake_pipe[1] >= 0) {
        int saved = errno;
        ssize_t w = write(g_wake_pipe[1], "x", 1);
        (void)w;
        errno = saved;
    }
}
static void on_sigusr1(int sig) { (void)sig; g_sig_cmd = 1; wake_main(); }
static void on_sigusr2(int sig) { (void)sig; g_sig_cmd = 2; wake_main(); }
static void on_sigterm(int sig) { (void)sig; g_sig_cmd = 3; wake_main(); }
static void on_sigrtmin(int sig) { (void)sig; g_sig_cmd = 4; wake_main(); }
static void on_sigapps(int sig) { (void)sig; g_sig_cmd = 5; wake_main(); }

static void request_frame(EdgeOutput *out);
static void damage_all(EdgeApp *app);
static void set_mode(EdgeApp *app, EdgeMode new_mode);

/* ========================================================================== */
/* Helpers                                                                    */
/* ========================================================================== */
static const char *pid_file_path(void) {
    static char path[512];
    const char *rt = getenv("XDG_RUNTIME_DIR");
    if (!rt) rt = "/tmp";
    snprintf(path, sizeof(path), "%s/infynitae-edge.pid", rt);
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

/* `script` run by sh with `a1` and `a2` as "$1" and "$2": a name from outside (a Wi-Fi network's,
 * a Bluetooth device's, an app's) is never read as shell code, whatever quotes it holds. */
/* A tray app's window, opened again: its program by its name (lowercased), else its desktop file */
/* "Open" on a background app: its name is only a process's short name, which the process chose,
 * so it is never run as a command. A name with a slash (or that looks like an option) is refused;
 * otherwise the app opens through its desktop file: the one with that id, else the one whose
 * Exec program has that name. */
#define TRAY_LAUNCH_SCRIPT \
    "app=$(printf '%s' \"$1\" | tr '[:upper:]' '[:lower:]'); " \
    "case \"$app\" in ''|*/*|-*|.*) exit 0;; esac; " \
    "gtk-launch \"$app\" 2>/dev/null && exit 0; " \
    "IFS=:; for d in \"${XDG_DATA_HOME:-$HOME/.local/share}\" ${XDG_DATA_DIRS:-/usr/local/share:/usr/share}; do " \
    "for f in \"$d\"/applications/*.desktop; do [ -f \"$f\" ] || continue; " \
    "e=$(sed -n 's/^Exec=//p' \"$f\" | head -n1); e=${e%% *}; e=${e##*/}; " \
    "if [ \"$(printf '%s' \"$e\" | tr '[:upper:]' '[:lower:]')\" = \"$app\" ]; then " \
    "id=${f##*/}; exec gtk-launch \"${id%.desktop}\"; fi; done; done"

static void spawn_script(const char *script, const char *a1, const char *a2) {
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
        execl("/bin/sh", "sh", "-c", script, "sh", a1 ? a1 : "", a2 ? a2 : "", (char *)NULL);
        _exit(127);
    }
}

/* ========================================================================== */
/* StatusNotifierWatcher + com.canonical.dbusmenu Host                        */
/* ========================================================================== */
static const gchar sni_watcher_xml[] =
    "<node>"
    "  <interface name='org.kde.StatusNotifierWatcher'>"
    "    <method name='RegisterStatusNotifierItem'>"
    "      <arg type='s' name='service' direction='in'/>"
    "    </method>"
    "    <method name='RegisterStatusNotifierHost'>"
    "      <arg type='s' name='service' direction='in'/>"
    "    </method>"
    "    <property name='RegisteredStatusNotifierItems' type='as' access='read'/>"
    "    <property name='IsStatusNotifierHostRegistered' type='b' access='read'/>"
    "    <property name='ProtocolVersion' type='i' access='read'/>"
    "    <signal name='StatusNotifierItemRegistered'>"
    "      <arg type='s' name='service'/>"
    "    </signal>"
    "    <signal name='StatusNotifierItemUnregistered'>"
    "      <arg type='s' name='service'/>"
    "    </signal>"
    "    <signal name='StatusNotifierHostRegistered'/>"
    "  </interface>"
    "</node>";

static void sni_fetch_item_props(EdgeApp *app, SniItem *item) {
    if (!app->dbus) return;

    /* Get PID of the D-Bus sender so we can match it to tray_apps */
    GVariant *pid_res = g_dbus_connection_call_sync(
        app->dbus, "org.freedesktop.DBus", "/org/freedesktop/DBus",
        "org.freedesktop.DBus", "GetConnectionUnixProcessID",
        g_variant_new("(s)", item->bus_name), G_VARIANT_TYPE("(u)"),
        G_DBUS_CALL_FLAGS_NONE, 150, NULL, NULL);
    if (pid_res) {
        g_variant_get(pid_res, "(u)", &item->pid);
        g_variant_unref(pid_res);
    }

    const char *ifaces[2] = { "org.kde.StatusNotifierItem", "org.ayatana.StatusNotifierItem" };
    for (int i = 0; i < 2; i++) {
        GVariant *res = g_dbus_connection_call_sync(
            app->dbus, item->bus_name, item->obj_path,
            "org.freedesktop.DBus.Properties", "GetAll",
            g_variant_new("(s)", ifaces[i]), G_VARIANT_TYPE("(a{sv})"),
            G_DBUS_CALL_FLAGS_NONE, 150, NULL, NULL);
        if (!res) continue;

        GVariant *dict = g_variant_get_child_value(res, 0);
        const gchar *s_val = NULL;
        if (g_variant_lookup(dict, "Id", "&s", &s_val) && s_val)
            snprintf(item->id, sizeof(item->id), "%s", s_val);
        if (g_variant_lookup(dict, "Title", "&s", &s_val) && s_val)
            snprintf(item->title, sizeof(item->title), "%s", s_val);
        if (g_variant_lookup(dict, "Menu", "&o", &s_val) && s_val)
            snprintf(item->menu_path, sizeof(item->menu_path), "%s", s_val);
        g_variant_unref(dict);
        g_variant_unref(res);
        if (item->menu_path[0] != '\0' || item->id[0] != '\0') break;
    }
}

static void sni_handle_method_call(GDBusConnection *conn, const gchar *sender,
                                   const gchar *object_path, const gchar *interface_name,
                                   const gchar *method_name, GVariant *parameters,
                                   GDBusMethodInvocation *invocation, gpointer user_data) {
    (void)object_path; (void)interface_name;
    EdgeApp *app = user_data;

    if (g_strcmp0(method_name, "RegisterStatusNotifierItem") == 0) {
        const gchar *service = NULL;
        g_variant_get(parameters, "(&s)", &service);
        if (service && app->sni_count < MAX_SNI_ITEMS) {
            char bus_name[128] = {0};
            char obj_path[128] = {0};
            if (service[0] == '/') {
                snprintf(bus_name, sizeof(bus_name), "%s", sender ? sender : "");
                snprintf(obj_path, sizeof(obj_path), "%s", service);
            } else {
                snprintf(bus_name, sizeof(bus_name), "%s", service);
                strcpy(obj_path, "/StatusNotifierItem");
            }
            if (bus_name[0] != '\0') {
                int idx = -1;
                for (int i = 0; i < app->sni_count; i++) {
                    if (strcmp(app->sni_items[i].bus_name, bus_name) == 0 &&
                        strcmp(app->sni_items[i].obj_path, obj_path) == 0) {
                        idx = i;
                        break;
                    }
                }
                if (idx < 0) idx = app->sni_count++;
                SniItem *it = &app->sni_items[idx];
                memset(it, 0, sizeof(*it));
                snprintf(it->bus_name, sizeof(it->bus_name), "%s", bus_name);
                snprintf(it->obj_path, sizeof(it->obj_path), "%s", obj_path);
                sni_fetch_item_props(app, it);

                char full_item[260];
                snprintf(full_item, sizeof(full_item), "%s%s", bus_name, obj_path);
                g_dbus_connection_emit_signal(
                    conn, NULL, "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher",
                    "StatusNotifierItemRegistered", g_variant_new("(s)", full_item), NULL);
            }
        }
        g_dbus_method_invocation_return_value(invocation, NULL);
    } else if (g_strcmp0(method_name, "RegisterStatusNotifierHost") == 0) {
        g_dbus_method_invocation_return_value(invocation, NULL);
    } else {
        g_dbus_method_invocation_return_dbus_error(invocation, "org.freedesktop.DBus.Error.UnknownMethod", "Unknown method");
    }
}

static GVariant *sni_handle_get_property(GDBusConnection *conn, const gchar *sender,
                                         const gchar *object_path, const gchar *interface_name,
                                         const gchar *property_name, GError **error,
                                         gpointer user_data) {
    (void)conn; (void)sender; (void)object_path; (void)interface_name; (void)error;
    EdgeApp *app = user_data;

    if (g_strcmp0(property_name, "RegisteredStatusNotifierItems") == 0) {
        GVariantBuilder b;
        g_variant_builder_init(&b, G_VARIANT_TYPE("as"));
        for (int i = 0; i < app->sni_count; i++) {
            char full[260];
            snprintf(full, sizeof(full), "%s%s", app->sni_items[i].bus_name, app->sni_items[i].obj_path);
            g_variant_builder_add(&b, "s", full);
        }
        return g_variant_builder_end(&b);
    }
    if (g_strcmp0(property_name, "IsStatusNotifierHostRegistered") == 0) {
        return g_variant_new_boolean(TRUE);
    }
    if (g_strcmp0(property_name, "ProtocolVersion") == 0) {
        return g_variant_new_int32(0);
    }
    return NULL;
}

static const GDBusInterfaceVTable sni_vtable = {
    .method_call = sni_handle_method_call,
    .get_property = sni_handle_get_property,
    .set_property = NULL,
};

static void setup_sni_watcher(EdgeApp *app) {
    app->dbus = g_bus_get_sync(G_BUS_TYPE_SESSION, NULL, NULL);
    if (!app->dbus) return;

    GDBusNodeInfo *node = g_dbus_node_info_new_for_xml(sni_watcher_xml, NULL);
    if (!node) return;

    app->sni_watcher_reg_id = g_dbus_connection_register_object(
        app->dbus, "/StatusNotifierWatcher", node->interfaces[0],
        &sni_vtable, app, NULL, NULL);
    g_dbus_node_info_unref(node);

    app->sni_bus_owner_id = g_bus_own_name_on_connection(
        app->dbus, "org.kde.StatusNotifierWatcher",
        G_BUS_NAME_OWNER_FLAGS_REPLACE, NULL, NULL, NULL, NULL);

    g_dbus_connection_emit_signal(
        app->dbus, NULL, "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher",
        "StatusNotifierHostRegistered", NULL, NULL);
}

static int find_sni_for_tray_app(EdgeApp *app, const TrayApp *ta) {
    const char *key = ta->proc_name[0] ? ta->proc_name : ta->name;
    for (int i = 0; i < app->sni_count; i++) {
        SniItem *s = &app->sni_items[i];
        if (s->pid > 0 && ta->pid > 0 && s->pid == ta->pid) return i;
        if (s->id[0] && strcasestr(s->id, key)) return i;
        if (s->title[0] && strcasestr(s->title, key)) return i;
        if (s->id[0] && strcasestr(key, s->id)) return i;
    }
    return -1;
}

/* Strip mnemonic underscores ("_Mute" -> "Mute") */
static void clean_menu_label(char *dst, size_t dst_sz, const char *src) {
    size_t j = 0;
    for (size_t i = 0; src[i] && j + 1 < dst_sz; i++) {
        if (src[i] == '_' && src[i + 1] != '_') continue;
        dst[j++] = src[i];
    }
    dst[j] = '\0';
}

/* Populate ctx_items: first from the app's own com.canonical.dbusmenu, then fallback Open/Quit/Kill */
static void build_tray_context_menu(EdgeApp *app, int tidx) {
    app->ctx_count = 0;
    if (tidx < 0 || tidx >= app->tray_count) return;
    TrayApp *ta = &app->tray_apps[tidx];
    ta->sni_idx = find_sni_for_tray_app(app, ta);

    if (ta->sni_idx >= 0 && app->dbus) {
        SniItem *sni = &app->sni_items[ta->sni_idx];
        if (sni->menu_path[0] != '\0') {
            g_dbus_connection_call_sync(
                app->dbus, sni->bus_name, sni->menu_path,
                "com.canonical.dbusmenu", "AboutToShow",
                g_variant_new("(i)", 0), NULL,
                G_DBUS_CALL_FLAGS_NONE, 120, NULL, NULL);

            const gchar *prop_names[] = { "label", "visible", "enabled", "type", NULL };
            GVariant *res = g_dbus_connection_call_sync(
                app->dbus, sni->bus_name, sni->menu_path,
                "com.canonical.dbusmenu", "GetLayout",
                g_variant_new("(ii^as)", 0, 1, prop_names),
                G_VARIANT_TYPE("(u(ia{sv}av))"),
                G_DBUS_CALL_FLAGS_NONE, 180, NULL, NULL);

            if (res) {
                GVariant *layout = g_variant_get_child_value(res, 1);
                GVariant *children = g_variant_get_child_value(layout, 2);
                GVariantIter iter;
                GVariant *child_v = NULL;
                g_variant_iter_init(&iter, children);
                while (g_variant_iter_next(&iter, "v", &child_v) && app->ctx_count < MAX_CTX_ITEMS - 1) {
                    int32_t cid = 0;
                    GVariant *cprops = g_variant_get_child_value(child_v, 1);
                    g_variant_get_child(child_v, 0, "i", &cid);

                    gboolean visible = TRUE, enabled = TRUE;
                    const gchar *type_s = NULL, *label_s = NULL;
                    g_variant_lookup(cprops, "visible", "b", &visible);
                    g_variant_lookup(cprops, "enabled", "b", &enabled);
                    g_variant_lookup(cprops, "type", "&s", &type_s);
                    g_variant_lookup(cprops, "label", "&s", &label_s);

                    if (visible && enabled && (!type_s || strcmp(type_s, "separator") != 0) &&
                        label_s && label_s[0] != '\0') {
                        CtxMenuItem *cm = &app->ctx_items[app->ctx_count++];
                        cm->id = cid;
                        clean_menu_label(cm->label, sizeof(cm->label), label_s);
                    }
                    g_variant_unref(cprops);
                    g_variant_unref(child_v);
                }
                g_variant_unref(children);
                g_variant_unref(layout);
                g_variant_unref(res);
            }
        }
    }

    /* Always provide clean fallback actions if the app has no dbusmenu */
    if (app->ctx_count == 0) {
        app->ctx_items[0].id = -1; strcpy(app->ctx_items[0].label, "Open");
        app->ctx_items[1].id = -2; strcpy(app->ctx_items[1].label, "Quit");
        app->ctx_items[2].id = -3; strcpy(app->ctx_items[2].label, "Force Kill");
        app->ctx_count = 3;
    }
}

static void trigger_dbusmenu_click(EdgeApp *app, int sni_idx, int32_t item_id) {
    if (!app->dbus || sni_idx < 0 || sni_idx >= app->sni_count) return;
    SniItem *sni = &app->sni_items[sni_idx];
    if (sni->menu_path[0] == '\0') return;
    g_dbus_connection_call(
        app->dbus, sni->bus_name, sni->menu_path,
        "com.canonical.dbusmenu", "Event",
        g_variant_new("(isvu)", item_id, "clicked", g_variant_new_int32(0), (guint32)time(NULL)),
        NULL, G_DBUS_CALL_FLAGS_NONE, 200, NULL, NULL, NULL);
}

/* ========================================================================== */
/* Data Collectors                                                            */
/* ========================================================================== */
static void load_all_apps(EdgeApp *app) {
    app->app_count = app_load_all(app->apps, MAX_APPS);
    recent_apps_load(&app->recent_apps);
    recent_apps_apply(&app->recent_apps, app->apps, app->app_count);
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

static void update_filters(EdgeApp *app) {
    app->filtered_win_count = 0;
    for (int i = 0; i < app->win_count; i++) {
        if (str_contains_ci(app->windows[i].app_id, app->search) ||
            str_contains_ci(app->windows[i].title, app->search)) {
            app->filtered_wins[app->filtered_win_count++] = i;
        }
    }
    app->filtered_app_count = app_search(app->apps, app->app_count, app->search, app->filtered_apps, MAX_APPS);
    app->filtered_clip_count = 0;
    for (int i = 0; i < app->clip_count; i++) {
        if (str_contains_ci(app->clip_items[i].preview, app->search)) {
            app->filtered_clips[app->filtered_clip_count++] = i;
        }
    }
    if (app->mode == MODE_CLIPBOARD || app->target_mode == MODE_CLIPBOARD) {
        int shown = app->filtered_clip_count;
        if (shown <= 0) app->selected_idx = 0;
        else if (app->selected_idx >= shown) app->selected_idx = shown - 1;
        return;
    }
    int total = app->mode == MODE_APPS ? app->filtered_app_count : app->filtered_win_count;
    if (total <= 0) app->selected_idx = 0;
    else if (app->selected_idx >= total) app->selected_idx = total - 1;
}

static void parse_snapshot_buf(EdgeApp *app, const char *buf) {
    app->win_count = 0;
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
                /* the window wants attention (owner, 2026-10-31): its element says so */
                const char *att_pos = strstr(id_pos, "\"attention\":true");
                w->attention = att_pos && (!next_obj || att_pos < next_obj);
                if (w->app_id[0] == '\0') strcpy(w->app_id, "Window");
                if (w->title[0] == '\0') strcpy(w->title, w->app_id);
                app->win_count++;
            }
        }
        p = id_pos + 6;
    }
    update_filters(app);
}

static void format_pretty_app_name(char *dst, size_t dst_sz, const char *raw) {
    if (!raw || !*raw) {
        dst[0] = '\0';
        return;
    }
    if (strcasecmp(raw, "rustdesk") == 0) {
        snprintf(dst, dst_sz, "RustDesk");
        return;
    }
    if (strcasecmp(raw, "obs") == 0 || strcasecmp(raw, "obs-studio") == 0) {
        snprintf(dst, dst_sz, "OBS");
        return;
    }
    if (strcasecmp(raw, "telegram-desktop") == 0) {
        snprintf(dst, dst_sz, "Telegram");
        return;
    }
    snprintf(dst, dst_sz, "%s", raw);
    bool cap_next = true;
    for (size_t i = 0; dst[i]; i++) {
        if (dst[i] == '-' || dst[i] == '_') {
            cap_next = true;
        } else if (cap_next && isalpha((unsigned char)dst[i])) {
            dst[i] = (char)toupper((unsigned char)dst[i]);
            cap_next = false;
        } else {
            cap_next = false;
        }
    }
}

static void parse_control_buf(EdgeApp *app, char *buf) {
    app->tray_count = 0;
    char *saveptr = NULL;
    for (char *line = strtok_r(buf, "\n", &saveptr); line; line = strtok_r(NULL, "\n", &saveptr)) {
        size_t len = strlen(line);
        while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r')) line[--len] = '\0';

        if (strncmp(line, "SYS\t", 4) == 0) {
            char *p = line + 4;
            char *fields[7] = {0};
            for (int i = 0; i < 7; i++) {
                fields[i] = p;
                char *tab = strchr(p, '\t');
                if (tab) {
                    *tab = '\0';
                    p = tab + 1;
                } else {
                    p += strlen(p);
                }
            }
            app->wifi_on = (strcmp(fields[0], "enabled") == 0);
            if (app->wifi_on) {
                if (fields[1][0] != '\0') snprintf(app->wifi_label, sizeof(app->wifi_label), "%s", fields[1]);
                else strcpy(app->wifi_label, "On");
            } else {
                strcpy(app->wifi_label, "Off");
            }
            app->bt_on = (strcmp(fields[2], "on") == 0);
            if (app->bt_on) {
                if (fields[3][0] != '\0') snprintf(app->bt_label, sizeof(app->bt_label), "%s", fields[3]);
                else strcpy(app->bt_label, "On");
            } else {
                strcpy(app->bt_label, "Off");
            }
            app->volume_pct = atoi(fields[4]);
            if (app->volume_pct < 0) app->volume_pct = 0;
            if (app->volume_pct > 100) app->volume_pct = 100;
            app->volume_muted = (strcmp(fields[5], "yes") == 0);
            if (fields[6][0] != '\0') snprintf(app->power_profile, sizeof(app->power_profile), "%s", fields[6]);
        } else if (app->tray_count < MAX_TRAY_APPS) {
            char *p = line;
            char *t1 = strchr(p, '\t');
            if (!t1) continue;
            *t1 = '\0';
            char *f_name = t1 + 1;
            char *t2 = strchr(f_name, '\t');
            if (!t2) continue;
            *t2 = '\0';
            char *f_mem = t2 + 1;
            char *t3 = strchr(f_mem, '\t');
            uint32_t pid = 0;
            if (t3) {
                *t3 = '\0';
                pid = (uint32_t)atoi(t3 + 1);
            }

            TrayApp *ta = &app->tray_apps[app->tray_count++];
            snprintf(ta->proc_name, sizeof(ta->proc_name), "%s", f_name);
            format_pretty_app_name(ta->name, sizeof(ta->name), f_name);
            snprintf(ta->mem, sizeof(ta->mem), "%s", f_mem);
            ta->pid = pid;
            ta->sni_idx = find_sni_for_tray_app(app, ta);
        }
    }
    if (app->tray_ctx_idx >= app->tray_count) app->tray_ctx_idx = -1;
}

static void parse_subpanel_buf(EdgeApp *app, char *buf) {
    app->sub_count = 0;
    char *saveptr = NULL;
    for (char *line = strtok_r(buf, "\n", &saveptr);
         line && app->sub_count < MAX_SUB_ITEMS;
         line = strtok_r(NULL, "\n", &saveptr)) {
        size_t len = strlen(line);
        while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r')) line[--len] = '\0';
        char *f_act = line;
        char *t1 = strchr(f_act, '\t');
        if (!t1) continue;
        *t1 = '\0';
        char *f_id = t1 + 1;
        char *t2 = strchr(f_id, '\t');
        if (!t2) continue;
        *t2 = '\0';
        char *f_lbl = t2 + 1;
        char *t3 = strchr(f_lbl, '\t');
        if (!t3) continue;
        *t3 = '\0';
        char *f_meta = t3 + 1;

        SubItem *si = &app->sub_items[app->sub_count++];
        si->active = (atoi(f_act) != 0);
        snprintf(si->id, sizeof(si->id), "%s", f_id);
        snprintf(si->label, sizeof(si->label), "%s", f_lbl);
        snprintf(si->meta, sizeof(si->meta), "%s", f_meta);
    }
}

static void sanitize_inline_text(char *dst, size_t dst_sz, const char *src) {
    char tmp[256];
    size_t j = 0;
    bool prev_space = true;
    for (size_t i = 0; src && src[i] && j + 1 < sizeof(tmp); i++) {
        unsigned char c = (unsigned char)src[i];
        if (c == '\n' || c == '\r' || c == '\t' || c == ' ') {
            if (!prev_space) {
                tmp[j++] = ' ';
                prev_space = true;
            }
        } else if (c >= 32) {
            tmp[j++] = (char)c;
            prev_space = false;
        }
    }
    while (j > 0 && tmp[j - 1] == ' ') j--;
    tmp[j] = '\0';

    const char *home = getenv("HOME");
    size_t hlen = (home && *home) ? strlen(home) : 0;
    size_t out = 0;
    for (size_t i = 0; tmp[i] && out + 1 < dst_sz;) {
        if (hlen > 1 && strncmp(tmp + i, home, hlen) == 0 &&
            (tmp[i + hlen] == '/' || tmp[i + hlen] == '\0' || tmp[i + hlen] == ' ')) {
            dst[out++] = '~';
            i += hlen;
        } else {
            dst[out++] = tmp[i++];
        }
    }
    dst[out] = '\0';
}

static void parse_clipboard_buf(EdgeApp *app, char *buf) {
    app->clip_count = 0;
    char *saveptr = NULL;
    for (char *line = strtok_r(buf, "\n", &saveptr);
         line && app->clip_count < MAX_CLIP_ITEMS;
         line = strtok_r(NULL, "\n", &saveptr)) {
        size_t len = strlen(line);
        while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r')) line[--len] = '\0';
        if (len == 0) continue;

        ClipItem *ci = &app->clip_items[app->clip_count++];
        snprintf(ci->raw_line, sizeof(ci->raw_line), "%s", line);
        const char *tab = strchr(line, '\t');
        const char *body = tab ? (tab + 1) : line;
        sanitize_inline_text(ci->preview, sizeof(ci->preview), body);
        if (ci->preview[0] == '\0') {
            snprintf(ci->preview, sizeof(ci->preview), "(Clipboard item)");
        }
    }
    update_filters(app);
}

/* One mako list (ListNotifications or ListHistory) into the panel's notifications. */
static void add_mako_list(EdgeApp *app, GVariant *res) {
    GVariant *arr = g_variant_get_child_value(res, 0);
    GVariantIter iter;
    GVariant *dict = NULL;
    g_variant_iter_init(&iter, arr);
    while ((dict = g_variant_iter_next_value(&iter)) != NULL) {
        guint32 nid = 0;
        const gchar *app_s = NULL, *sum_s = NULL, *body_s = NULL;
        g_variant_lookup(dict, "id", "u", &nid);
        g_variant_lookup(dict, "app-name", "&s", &app_s);
        g_variant_lookup(dict, "summary", "&s", &sum_s);
        g_variant_lookup(dict, "body", "&s", &body_s);

        if (nid > app->max_notif_id) app->max_notif_id = nid;

        bool dup = false;
        for (int k = 0; k < app->notif_count; k++) {
            if (app->notifs[k].id == nid && nid != 0) {
                dup = true;
                break;
            }
        }

        if (!dup && nid > app->cleared_notif_id && app->notif_count < MAX_NOTIFS) {
            char s_clean[96] = {0}, b_clean[96] = {0}, a_clean[32] = {0};
            sanitize_inline_text(s_clean, sizeof(s_clean), sum_s);
            sanitize_inline_text(b_clean, sizeof(b_clean), body_s);
            sanitize_inline_text(a_clean, sizeof(a_clean), app_s);

            if (s_clean[0] != '\0' || b_clean[0] != '\0') {
                char full_app[32], full_text[128];
                snprintf(full_app, sizeof(full_app), "%s", a_clean[0] ? a_clean : "Notify");
                if (s_clean[0] && b_clean[0] && strcasecmp(s_clean, b_clean) != 0) {
                    snprintf(full_text, sizeof(full_text), "%s — %s", s_clean, b_clean);
                } else {
                    snprintf(full_text, sizeof(full_text), "%s", s_clean[0] ? s_clean : b_clean);
                }

                bool text_dup = false;
                for (int k = 0; k < app->notif_count; k++) {
                    if (strcmp(app->notifs[k].app, full_app) == 0 &&
                        strcmp(app->notifs[k].text, full_text) == 0) {
                        text_dup = true;
                        break;
                    }
                }

                if (!text_dup) {
                    NotifItem *ni = &app->notifs[app->notif_count++];
                    ni->id = nid;
                    snprintf(ni->app, sizeof(ni->app), "%s", full_app);
                    snprintf(ni->text, sizeof(ni->text), "%s", full_text);
                }
            }
        }
        g_variant_unref(dict);
    }
    g_variant_unref(arr);
    g_variant_unref(res);
}

typedef struct {
    EdgeApp *app;
    unsigned generation;
} MakoCall;

static void on_mako_list(GObject *source, GAsyncResult *result, gpointer data) {
    MakoCall *call = data;
    GVariant *res = g_dbus_connection_call_finish(G_DBUS_CONNECTION(source), result, NULL);
    EdgeApp *app = call->app;
    /* a reply for an earlier opening is dropped: the list was started again since */
    if (res && call->generation == app->notif_generation) {
        add_mako_list(app, res);
        damage_all(app);
    } else if (res) {
        g_variant_unref(res);
    }
    g_free(call);
}

/* The panel's notifications from mako, without waiting: the panel opens at once and the list
 * fills in when mako answers (until 2026-11-06 two synchronous calls of up to 120 ms each ran
 * before the first frame). */
static void fetch_mako_notifications(EdgeApp *app) {
    app->notif_count = 0;
    app->notif_generation++;
    if (!app->dbus) return;

    const char *methods[2] = { "ListNotifications", "ListHistory" };
    for (int m = 0; m < 2; m++) {
        MakoCall *call = g_new0(MakoCall, 1);
        call->app = app;
        call->generation = app->notif_generation;
        g_dbus_connection_call(
            app->dbus, "org.freedesktop.Notifications", "/fr/emersion/Mako",
            "fr.emersion.Mako", methods[m], NULL, G_VARIANT_TYPE("(aa{sv})"),
            G_DBUS_CALL_FLAGS_NONE, 500, NULL, on_mako_list, call);
    }
}

static bool check_pid_file_alive(const char *path) {
    FILE *f = fopen(path, "r");
    if (!f) return false;
    int pid = 0;
    bool ok = (fscanf(f, "%d", &pid) == 1 && pid > 0 && kill(pid, 0) == 0);
    fclose(f);
    return ok;
}

static void start_async_refresh(EdgeApp *app, int kind) {
    if (kind == MODE_CONTROL) {
        const char *rt = getenv("XDG_RUNTIME_DIR");
        if (!rt) rt = "/tmp";
        char dnd_path[256], sup_pid[256], rep_pid[256];
        snprintf(dnd_path, sizeof(dnd_path), "%s/infynitae/do-not-disturb", rt);
        snprintf(sup_pid, sizeof(sup_pid), "%s/infynitae-recorder-supervisor.pid", rt);
        snprintf(rep_pid, sizeof(rep_pid), "%s/infynitae-replay.pid", rt);
        app->dnd_on = (access(dnd_path, F_OK) == 0);
        app->replay_on = check_pid_file_alive(sup_pid) || check_pid_file_alive(rep_pid);
        fetch_mako_notifications(app);

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
                        app->battery_charging = (strncmp(st, "Charging", 8) == 0 || strncmp(st, "Full", 4) == 0);
                    }
                    fclose(sf);
                }
                break;
            }
        }
    }

    if (app->async_fd >= 0) {
        close(app->async_fd);
        app->async_fd = -1;
    }

    int pfd[2];
    if (pipe(pfd) < 0) return;

    pid_t pid = fork();
    if (pid < 0) {
        close(pfd[0]);
        close(pfd[1]);
        return;
    }
    if (pid == 0) {
        close(pfd[0]);
        dup2(pfd[1], STDOUT_FILENO);
        int devnull = open("/dev/null", O_RDWR);
        if (devnull >= 0) {
            dup2(devnull, STDIN_FILENO);
            dup2(devnull, STDERR_FILENO);
            if (devnull > 2) close(devnull);
        }
        if (pfd[1] > 2) close(pfd[1]);

        if (kind == MODE_CENTER) {
            execl("/bin/sh", "sh", "-c", "infynitae msg snapshot 2>/dev/null", (char *)NULL);
        } else if (kind == MODE_CONTROL) {
            execl("/bin/sh", "sh", "-c",
                  "ssid=$(nmcli -t -f active,ssid dev wifi 2>/dev/null | sed -n 's/^yes://p' | head -1 | tr '\\t\\r' '  '); "
                  "wradio=$(nmcli radio wifi 2>/dev/null); "
                  "bt=$(bluetoothctl show 2>/dev/null | grep -q 'Powered: yes' && echo on || echo off); "
                  "btdev=$( [ \"$bt\" = on ] && bluetoothctl devices Connected 2>/dev/null | head -1 | cut -d' ' -f3- ); "
                  "vol=$(LC_ALL=C pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null | grep -o '[0-9]*%' | head -1 | tr -d '%'); "
                  "mute=$(LC_ALL=C pactl get-sink-mute @DEFAULT_SINK@ 2>/dev/null | awk '{print $2}'); "
                  "pp=$(powerprofilesctl get 2>/dev/null || echo none); "
                  "printf 'SYS\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s\\n' \"${wradio:-disabled}\" \"${ssid:-}\" \"$bt\" \"${btdev:-}\" \"${vol:-75}\" \"${mute:-no}\" \"$pp\"; "
                  "sh \"$HOME/infynitae/live/control-menu\" apps-list 2>/dev/null | head -n 6",
                  (char *)NULL);
        } else if (kind == 3) {
            if (app->subpanel == SUB_WIFI) {
                execl("/bin/sh", "sh", "-c",
                      "nmcli -t -f IN-USE,SIGNAL,SECURITY,SSID dev wifi list --rescan no 2>/dev/null "
                      "| sed 's/\\\\:/\\x1f/g' "
                      /* a network's name is the access point's: a tab in it is carried as \036 in the id (the
                       * connect script turns it back) and shown as a space, so it never adds a field */
                      "| awk -F: '$4 != \"\" { gsub(\"\\x1f\", \":\", $4); if (seen[$4]++) next; "
                      "id = $4; gsub(\"\\t\", \"\\036\", id); lbl = $4; gsub(/[[:cntrl:]]/, \" \", lbl); "
                      "printf \"%d\\t%s\\t%s\\t%s%%\\n\", ($1==\"*\"), id, lbl, $2 }' "
                      "| head -n 6",
                      (char *)NULL);
            } else if (app->subpanel == SUB_BT) {
                execl("/bin/sh", "sh", "-c",
                      "bluetoothctl devices Paired 2>/dev/null | head -n 6 | while read -r _ mac name; do "
                      "  [ -z \"$mac\" ] && continue; "
                      "  if bluetoothctl info \"$mac\" 2>/dev/null | grep -q 'Connected: yes'; then "
                      "    printf '1\\t%s\\t%s\\tConnected\\n' \"$mac\" \"$name\"; "
                      "  else "
                      "    printf '0\\t%s\\t%s\\tPaired\\n' \"$mac\" \"$name\"; "
                      "  fi; "
                      "done",
                      (char *)NULL);
            } else if (app->subpanel == SUB_AUDIO) {
                execl("/bin/sh", "sh", "-c",
                      "def=$(pactl get-default-sink 2>/dev/null); "
                      "pactl list sinks 2>/dev/null | awk -F': ' -v d=\"$def\" '"
                      "/^\\s*Name:/ { n=$2 } "
                      "/^\\s*Description:/ { printf \"%d\\t%s\\t%s\\tOutput\\n\", (n==d), n, $2 }"
                      "' | head -n 6",
                      (char *)NULL);
            }
        } else if (kind == 4) {
            execl("/bin/sh", "sh", "-c", "cliphist list 2>/dev/null | head -n 24", (char *)NULL);
        }
        _exit(127);
    }

    close(pfd[1]);
    int flags = fcntl(pfd[0], F_GETFL, 0);
    fcntl(pfd[0], F_SETFL, flags | O_NONBLOCK);
    app->async_fd = pfd[0];
    app->async_pid = pid;
    app->async_kind = kind;
    app->async_len = 0;
}

static void refresh_snapshot(EdgeApp *app) {
    start_async_refresh(app, MODE_CENTER);
}

static void refresh_control_data(EdgeApp *app) {
    start_async_refresh(app, MODE_CONTROL);
}

static void refresh_subpanel_data(EdgeApp *app) {
    app->sub_count = 0;
    start_async_refresh(app, 3);
}

static void refresh_clipboard_data(EdgeApp *app) {
    start_async_refresh(app, 4);
}

/* ========================================================================== */
/* Cairo + Pango helpers                                                       */
static void fill_rect(cairo_t *cr, double x, double y, double w, double h) {
    cairo_rectangle(cr, round(x), round(y), round(w), round(h));
    cairo_fill(cr);
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

static void draw_text_right_max(cairo_t *cr, PangoLayout *layout, const char *font_str,
                                const char *text, double right_x, double cy, double max_w);

static void draw_text_right(cairo_t *cr, PangoLayout *layout, const char *font_str,
                            const char *text, double right_x, double cy) {
    draw_text_right_max(cr, layout, font_str, text, right_x, cy, 72.0);
}

/* Right-aligned at right_x, cut with an ellipsis past max_w. */
static void draw_text_right_max(cairo_t *cr, PangoLayout *layout, const char *font_str,
                                const char *text, double right_x, double cy, double max_w) {
    PangoFontDescription *desc = pango_font_description_from_string(font_str);
    pango_layout_set_font_description(layout, desc);
    pango_font_description_free(desc);
    pango_layout_set_width(layout, -1);
    pango_layout_set_ellipsize(layout, PANGO_ELLIPSIZE_NONE);
    pango_layout_set_text(layout, text, -1);

    PangoRectangle ink, logical;
    pango_layout_get_extents(layout, &ink, &logical);
    double w = (double)logical.width / (double)PANGO_SCALE;
    if (w > max_w) {
        pango_layout_set_width(layout, (int)(max_w * PANGO_SCALE));
        pango_layout_set_ellipsize(layout, PANGO_ELLIPSIZE_END);
        pango_layout_get_extents(layout, &ink, &logical);
        w = (double)logical.width / (double)PANGO_SCALE;
    }
    double oy = ((double)logical.y + (double)logical.height / 2.0) / (double)PANGO_SCALE;
    cairo_move_to(cr, right_x - w, cy - oy);
    pango_cairo_show_layout(cr, layout);
    pango_layout_set_width(layout, -1);
    pango_layout_set_ellipsize(layout, PANGO_ELLIPSIZE_NONE);
}

/* the panel being drawn, for the helpers below that take no EdgeApp (set by draw_panels) */
static const struct EdgeApp *g_theme;
static int64_t now_ns(void);

static void add_hitbox_nav(EdgeApp *app, double x, double y, double w, double h,
                           int action_id, int index, int nav_idx) {
    if (app->hitbox_count >= MAX_HITBOXES) return;
    HitBox *hb = &app->hitboxes[app->hitbox_count++];
    hb->x = x;
    hb->y = y;
    hb->w = w;
    hb->h = h;
    hb->action_id = action_id;
    hb->index = index;
    hb->nav_idx = nav_idx;
}

static void add_hitbox(EdgeApp *app, double x, double y, double w, double h, int action_id, int index) {
    add_hitbox_nav(app, x, y, w, h, action_id, index, -1);
}

static void parse_hex_rgb(const char *val, double out[3]) {
    while (*val && (*val == ' ' || *val == '\t' || *val == '"' || *val == '\'')) val++;
    if (*val == '#') val++;
    unsigned int r = 0, g = 0, b = 0;
    if (sscanf(val, "%02x%02x%02x", &r, &g, &b) == 3) {
        out[0] = r / 255.0;
        out[1] = g / 255.0;
        out[2] = b / 255.0;
    }
}

static void load_position_config(EdgeApp *app) {
    /* config/config's default (EDGE_POSITION="top"): a user with no config line got the bottom */
    app->bottom_edge = false;
    app->control_on_left = false;
    app->hair_set = false;
    app->square = false;
    app->surface_rgb[0] = 0x11 / 255.0; app->surface_rgb[1] = 0x13 / 255.0; app->surface_rgb[2] = 0x17 / 255.0;
    app->border_rgb[0]  = 0x33 / 255.0; app->border_rgb[1]  = 0x3b / 255.0; app->border_rgb[2]  = 0x54 / 255.0;
    app->text_rgb[0]    = 0xee / 255.0; app->text_rgb[1]    = 0xf2 / 255.0; app->text_rgb[2]    = 0xf8 / 255.0;
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
        /* EDGE_POSITION and EDGE_CONTROL_SIDE; ISLAND_EDGE and ISLAND_CONTROL_SIDE, their names
         * before 2026-10-31, still read */
        const char *v = NULL;
        if (strncmp(p, "EDGE_POSITION=", 14) == 0) v = p + 14;
        else if (strncmp(p, "ISLAND_EDGE=", 12) == 0) v = p + 12;
        if (v) {
            if (config_value_is(v, "top")) app->bottom_edge = false;
            else if (config_value_is(v, "bottom")) app->bottom_edge = true;
            continue;
        }
        if (strncmp(p, "EDGE_CONTROL_SIDE=", 18) == 0) v = p + 18;
        else if (strncmp(p, "ISLAND_CONTROL_SIDE=", 20) == 0) v = p + 20;
        if (v) {
            if (config_value_is(v, "left")) app->control_on_left = true;
            else if (config_value_is(v, "right")) app->control_on_left = false;
        } else if (strncmp(p, "SURFACE_COLOR=", 14) == 0) {
            parse_hex_rgb(p + 14, app->surface_rgb);
        } else if (strncmp(p, "BORDER_INACTIVE=", 16) == 0) {
            parse_hex_rgb(p + 16, app->hair_rgb);
            app->hair_set = true;
        } else if (strncmp(p, "SQUARE_CORNERS=", 15) == 0) {
            app->square = config_value_is(p + 15, "true") || config_value_is(p + 15, "1") ||
                          config_value_is(p + 15, "yes") || config_value_is(p + 15, "on");
        } else if (strncmp(p, "BORDER_ACTIVE=", 14) == 0) {
            parse_hex_rgb(p + 14, app->border_rgb);
        } else if (strncmp(p, "TEXT_COLOR=", 11) == 0) {
            parse_hex_rgb(p + 11, app->text_rgb);
        }
    }
    fclose(f);
}

static void update_surface_input_region(EdgeOutput *out) {
    EdgeApp *app = out->app;
    if (!out->surface || !app->compositor) return;

    struct wl_region *reg = wl_compositor_create_region(app->compositor);
    int strip_y = app->bottom_edge ? (out->height - 6) : 0;
    int pill_w = 240;
    wl_region_add(reg, (out->width - pill_w) / 2, strip_y, pill_w, 6);
    int corner_w = 160;
    if (app->control_on_left) {
        wl_region_add(reg, 0, strip_y, corner_w, 6);
    } else {
        wl_region_add(reg, out->width - corner_w, strip_y, corner_w, 6);
    }

    if (app->mode == MODE_CLOSED && app->anim_progress <= 0.02 && !app->dragging) {
        zwlr_layer_surface_v1_set_keyboard_interactivity(
            out->layer_surface, ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_NONE);
    } else if (app->dragging) {
        wl_region_add(reg, 0, 0, out->width, out->height);
        zwlr_layer_surface_v1_set_keyboard_interactivity(
            out->layer_surface, ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_ON_DEMAND);
    } else {
        if (app->kb_grab && app->mode != MODE_CLOSED) {
            /* opened by a key, it holds the keyboard: the whole screen takes the pointer too, so
             * a click anywhere else reaches handle_click_at and closes it */
            wl_region_add(reg, 0, 0, out->width, out->height);
        } else if (out->input_w > 0 && out->input_h > 0) {
            wl_region_add(reg, out->input_x, out->input_y, out->input_w, out->input_h);
        }
        zwlr_layer_surface_v1_set_keyboard_interactivity(
            out->layer_surface,
            app->kb_grab ? ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_EXCLUSIVE
                         : ZWLR_LAYER_SURFACE_V1_KEYBOARD_INTERACTIVITY_ON_DEMAND);
    }
    wl_surface_set_input_region(out->surface, reg);
    wl_region_destroy(reg);
}

/* ========================================================================== */
/* Renderer (redraws only damaged rectangles)                                 */
/* ========================================================================== */
static bool allocate_buffer(EdgeOutput *out) {
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
    char tmpl[] = "/tmp/infynitae-edge-XXXXXX";
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

    memset(data, 0, size);
    out->shm_data = data;
    out->shm_size = size;
    out->buf_w = out->width;
    out->buf_h = out->height;
    out->prev_dx = 0;
    out->prev_dy = 0;
    out->prev_dw = out->width;
    out->prev_dh = out->height;
    return true;
}

static bool output_has_fullscreen(EdgeOutput *out);

/* ========================================================================== */
/* One look for every panel (owner, 2026-11-06: "super A and W feel weird, everything to be clean")
 * Fonts: sans-serif everywhere (12-15 px instead of 10-11); rows 32 px; a 40 px search field on the
 * side of the screen edge; panel outlines in the hairline colour; the accent (BORDER_ACTIVE) only
 * for what is selected or on; rounding off with SQUARE_CORNERS.                                    */
/* ========================================================================== */

#define CTRL_ROW 28.0
#define FONT_CAPTION "sans-serif 9"
#define FONT_BODY "sans-serif 10"
#define FONT_BODY_MEDIUM "sans-serif Medium 10"
#define FONT_SEARCH "sans-serif 11"
#define LIST_ROWS 8
#define ROW_H 32.0
#define SEARCH_H 40.0
#define PANEL_PAD 4.0

static void set_hair(cairo_t *cr, const EdgeApp *app, double alpha) {
    if (app->hair_set) cairo_set_source_rgba(cr, app->hair_rgb[0], app->hair_rgb[1], app->hair_rgb[2], alpha);
    else cairo_set_source_rgba(cr, app->text_rgb[0], app->text_rgb[1], app->text_rgb[2], 0.14 * alpha);
}

static void set_text(cairo_t *cr, const EdgeApp *app, double alpha) {
    cairo_set_source_rgba(cr, app->text_rgb[0], app->text_rgb[1], app->text_rgb[2], alpha);
}

static void set_accent(cairo_t *cr, const EdgeApp *app, double alpha) {
    cairo_set_source_rgba(cr, app->border_rgb[0], app->border_rgb[1], app->border_rgb[2], alpha);
}

/* A rounded rectangle as the current path (square with SQUARE_CORNERS). */
static void round_rect(cairo_t *cr, const EdgeApp *app, double x, double y, double w, double h, double r) {
    if (app->square || r <= 0.0) {
        cairo_rectangle(cr, x, y, w, h);
        return;
    }
    cairo_new_sub_path(cr);
    cairo_arc(cr, x + w - r, y + r, r, -M_PI / 2, 0);
    cairo_arc(cr, x + w - r, y + h - r, r, 0, M_PI / 2);
    cairo_arc(cr, x + r, y + h - r, r, M_PI / 2, M_PI);
    cairo_arc(cr, x + r, y + r, r, M_PI, 3 * M_PI / 2);
    cairo_close_path(cr);
}

/* The panel's background and outline. */
static void draw_panel_box(cairo_t *cr, const EdgeApp *app, double x, double y, double w, double h, double ease) {
    cairo_new_path(cr);
    round_rect(cr, app, x + 0.5, y + 0.5, w - 1.0, h - 1.0, 8.0);
    cairo_set_source_rgba(cr, app->surface_rgb[0], app->surface_rgb[1], app->surface_rgb[2], 0.96 * ease);
    cairo_fill_preserve(cr);
    set_hair(cr, app, 0.95 * ease);
    cairo_set_line_width(cr, 1.0);
    cairo_stroke(cr);
}

typedef struct {
    const char *primary;
    const char *secondary;
    bool focused;    /* the window with the focus: a small accent dot */
    bool attention;  /* a window asking for attention: an accent dot and its title in medium */
    int action;
    int index;
} ListRow;

/* The selected row moved into view. */
static void list_follow(EdgeApp *app, int count) {
    if (app->selected_idx < 0) app->selected_idx = 0;
    if (count > 0 && app->selected_idx >= count) app->selected_idx = count - 1;
    if (app->selected_idx < app->list_scroll) app->list_scroll = app->selected_idx;
    if (app->selected_idx >= app->list_scroll + LIST_ROWS) app->list_scroll = app->selected_idx - LIST_ROWS + 1;
    if (app->list_scroll > count - LIST_ROWS) app->list_scroll = count - LIST_ROWS;
    if (app->list_scroll < 0) app->list_scroll = 0;
}

/* One list panel: the search field on the screen-edge side, then up to LIST_ROWS rows growing away
 * from it (the first result next to the field), the selected one marked, a count when there are
 * more than fit. `rows` holds all `count` rows; `empty` is said when there are none; `right_action`
 * (>= 0) adds a word at the right of the search field (Clear). Returns the panel rectangle. */
static void draw_list_panel(EdgeApp *app, cairo_t *cr, PangoLayout *layout, int w, int h, double ease,
                            double panel_w, const ListRow *rows, int count, const char *placeholder,
                            const char *empty, const char *right_word, int right_action, int rect[4]) {
    list_follow(app, count);
    int shown = count < LIST_ROWS ? count : LIST_ROWS;
    int slots = shown > 0 ? shown : 1;
    double list_h = PANEL_PAD * 2 + slots * ROW_H;
    double total_h = SEARCH_H + 1.0 + list_h;
    double bw = panel_w;
    if (bw > w - 24.0) bw = w - 24.0;
    double bx = round((w - bw) / 2.0);
    double by = round(app->bottom_edge ? (h + total_h * (1.0 - ease) - (total_h + 12.0) * ease)
                                       : (-total_h * (1.0 - ease) + 12.0 * ease));
    rect[0] = (int)bx - 2;
    rect[1] = (int)by - 2;
    rect[2] = (int)bw + 4;
    rect[3] = (int)total_h + 4;
    draw_panel_box(cr, app, bx, by, bw, total_h, ease);

    /* the search field, with the screen edge */
    double sy = app->bottom_edge ? by + list_h + 1.0 : by;
    double scy = sy + SEARCH_H / 2.0;
    double right_w = 0.0;
    if (right_word && right_action >= 0 && count > 0) {
        right_w = 64.0;
        set_text(cr, app, 0.55 * ease);
        draw_text_right(cr, layout, FONT_CAPTION, right_word, bx + bw - 14.0, scy);
        add_hitbox(app, bx + bw - right_w, sy, right_w, SEARCH_H, right_action, 0);
    } else if (count > LIST_ROWS) {
        char more[32];
        snprintf(more, sizeof(more), "%d of %d", app->selected_idx + 1, count);
        right_w = 72.0;
        set_text(cr, app, 0.40 * ease);
        draw_text_right(cr, layout, FONT_CAPTION, more, bx + bw - 14.0, scy);
    }
    if (app->search_len > 0) {
        set_text(cr, app, 0.96 * ease);
        draw_text_left(cr, layout, FONT_SEARCH, app->search, bx + 16.0, scy, bw - 32.0 - right_w);
    } else {
        set_text(cr, app, 0.42 * ease);
        draw_text_left(cr, layout, FONT_SEARCH, placeholder, bx + 16.0, scy, bw - 32.0 - right_w);
    }
    add_hitbox(app, bx, sy, bw - right_w, SEARCH_H, ACT_SEARCH_BOX, 0);
    /* the divider between the field and the list */
    set_hair(cr, app, 0.8 * ease);
    cairo_set_line_width(cr, 1.0);
    double div_y = app->bottom_edge ? sy - 0.5 : sy + SEARCH_H + 0.5;
    cairo_move_to(cr, bx + 1.0, round(div_y) + 0.5);
    cairo_line_to(cr, bx + bw - 1.0, round(div_y) + 0.5);
    cairo_stroke(cr);

    double list_top = app->bottom_edge ? by : sy + SEARCH_H + 1.0;
    if (shown == 0) {
        set_text(cr, app, 0.45 * ease);
        draw_text_left(cr, layout, FONT_BODY, empty, bx + 16.0, list_top + PANEL_PAD + ROW_H / 2.0, bw - 32.0);
        return;
    }
    for (int k = 0; k < shown; k++) {
        int i = app->list_scroll + k;
        const ListRow *r = &rows[i];
        /* row k counted from the search field outward */
        double ry = app->bottom_edge ? list_top + PANEL_PAD + (slots - 1 - k) * ROW_H
                                     : list_top + PANEL_PAD + k * ROW_H;
        double rx = bx + PANEL_PAD, rw = bw - 2 * PANEL_PAD;
        bool sel = app->selected_idx == i;
        if (sel) {
            cairo_new_path(cr);
            round_rect(cr, app, rx, ry, rw, ROW_H, 6.0);
            set_accent(cr, app, 0.18 * ease);
            cairo_fill(cr);
            set_accent(cr, app, 0.95 * ease);
            fill_rect(cr, rx, ry + 6.0, 2.0, ROW_H - 12.0);
        }
        double tx = rx + 14.0;
        if (r->attention || r->focused) {
            cairo_new_path(cr);
            set_accent(cr, app, (r->attention ? 0.95 : 0.6) * ease);
            cairo_arc(cr, rx + 14.0, ry + ROW_H / 2.0, r->attention ? 3.5 : 2.5, 0, 2 * M_PI);
            cairo_fill(cr);
            tx = rx + 26.0;
        }
        double sec_w = 0.0;
        if (r->secondary && r->secondary[0]) {
            sec_w = rw * 0.34;
            set_text(cr, app, 0.50 * ease);
            draw_text_right_max(cr, layout, FONT_CAPTION, r->secondary, rx + rw - 12.0, ry + ROW_H / 2.0, sec_w - 8.0);
        }
        set_text(cr, app, (sel ? 0.98 : 0.84) * ease);
        draw_text_left(cr, layout, (r->attention || r->focused) ? FONT_BODY_MEDIUM : FONT_BODY, r->primary, tx,
                       ry + ROW_H / 2.0, rx + rw - tx - 16.0 - sec_w);
        add_hitbox_nav(app, rx, ry, rw, ROW_H, r->action, r->index, i);
    }
}

/* Everything the edge shows for a screen of w x h at the current mode and progress, into cr; the
 * rectangle drawn in goes to *rect (x, y, w, h). Shared by the live panels and --render. */
static void draw_panels(EdgeApp *app, cairo_t *cr, PangoLayout *layout, int w, int h, int rect[4]) {
    g_theme = app;
    app->hitbox_count = 0;

    double p = app->anim_progress;
    if (p < 0.0) p = 0.0;
    if (p > 1.0) p = 1.0;
    double ease = 1.0 - pow(1.0 - p, 3.0);

    int cur_dx = 0, cur_dy = 0, cur_dw = 0, cur_dh = 0;

    if (p <= 0.01) {
        double hit_y = app->bottom_edge ? (h - 6.0) : 0.0;
        add_hitbox(app, (w - 240.0) / 2.0, hit_y, 240.0, 6.0, ACT_CENTER_EDGE, 0);
        double corner_x = app->control_on_left ? 0.0 : (w - 160.0);
        add_hitbox(app, corner_x, hit_y, 160.0, 6.0, ACT_CORNER_EDGE, 0);
    } else {
        EdgeMode active_m = (app->mode != MODE_CLOSED) ? app->mode : app->target_mode;

        /* A. Window list (Super+L): every window, title and app, the focused one and the ones
         *    asking for attention marked */
        if (active_m == MODE_CENTER) {
            static ListRow rows[MAX_WINDOWS];
            for (int i = 0; i < app->filtered_win_count; i++) {
                CanvasWindow *cw = &app->windows[app->filtered_wins[i]];
                rows[i] = (ListRow){ cw->title[0] ? cw->title : cw->app_id, cw->title[0] ? cw->app_id : "",
                                     cw->focused, cw->attention, ACT_WIN_ITEM, app->filtered_wins[i] };
            }
            int rect[4];
            draw_list_panel(app, cr, layout, w, h, ease, 640.0, rows, app->filtered_win_count, "Search windows",
                            app->search_len > 0 ? "No matching windows" : "No open windows", NULL, -1, rect);
            cur_dx = rect[0]; cur_dy = rect[1]; cur_dw = rect[2]; cur_dh = rect[3];
        }

        /* A2. Apps (Super+W): one column, the best match next to the search field */
        if (active_m == MODE_APPS) {
            static ListRow rows[MAX_APPS];
            for (int i = 0; i < app->filtered_app_count; i++) {
                DesktopApp *da = &app->apps[app->filtered_apps[i]];
                rows[i] = (ListRow){ da->name, da->generic, false, false, ACT_APP_LAUNCH, app->filtered_apps[i] };
            }
            int rect[4];
            draw_list_panel(app, cr, layout, w, h, ease, 560.0, rows, app->filtered_app_count, "Search apps",
                            "No matching apps", NULL, -1, rect);
            cur_dx = rect[0]; cur_dy = rect[1]; cur_dw = rect[2]; cur_dh = rect[3];
        }

        if (active_m == MODE_CONTROL) {
            /* B. Control panel (Super+A). Owner, 2026-11-06: "the design feels mid ... the text is
             * over": labels ran into their values (Power mode / Performance) and a notification into
             * its app name. Now every piece of text has its own room and is cut with an ellipsis:
             * a clock header, a volume slider, six tiles (label above its state, lit in the accent
             * when on), then optional sections under a small title, then three buttons. No dividers:
             * space and section titles separate. Navigation order and actions are unchanged. */
            const double PAD = 12.0, GAP = 6.0, SECTION = 12.0;
            const double HEAD_H = 34.0, VOL_H = 32.0, TILE_H = 46.0, ITEM_H = 30.0;
            double NOTIF_H = 42.0;
            const double TITLE_H = 20.0, BTN_H = 32.0;

            int sub_rows = (app->subpanel != SUB_NONE) ? (app->sub_count > 0 ? app->sub_count : 1) : 0;
            int ctx_rows = (app->tray_ctx_idx >= 0 && app->tray_ctx_idx < app->tray_count)
                ? ((app->ctx_count + 2) / 3) : 0;
            double sub_extra_h = sub_rows ? (GAP + sub_rows * ITEM_H) : 0.0;
            double tray_extra_h = app->tray_count > 0 ? (SECTION + TITLE_H + app->tray_count * ITEM_H + ctx_rows * ITEM_H) : 0.0;
            double fixed_h = PAD + HEAD_H + GAP + VOL_H + GAP + 3.0 * TILE_H + 2.0 * GAP + sub_extra_h + tray_extra_h +
                             SECTION + BTN_H + PAD;
            /* a short screen with every section full: notifications on one line each */
            bool notif_compact = fixed_h + (SECTION + TITLE_H + app->notif_count * NOTIF_H) > h - 16.0;
            if (notif_compact) NOTIF_H = ITEM_H + 2.0;
            double notif_extra_h = app->notif_count > 0 ? (SECTION + TITLE_H + app->notif_count * NOTIF_H) : 0.0;

            double ctrl_w = 360.0;
            double ctrl_h = fixed_h + notif_extra_h;
            double ctrl_x = app->control_on_left ? 8.0 : (w - ctrl_w - 8.0);
            double ctrl_y = round(app->bottom_edge
                ? (h + ctrl_h * (1.0 - ease) - (ctrl_h + 8.0) * ease)
                : (-ctrl_h * (1.0 - ease) + 8.0 * ease));

            cur_dx = (int)ctrl_x - 2;
            cur_dy = (int)ctrl_y - 2;
            cur_dw = (int)ctrl_w + 4;
            cur_dh = (int)ctrl_h + 4;

            draw_panel_box(cr, app, ctrl_x, ctrl_y, ctrl_w, ctrl_h, ease);

            double rx = ctrl_x + PAD;
            double rw = ctrl_w - 2.0 * PAD;
            double ry = ctrl_y + PAD;
            const double *acc = app->border_rgb;

            /* a rounded tile: the accent when on, a faint fill when off; selected: an accent outline */
#define CTRL_TILE(X, Y, W, H, ON, SEL)                                                              \
            do {                                                                                    \
                cairo_new_path(cr);                                                                 \
                round_rect(cr, app, (X) + 0.5, (Y) + 0.5, (W) - 1.0, (H) - 1.0, 7.0);               \
                if (ON) cairo_set_source_rgba(cr, acc[0], acc[1], acc[2], 0.26 * ease);             \
                else set_text(cr, app, 0.055 * ease);                                               \
                cairo_fill_preserve(cr);                                                            \
                if (SEL) {                                                                          \
                    set_accent(cr, app, 0.95 * ease);                                               \
                    cairo_set_line_width(cr, 1.0);                                                  \
                    cairo_stroke(cr);                                                               \
                } else {                                                                            \
                    cairo_new_path(cr);                                                             \
                }                                                                                   \
            } while (0)

            /* Header: the time large, the date beside it, the battery at the right */
            time_t now = time(NULL);
            struct tm *tm_info = localtime(&now);
            char time_s[16], date_s[32];
            strftime(time_s, sizeof(time_s), "%H:%M", tm_info);
            strftime(date_s, sizeof(date_s), "%A, %B %d", tm_info);
            set_text(cr, app, 0.96 * ease);
            draw_text_left(cr, layout, "sans-serif Medium 16", time_s, rx + 2.0, ry + HEAD_H / 2.0, 90.0);
            set_text(cr, app, 0.55 * ease);
            draw_text_left(cr, layout, FONT_BODY, date_s, rx + 70.0, ry + HEAD_H / 2.0 + 1.0, rw - 140.0);
            if (app->battery_pct >= 0) {
                char bat_s[24];
                snprintf(bat_s, sizeof(bat_s), "%s%d%%", app->battery_charging ? "Charging " : "", app->battery_pct);
                set_text(cr, app, 0.70 * ease);
                draw_text_right_max(cr, layout, FONT_BODY, bat_s, rx + rw - 2.0, ry + HEAD_H / 2.0 + 1.0, 100.0);
            }
            ry += HEAD_H + GAP;

            /* Volume: a label (click: mute), a slider with a knob, the level */
            bool vol_sel = app->subpanel == SUB_AUDIO;
            CTRL_TILE(rx, ry, rw, VOL_H, false, vol_sel);
            set_text(cr, app, (app->volume_muted ? 0.45 : 0.85) * ease);
            draw_text_left(cr, layout, FONT_BODY, app->volume_muted ? "Muted" : "Volume", rx + 12.0, ry + VOL_H / 2.0, 58.0);
            add_hitbox(app, rx, ry, 70.0, VOL_H, ACT_VOL_MUTE, 0);

            double bx = rx + 76.0;
            double bw = rw - 76.0 - 56.0;
            double by = ry + VOL_H / 2.0;
            cairo_set_line_cap(cr, CAIRO_LINE_CAP_ROUND);
            cairo_set_line_width(cr, 4.0);
            set_text(cr, app, 0.14 * ease);
            cairo_move_to(cr, bx, by);
            cairo_line_to(cr, bx + bw, by);
            cairo_stroke(cr);
            double level = app->volume_muted ? 0.0 : (app->volume_pct > 100 ? 100 : app->volume_pct) / 100.0;
            double kx = bx + bw * level;
            if (level > 0.0) {
                cairo_set_source_rgba(cr, acc[0], acc[1], acc[2], 0.95 * ease);
                cairo_move_to(cr, bx, by);
                cairo_line_to(cr, kx, by);
                cairo_stroke(cr);
            }
            cairo_set_line_cap(cr, CAIRO_LINE_CAP_BUTT);
            cairo_new_path(cr);
            cairo_arc(cr, kx, by, 6.0, 0, 2 * M_PI);
            set_text(cr, app, (app->volume_muted ? 0.45 : 0.95) * ease);
            cairo_fill(cr);
            add_hitbox(app, bx, ry, bw, VOL_H, ACT_VOL_BAR, (int)bx);

            char v_s[16];
            if (app->volume_muted) strcpy(v_s, "Off");
            else snprintf(v_s, sizeof(v_s), "%d%%", app->volume_pct);
            add_hitbox(app, bx + bw + 4.0, ry, rx + rw - (bx + bw + 4.0), VOL_H, ACT_VOL_MUTE, 0);
            set_text(cr, app, 0.80 * ease);
            draw_text_right_max(cr, layout, FONT_BODY, v_s, rx + rw - 12.0, ry + VOL_H / 2.0, 40.0);
            ry += VOL_H + GAP;

            /* Six tiles in two columns: the name, and under it its state */
            double tw = floor((rw - GAP) / 2.0);
            const char *p_disp = "Balanced";
            if (strcmp(app->power_profile, "none") == 0) p_disp = "Not available";
            else if (strcasecmp(app->power_profile, "performance") == 0) p_disp = "Performance";
            else if (strcasecmp(app->power_profile, "power-saver") == 0) p_disp = "Power saver";

            struct {
                const char *label;
                const char *val;
                bool active;
                int action;
                bool sub_open;
            } sw[6] = {
                { "Wi-Fi", app->wifi_label, app->wifi_on, ACT_WIFI_TOGGLE, app->subpanel == SUB_WIFI },
                { "Bluetooth", app->bt_label, app->bt_on, ACT_BT_TOGGLE, app->subpanel == SUB_BT },
                { "Do not disturb", app->dnd_on ? "On" : "Off", app->dnd_on, ACT_DND_TOGGLE, false },
                { "Power mode", p_disp, strcasecmp(app->power_profile, "performance") == 0, ACT_POWER_PROFILE, false },
                { "Replay", app->replay_on ? "Recording" : "Off", app->replay_on, ACT_REPLAY_TOGGLE, false },
                { "Save clip", app->replay_on ? "Last 20 minutes" : "Replay is off", false, ACT_REPLAY_SAVE, false }
            };

            for (int i = 0; i < 6; i++) {
                double sx = rx + (i % 2) * (tw + GAP);
                double sy = ry + (i / 2) * (TILE_H + GAP);
                double sw_w = (i % 2) ? (rw - tw - GAP) : tw;
                bool sel = (app->selected_idx == i) || sw[i].sub_open;
                bool dim = sw[i].action == ACT_REPLAY_SAVE && !app->replay_on;
                CTRL_TILE(sx, sy, sw_w, TILE_H, sw[i].active, sel);
                set_text(cr, app, (dim ? 0.45 : 0.95) * ease);
                draw_text_left(cr, layout, FONT_BODY_MEDIUM, sw[i].label, sx + 12.0, sy + 16.0, sw_w - 24.0);
                if (sw[i].active) cairo_set_source_rgba(cr, acc[0], acc[1], acc[2], 0.95 * ease);
                else set_text(cr, app, (dim ? 0.35 : 0.55) * ease);
                draw_text_left(cr, layout, FONT_CAPTION, sw[i].val[0] ? sw[i].val : "Off", sx + 12.0, sy + 32.0, sw_w - 24.0);
                add_hitbox_nav(app, sx, sy, sw_w, TILE_H, sw[i].action, i, i);
            }
            ry += 3.0 * TILE_H + 2.0 * GAP;
            int nav_cursor = 6;

            /* Under the tiles: the networks, devices or outputs of an open tile */
            if (app->subpanel != SUB_NONE) {
                ry += GAP;
                if (app->sub_count == 0) {
                    set_text(cr, app, 0.45 * ease);
                    draw_text_left(cr, layout, FONT_BODY, "None found", rx + 12.0, ry + ITEM_H / 2.0, rw - 24.0);
                    ry += ITEM_H;
                } else {
                    for (int i = 0; i < app->sub_count; i++) {
                        SubItem *si = &app->sub_items[i];
                        int nav_i = nav_cursor++;
                        bool sel = (app->selected_idx == nav_i);
                        if (sel || si->active) CTRL_TILE(rx, ry, rw, ITEM_H - 2.0, si->active, sel);
                        set_text(cr, app, (si->active || sel ? 0.95 : 0.72) * ease);
                        draw_text_left(cr, layout, si->active ? FONT_BODY_MEDIUM : FONT_BODY, si->label,
                                       rx + 12.0, ry + (ITEM_H - 2.0) / 2.0, rw - 110.0);
                        set_text(cr, app, 0.50 * ease);
                        draw_text_right_max(cr, layout, FONT_CAPTION, si->meta, rx + rw - 12.0, ry + (ITEM_H - 2.0) / 2.0, 84.0);
                        add_hitbox_nav(app, rx, ry, rw, ITEM_H, ACT_SUB_SELECT, i, nav_i);
                        ry += ITEM_H;
                    }
                }
            }

            /* Background apps (hidden when there are none) */
            if (app->tray_count > 0) {
                ry += SECTION;
                set_text(cr, app, 0.48 * ease);
                draw_text_left(cr, layout, FONT_CAPTION, "Background apps", rx + 2.0, ry + TITLE_H / 2.0, rw - 4.0);
                ry += TITLE_H;
                for (int i = 0; i < app->tray_count; i++) {
                    TrayApp *ta = &app->tray_apps[i];
                    int nav_i = nav_cursor++;
                    bool ctx_open = (app->tray_ctx_idx == i);
                    bool sel = (app->selected_idx == nav_i);
                    if (sel || ctx_open) CTRL_TILE(rx, ry, rw, ITEM_H - 2.0, false, sel);
                    set_text(cr, app, (sel || ctx_open ? 0.96 : 0.82) * ease);
                    draw_text_left(cr, layout, FONT_BODY, ta->name, rx + 12.0, ry + (ITEM_H - 2.0) / 2.0, rw - 110.0);
                    set_text(cr, app, 0.48 * ease);
                    draw_text_right_max(cr, layout, FONT_CAPTION, ta->mem, rx + rw - 12.0, ry + (ITEM_H - 2.0) / 2.0, 84.0);
                    add_hitbox_nav(app, rx, ry, rw, ITEM_H, ACT_TRAY_ITEM, i, nav_i);
                    ry += ITEM_H;

                    /* the app's own menu (com.canonical.dbusmenu) or Open / Quit / Kill */
                    if (ctx_open && app->ctx_count > 0) {
                        int cols = app->ctx_count < 3 ? app->ctx_count : 3;
                        double cw = floor((rw - (cols - 1) * GAP) / cols);
                        for (int k = 0; k < app->ctx_count; k++) {
                            double cx_b = rx + (k % 3) * (cw + GAP);
                            double cy_b = ry + (k / 3) * ITEM_H;
                            int c_nav = nav_cursor++;
                            bool c_sel = (app->selected_idx == c_nav);
                            CTRL_TILE(cx_b, cy_b, cw, ITEM_H - 4.0, false, c_sel);
                            set_text(cr, app, (c_sel ? 0.98 : 0.80) * ease);
                            draw_text_centered(cr, layout, FONT_BODY, app->ctx_items[k].label,
                                               cx_b + cw / 2.0, cy_b + (ITEM_H - 4.0) / 2.0, false);
                            add_hitbox_nav(app, cx_b, cy_b, cw, ITEM_H - 4.0, ACT_TRAY_CTX_CLICK, k, c_nav);
                        }
                        ry += ((app->ctx_count + 2) / 3) * ITEM_H;
                    }
                }
            }

            /* Notifications (mako's current and history; hidden when there are none): the app above
             * the text, each on its own line */
            if (app->notif_count > 0) {
                ry += SECTION;
                int clr_nav = nav_cursor++;
                bool clr_sel = (app->selected_idx == clr_nav);
                set_text(cr, app, 0.48 * ease);
                draw_text_left(cr, layout, FONT_CAPTION, "Notifications", rx + 2.0, ry + TITLE_H / 2.0, rw - 80.0);
                if (clr_sel) CTRL_TILE(rx + rw - 56.0, ry, 56.0, TITLE_H, false, true);
                set_text(cr, app, (clr_sel ? 0.96 : 0.66) * ease);
                draw_text_right_max(cr, layout, FONT_CAPTION, "Clear all", rx + rw - 8.0, ry + TITLE_H / 2.0, 50.0);
                add_hitbox_nav(app, rx + rw - 56.0, ry, 56.0, TITLE_H, ACT_NOTIF_CLEAR, 0, clr_nav);
                ry += TITLE_H;

                for (int i = 0; i < app->notif_count; i++) {
                    NotifItem *ni = &app->notifs[i];
                    int nav_i = nav_cursor++;
                    bool sel = (app->selected_idx == nav_i);
                    CTRL_TILE(rx, ry, rw, NOTIF_H - 4.0, false, sel);
                    if (notif_compact) {
                        set_text(cr, app, (sel ? 0.98 : 0.88) * ease);
                        draw_text_left(cr, layout, FONT_BODY, ni->text, rx + 12.0, ry + (NOTIF_H - 4.0) / 2.0, rw - 110.0);
                        set_text(cr, app, 0.50 * ease);
                        draw_text_right_max(cr, layout, FONT_CAPTION, ni->app, rx + rw - 12.0, ry + (NOTIF_H - 4.0) / 2.0, 80.0);
                    } else {
                        set_text(cr, app, 0.50 * ease);
                        draw_text_left(cr, layout, FONT_CAPTION, ni->app[0] ? ni->app : "Notification", rx + 12.0, ry + 12.0, rw - 24.0);
                        set_text(cr, app, (sel ? 0.98 : 0.88) * ease);
                        draw_text_left(cr, layout, FONT_BODY, ni->text, rx + 12.0, ry + 27.0, rw - 24.0);
                    }
                    add_hitbox_nav(app, rx, ry, rw, NOTIF_H - 4.0, ACT_NOTIF_ITEM, i, nav_i);
                    ry += NOTIF_H;
                }
                ry -= 4.0;
            }

            /* Restart, Shut down, Log out: three buttons (Restart and Shut down ask for a second
             * press within 3 s, power_confirm_*) */
            ry += SECTION;
            double pw = floor((rw - 2.0 * GAP) / 3.0);
            bool confirming = app->power_confirm_until > now_ns();
            const char *p_labels[3] = {
                confirming && app->power_confirm_action == ACT_REBOOT ? "Press again" : "Restart",
                confirming && app->power_confirm_action == ACT_POWEROFF ? "Press again" : "Shut down",
                "Log out" };
            int p_acts[3] = { ACT_REBOOT, ACT_POWEROFF, ACT_LOGOUT };
            for (int i = 0; i < 3; i++) {
                double px_b = rx + i * (pw + GAP);
                double pw_i = i == 2 ? (rx + rw - px_b) : pw;
                int p_nav = nav_cursor++;
                bool p_sel = (app->selected_idx == p_nav);
                bool warn = confirming && p_acts[i] == app->power_confirm_action;
                if (warn) {
                    cairo_new_path(cr);
                    round_rect(cr, app, px_b + 0.5, ry + 0.5, pw_i - 1.0, BTN_H - 1.0, 7.0);
                    cairo_set_source_rgba(cr, 0.97, 0.46, 0.56, 0.22 * ease);
                    cairo_fill(cr);
                } else {
                    CTRL_TILE(px_b, ry, pw_i, BTN_H, false, p_sel);
                }
                if (warn) cairo_set_source_rgba(cr, 0.98, 0.62, 0.68, 0.98 * ease);
                else set_text(cr, app, (p_sel ? 0.98 : 0.80) * ease);
                draw_text_centered(cr, layout, warn ? FONT_BODY_MEDIUM : FONT_BODY, p_labels[i],
                                   px_b + pw_i / 2.0, ry + BTN_H / 2.0, false);
                add_hitbox_nav(app, px_b, ry, pw_i, BTN_H, p_acts[i], 0, p_nav);
            }
#undef CTRL_TILE
        }

        /* C. Clipboard (Super+V): the same list, Clear at the right of the search field */
        if (active_m == MODE_CLIPBOARD) {
            static ListRow rows[MAX_CLIP_ITEMS];
            for (int i = 0; i < app->filtered_clip_count; i++) {
                int cidx = app->filtered_clips[i];
                rows[i] = (ListRow){ app->clip_items[cidx].preview, "", false, false, ACT_CLIP_ITEM, cidx };
            }
            int rect[4];
            draw_list_panel(app, cr, layout, w, h, ease, 600.0, rows, app->filtered_clip_count, "Search clipboard",
                            app->clip_count == 0 ? "Clipboard is empty" : "No matching clipboard items", "Clear",
                            ACT_CLIP_CLEAR, rect);
            cur_dx = rect[0]; cur_dy = rect[1]; cur_dw = rect[2]; cur_dh = rect[3];
        }
    }

    rect[0] = cur_dx;
    rect[1] = cur_dy;
    rect[2] = cur_dw;
    rect[3] = cur_dh;
}

static void render_output(EdgeOutput *out) {
    if (!out->configured || out->width <= 0 || out->height <= 0) return;

    /* Closed over a window that fills the screen: unmapped, nothing drawn over it, no strips */
    bool idle = out->app->mode == MODE_CLOSED && out->app->anim_progress <= 0.01 &&
                !out->app->dragging;
    if (idle && output_has_fullscreen(out)) {
        if (!out->unmapped) {
            if (out->frame_cb) {
                wl_callback_destroy(out->frame_cb);
                out->frame_cb = NULL;
            }
            wl_surface_attach(out->surface, NULL, 0, 0);
            wl_surface_commit(out->surface);
            out->unmapped = true;
        }
        return;
    }
    if (out->unmapped) {
        /* layer-shell remap: a commit without a buffer, then draw at the configure. The whole
         * buffer is cleared and damaged then (the compositor dropped its copy) */
        out->unmapped = false;
        out->configured = false;
        out->prev_dx = 0;
        out->prev_dy = 0;
        out->prev_dw = out->width;
        out->prev_dh = out->height;
        wl_surface_commit(out->surface);
        return;
    }

    if (!allocate_buffer(out)) return;

    EdgeApp *app = out->app;
    int w = out->width;
    int h = out->height;

    cairo_surface_t *surf = cairo_image_surface_create_for_data(
        (unsigned char *)out->shm_data, CAIRO_FORMAT_ARGB32, w, h, w * 4);
    cairo_t *cr = cairo_create(surf);

    if (out->prev_dw > 0 && out->prev_dh > 0) {
        cairo_save(cr);
        cairo_set_operator(cr, CAIRO_OPERATOR_CLEAR);
        cairo_rectangle(cr, out->prev_dx, out->prev_dy, out->prev_dw, out->prev_dh);
        cairo_fill(cr);
        cairo_restore(cr);
    }

    PangoLayout *layout = pango_cairo_create_layout(cr);
    int rect[4];
    draw_panels(app, cr, layout, w, h, rect);
    int cur_dx = rect[0], cur_dy = rect[1], cur_dw = rect[2], cur_dh = rect[3];
    g_object_unref(layout);
    cairo_destroy(cr);
    cairo_surface_destroy(surf);

    int dmg_x = out->prev_dx < cur_dx ? out->prev_dx : cur_dx;
    int dmg_y = out->prev_dy < cur_dy ? out->prev_dy : cur_dy;
    int dmg_r = (out->prev_dx + out->prev_dw) > (cur_dx + cur_dw) ? (out->prev_dx + out->prev_dw) : (cur_dx + cur_dw);
    int dmg_b = (out->prev_dy + out->prev_dh) > (cur_dy + cur_dh) ? (out->prev_dy + out->prev_dh) : (cur_dy + cur_dh);
    if (dmg_x < 0) dmg_x = 0;
    if (dmg_y < 0) dmg_y = 0;
    if (dmg_r > w) dmg_r = w;
    if (dmg_b > h) dmg_b = h;

    out->prev_dx = cur_dx;
    out->prev_dy = cur_dy;
    out->prev_dw = cur_dw;
    out->prev_dh = cur_dh;
    out->input_x = cur_dx;
    out->input_y = cur_dy;
    out->input_w = cur_dw;
    out->input_h = cur_dh;

    update_surface_input_region(out);
    wl_surface_attach(out->surface, out->buffer, 0, 0);
    if (dmg_r > dmg_x && dmg_b > dmg_y) {
        wl_surface_damage_buffer(out->surface, dmg_x, dmg_y, dmg_r - dmg_x, dmg_b - dmg_y);
    } else {
        wl_surface_damage_buffer(out->surface, 0, 0, 1, 1);
    }
    wl_surface_commit(out->surface);
}

/* ========================================================================== */
/* Actions & Input                                                            */
/* ========================================================================== */
static void set_mode(EdgeApp *app, EdgeMode new_mode) {
    if (new_mode != MODE_CLOSED) {
        load_position_config(app);
        app->target_mode = new_mode;
        app->mode = new_mode;
        app->kb_grab = true;
        app->search[0] = '\0';
        app->search_len = 0;
        app->subpanel = SUB_NONE;
        app->tray_ctx_idx = -1;
        app->selected_idx = 0;
        app->list_scroll = 0;
        if (new_mode == MODE_CENTER) {
            refresh_snapshot(app);
        } else if (new_mode == MODE_CONTROL) {
            refresh_control_data(app);
            app->selected_idx = (app->tray_count > 0) ? 6 : 0;
        } else if (new_mode == MODE_CLIPBOARD) {
            refresh_clipboard_data(app);
        } else if (new_mode == MODE_APPS) {
            /* each opening: an app just installed is there, the recent list is current */
            load_all_apps(app);
            update_filters(app);
        }
    } else {
        app->mode = MODE_CLOSED;
        app->kb_grab = false;
        app->subpanel = SUB_NONE;
        app->tray_ctx_idx = -1;
    }
    damage_all(app);
}

static void toggle_subpanel(EdgeApp *app, ControlSubpanel target) {
    if (app->subpanel == target) {
        app->subpanel = SUB_NONE;
    } else {
        app->subpanel = target;
        if (target == SUB_WIFI && !app->wifi_on) {
            spawn_detached("nmcli radio wifi on");
            app->wifi_on = true;
            strcpy(app->wifi_label, "On");
        } else if (target == SUB_BT && !app->bt_on) {
            spawn_detached("bluetoothctl power on");
            app->bt_on = true;
            strcpy(app->bt_label, "On");
        }
        refresh_subpanel_data(app);
        app->selected_idx = 6;
    }
    damage_all(app);
}

static void close_window_at(EdgeApp *app, int widx) {
    if (widx < 0 || widx >= app->win_count) return;
    CanvasWindow cw = app->windows[widx];
    for (int k = widx; k < app->win_count - 1; k++) {
        app->windows[k] = app->windows[k + 1];
    }
    app->win_count--;
    update_filters(app);
    damage_all(app);

    char cmd[256];
    snprintf(cmd, sizeof(cmd),
             "infynitae msg focus %lu %lu && infynitae msg shortcut close",
             cw.inst, cw.elem);
    spawn_detached(cmd);
}

static void quit_tray_at(EdgeApp *app, int tidx, const char *sig_name) {
    if (tidx < 0 || tidx >= app->tray_count) return;
    const char *pname = app->tray_apps[tidx].proc_name[0]
        ? app->tray_apps[tidx].proc_name
        : app->tray_apps[tidx].name;
    spawn_script("sh \"$HOME/infynitae/live/control-menu\" apps-kill \"$1\" \"$2\"",
                 pname, sig_name ? sig_name : "TERM");
    for (int k = tidx; k < app->tray_count - 1; k++) {
        app->tray_apps[k] = app->tray_apps[k + 1];
    }
    app->tray_count--;
    app->tray_ctx_idx = -1;
    damage_all(app);
}

static void run_cliphist_with_stdin(const char *cmd, const char *line) {
    if (!cmd || !line || !*line) return;
    int pfd[2];
    if (pipe(pfd) < 0) return;
    pid_t pid = fork();
    if (pid == 0) {
        setsid();
        close(pfd[1]);
        dup2(pfd[0], STDIN_FILENO);
        close(pfd[0]);
        int devnull = open("/dev/null", O_RDWR);
        if (devnull >= 0) {
            dup2(devnull, STDOUT_FILENO);
            dup2(devnull, STDERR_FILENO);
            if (devnull > 2) close(devnull);
        }
        execl("/bin/sh", "sh", "-c", cmd, (char *)NULL);
        _exit(127);
    } else if (pid > 0) {
        close(pfd[0]);
        size_t len = strlen(line);
        ssize_t w = write(pfd[1], line, len);
        (void)w;
        w = write(pfd[1], "\n", 1);
        (void)w;
        close(pfd[1]);
    } else {
        close(pfd[0]);
        close(pfd[1]);
    }
}

static void delete_clip_at(EdgeApp *app, int cidx) {
    if (cidx < 0 || cidx >= app->clip_count) return;
    run_cliphist_with_stdin("cliphist delete 2>/dev/null", app->clip_items[cidx].raw_line);
    for (int k = cidx; k < app->clip_count - 1; k++) {
        app->clip_items[k] = app->clip_items[k + 1];
    }
    app->clip_count--;
    update_filters(app);
    damage_all(app);
}

static void close_selected_item(EdgeApp *app) {
    for (int i = 0; i < app->hitbox_count; i++) {
        HitBox *hb = &app->hitboxes[i];
        if (hb->nav_idx == app->selected_idx) {
            if (hb->action_id == ACT_WIN_ITEM) {
                close_window_at(app, hb->index);
                return;
            }
            if (hb->action_id == ACT_CLIP_ITEM) {
                delete_clip_at(app, hb->index);
                return;
            }
            if (hb->action_id == ACT_TRAY_ITEM) {
                quit_tray_at(app, hb->index, "TERM");
                return;
            }
            if (hb->action_id == ACT_TRAY_CTX_CLICK && app->tray_ctx_idx >= 0) {
                quit_tray_at(app, app->tray_ctx_idx, "TERM");
                return;
            }
            if (hb->action_id == ACT_NOTIF_ITEM || hb->action_id == ACT_NOTIF_CLEAR) {
                app->cleared_notif_id = app->max_notif_id;
                app->notif_count = 0;
                spawn_detached("makoctl dismiss -a 2>/dev/null");
                damage_all(app);
                return;
            }
        }
    }
}

static void launch_app(EdgeApp *app, int index) {
    if (index < 0 || index >= app->app_count) return;
    DesktopApp *da = &app->apps[index];
    recent_apps_note(&app->recent_apps, da->id, (long)time(NULL));
    recent_apps_save(&app->recent_apps);
    set_mode(app, MODE_CLOSED);
    spawn_detached(da->exec_cmd);
}

static bool execute_hitbox(EdgeApp *app, HitBox *hb, double click_x, uint32_t button) {
    if (button == BTN_RIGHT) {
        if (hb->action_id == ACT_WIFI_TOGGLE) {
            toggle_subpanel(app, SUB_WIFI);
            return true;
        }
        if (hb->action_id == ACT_BT_TOGGLE) {
            toggle_subpanel(app, SUB_BT);
            return true;
        }
        if (hb->action_id == ACT_VOL_BAR || hb->action_id == ACT_VOL_MUTE) {
            toggle_subpanel(app, SUB_AUDIO);
            return true;
        }
        if (hb->action_id == ACT_TRAY_ITEM) {
            if (app->tray_ctx_idx == hb->index) {
                app->tray_ctx_idx = -1;
            } else {
                app->tray_ctx_idx = hb->index;
                build_tray_context_menu(app, hb->index);
            }
            damage_all(app);
            return true;
        }
        if (hb->action_id == ACT_WIN_ITEM) {
            close_window_at(app, hb->index);
            return true;
        }
        if (hb->action_id == ACT_CLIP_ITEM) {
            delete_clip_at(app, hb->index);
            return true;
        }
    }

    if (button == BTN_MIDDLE) {
        if (hb->action_id == ACT_WIN_ITEM) {
            close_window_at(app, hb->index);
            return true;
        }
        if (hb->action_id == ACT_CLIP_ITEM) {
            delete_clip_at(app, hb->index);
            return true;
        }
        if (hb->action_id == ACT_TRAY_ITEM) {
            quit_tray_at(app, hb->index, "TERM");
            return true;
        }
        return true;
    }

    switch (hb->action_id) {
    case ACT_CENTER_EDGE:
        set_mode(app, MODE_CENTER);
        return true;
    case ACT_CORNER_EDGE:
        set_mode(app, MODE_CONTROL);
        return true;
    case ACT_WIN_ITEM: {
        CanvasWindow *cw = &app->windows[hb->index];
        char cmd[256];
        snprintf(cmd, sizeof(cmd),
                 "infynitae msg navigate element %lu %lu && infynitae msg focus %lu %lu",
                 cw->inst, cw->elem, cw->inst, cw->elem);
        for (int k = 0; k < app->win_count; k++) {
            app->windows[k].focused = (k == hb->index);
        }
        app->kb_grab = false;
        damage_all(app);
        spawn_detached(cmd);
        return true;
    }
    case ACT_SEARCH_BOX:
        return true;
    case ACT_APP_LAUNCH:
        launch_app(app, hb->index);
        return true;
    case ACT_TRAY_ITEM: {
        int tidx = hb->index;
        if (tidx >= 0 && tidx < app->tray_count) {
            const char *pname = app->tray_apps[tidx].proc_name[0]
                ? app->tray_apps[tidx].proc_name
                : app->tray_apps[tidx].name;
            set_mode(app, MODE_CLOSED);
            spawn_script(TRAY_LAUNCH_SCRIPT, pname, NULL);
        }
        return true;
    }
    case ACT_TRAY_CTX_CLICK: {
        int cidx = hb->index;
        int tidx = app->tray_ctx_idx;
        if (tidx >= 0 && tidx < app->tray_count && cidx >= 0 && cidx < app->ctx_count) {
            int32_t item_id = app->ctx_items[cidx].id;
            if (item_id >= 0) {
                trigger_dbusmenu_click(app, app->tray_apps[tidx].sni_idx, item_id);
                app->tray_ctx_idx = -1;
                damage_all(app);
            } else if (item_id == -1) {
                const char *pname = app->tray_apps[tidx].proc_name[0]
                    ? app->tray_apps[tidx].proc_name
                    : app->tray_apps[tidx].name;
                set_mode(app, MODE_CLOSED);
                spawn_script(TRAY_LAUNCH_SCRIPT, pname, NULL);
            } else if (item_id == -2) {
                quit_tray_at(app, tidx, "TERM");
            } else if (item_id == -3) {
                quit_tray_at(app, tidx, "KILL");
            }
        }
        return true;
    }
    case ACT_SUB_SELECT: {
        int sidx = hb->index;
        if (sidx >= 0 && sidx < app->sub_count) {
            SubItem *si = &app->sub_items[sidx];
            if (app->subpanel == SUB_WIFI) {
                spawn_script("s=$(printf '%s' \"$1\" | tr '\\036' '\\t'); "
                             "nmcli connection up id \"$s\" >/dev/null 2>&1 || "
                             "nmcli dev wifi connect \"$s\" >/dev/null 2>&1",
                             si->id, NULL);
                snprintf(app->wifi_label, sizeof(app->wifi_label), "%s", si->label);
                for (int k = 0; k < app->sub_count; k++) app->sub_items[k].active = (k == sidx);
            } else if (app->subpanel == SUB_BT) {
                const char *bt_cmd;
                if (si->active) {
                    bt_cmd = "bluetoothctl disconnect \"$1\" >/dev/null 2>&1";
                    si->active = false;
                    strcpy(si->meta, "Paired");
                    strcpy(app->bt_label, "On");
                } else {
                    bt_cmd = "bluetoothctl connect \"$1\" >/dev/null 2>&1";
                    si->active = true;
                    strcpy(si->meta, "Connected");
                    snprintf(app->bt_label, sizeof(app->bt_label), "%s", si->label);
                }
                spawn_script(bt_cmd, si->id, NULL);
            } else if (app->subpanel == SUB_AUDIO) {
                spawn_script("pactl set-default-sink \"$1\" >/dev/null 2>&1", si->id, NULL);
                for (int k = 0; k < app->sub_count; k++) app->sub_items[k].active = (k == sidx);
            }
            damage_all(app);
        }
        return true;
    }
    case ACT_WIFI_TOGGLE:
        if (app->wifi_on) spawn_detached("nmcli radio wifi off");
        else spawn_detached("nmcli radio wifi on");
        app->wifi_on = !app->wifi_on;
        strcpy(app->wifi_label, app->wifi_on ? "On" : "Off");
        if (!app->wifi_on && app->subpanel == SUB_WIFI) app->subpanel = SUB_NONE;
        damage_all(app);
        return true;
    case ACT_BT_TOGGLE:
        if (app->bt_on) spawn_detached("bluetoothctl power off");
        else spawn_detached("bluetoothctl power on");
        app->bt_on = !app->bt_on;
        strcpy(app->bt_label, app->bt_on ? "On" : "Off");
        if (!app->bt_on && app->subpanel == SUB_BT) app->subpanel = SUB_NONE;
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
    case ACT_POWER_PROFILE: {
        if (strcmp(app->power_profile, "none") == 0) {
            spawn_detached("notify-send -a Infynitae 'Power modes' "
                           "'They need power-profiles-daemon, which is not installed'");
            return true;
        }
        const char *next_p = "balanced";
        if (strcmp(app->power_profile, "balanced") == 0) next_p = "performance";
        else if (strcmp(app->power_profile, "performance") == 0) next_p = "power-saver";
        snprintf(app->power_profile, sizeof(app->power_profile), "%s", next_p);
        /* a mode the machine lacks (performance, on many laptops) is refused: said, and the tile
         * shows the real mode again */
        spawn_script("powerprofilesctl set \"$1\" 2>/dev/null || "
                     "notify-send -a Infynitae 'Power modes' \"This machine has no $1 mode\"",
                     next_p, NULL);
        damage_all(app);
        return true;
    }
    case ACT_REPLAY_TOGGLE: {
        app->replay_on = !app->replay_on;
        if (app->replay_on) {
            spawn_detached("sh \"$HOME/infynitae/live/settings.sh\" set ENABLE_REPLAY_BUFFER true >/dev/null 2>&1; sh \"$HOME/infynitae/live/infynitae-reload\" recorder >/dev/null 2>&1");
        } else {
            spawn_detached("sh \"$HOME/infynitae/live/settings.sh\" set ENABLE_REPLAY_BUFFER false >/dev/null 2>&1; sh \"$HOME/infynitae/live/infynitae-reload\" recorder >/dev/null 2>&1");
        }
        damage_all(app);
        return true;
    }
    case ACT_REPLAY_SAVE:
        spawn_detached("~/.local/bin/infynitae-recorder replay-save >/dev/null 2>&1 || ~/infynitae/live/record/infynitae-recorder replay-save >/dev/null 2>&1");
        set_mode(app, MODE_CLOSED);
        return true;
    case ACT_NOTIF_CLEAR:
        app->cleared_notif_id = app->max_notif_id;
        app->notif_count = 0;
        spawn_detached("makoctl dismiss -a 2>/dev/null");
        damage_all(app);
        return true;
    case ACT_NOTIF_ITEM: {
        int nidx = hb->index;
        if (nidx >= 0 && nidx < app->notif_count) {
            char cmd[128];
            snprintf(cmd, sizeof(cmd), "makoctl dismiss -n %u 2>/dev/null", app->notifs[nidx].id);
            spawn_detached(cmd);
            if (app->notifs[nidx].id > app->cleared_notif_id) {
                app->cleared_notif_id = app->notifs[nidx].id;
            }
            for (int k = nidx; k < app->notif_count - 1; k++) {
                app->notifs[k] = app->notifs[k + 1];
            }
            app->notif_count--;
            damage_all(app);
        }
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
        int pct = (int)(((click_x - hb->x) / hb->w) * 100.0 + 0.5);
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
    case ACT_REBOOT:
        /* a second press within 3 s (owner, 2026-11-06: one click used to restart at once) */
        if (app->power_confirm_action != ACT_REBOOT || app->power_confirm_until < now_ns()) {
            app->power_confirm_action = ACT_REBOOT;
            app->power_confirm_until = now_ns() + 3000000000LL;
            damage_all(app);
            return true;
        }
        app->power_confirm_until = 0;
        set_mode(app, MODE_CLOSED);
        spawn_detached(
            "[ -f ~/.config/infynitae/config ] && . ~/.config/infynitae/config 2>/dev/null || true; "
            ": > ~/.local/state/infynitae/stop; "
            "pkill -f 'infynitae restore [w]atch' 2>/dev/null || true; "
            "(~/.local/bin/infynitae restore save 2>/dev/null || infynitae restore save 2>/dev/null || true); "
            "case \"${SHUTDOWN_SCREEN:-true}\" in 0|false|no|off|FALSE|False) ;; *) "
            "rm -f ~/.local/state/infynitae/lock.log; "
            "setsid -f ~/.local/bin/infynitae-lock --status 'Rebooting…' >/dev/null 2>&1; "
            "i=0; while [ $i -lt 15 ]; do "
            "  grep -q 'ready on' ~/.local/state/infynitae/lock.log 2>/dev/null && break; "
            "  sleep 0.05; i=$((i+1)); "
            "done ;; esac; "
            "systemctl reboot");
        return true;
    case ACT_POWEROFF:
        /* a second press within 3 s (owner, 2026-11-06: one click used to restart at once) */
        if (app->power_confirm_action != ACT_POWEROFF || app->power_confirm_until < now_ns()) {
            app->power_confirm_action = ACT_POWEROFF;
            app->power_confirm_until = now_ns() + 3000000000LL;
            damage_all(app);
            return true;
        }
        app->power_confirm_until = 0;
        set_mode(app, MODE_CLOSED);
        spawn_detached(
            "[ -f ~/.config/infynitae/config ] && . ~/.config/infynitae/config 2>/dev/null || true; "
            ": > ~/.local/state/infynitae/stop; "
            "pkill -f 'infynitae restore [w]atch' 2>/dev/null || true; "
            "(~/.local/bin/infynitae restore save 2>/dev/null || infynitae restore save 2>/dev/null || true); "
            "case \"${SHUTDOWN_SCREEN:-true}\" in 0|false|no|off|FALSE|False) ;; *) "
            "rm -f ~/.local/state/infynitae/lock.log; "
            "setsid -f ~/.local/bin/infynitae-lock --status 'Shutting down…' >/dev/null 2>&1; "
            "i=0; while [ $i -lt 15 ]; do "
            "  grep -q 'ready on' ~/.local/state/infynitae/lock.log 2>/dev/null && break; "
            "  sleep 0.05; i=$((i+1)); "
            "done ;; esac; "
            "systemctl poweroff");
        return true;
    case ACT_LOGOUT:
        set_mode(app, MODE_CLOSED);
        spawn_detached(
            "pkill -f 'infynitae restore [w]atch' 2>/dev/null || true; "
            "(~/.local/bin/infynitae restore save 2>/dev/null || infynitae restore save 2>/dev/null || true); "
            "~/.local/bin/infynitae-session stop");
        return true;
    case ACT_CLIP_ITEM: {
        int cidx = hb->index;
        if (cidx >= 0 && cidx < app->clip_count) {
            run_cliphist_with_stdin("cliphist decode 2>/dev/null | wl-copy", app->clip_items[cidx].raw_line);
            set_mode(app, MODE_CLOSED);
        }
        return true;
    }
    case ACT_CLIP_CLEAR:
        app->clip_count = 0;
        app->filtered_clip_count = 0;
        spawn_detached("cliphist wipe 2>/dev/null");
        damage_all(app);
        return true;
    }
    return false;
}

static void activate_selected(EdgeApp *app) {
    /* from the list itself: the hitboxes are from the last frame, and Enter right after typing
     * must start what the search says now */
    if (app->mode == MODE_APPS) {
        int shown = app->filtered_app_count;
        if (app->selected_idx >= 0 && app->selected_idx < shown) {
            launch_app(app, app->filtered_apps[app->selected_idx]);
        }
        return;
    }
    for (int i = 0; i < app->hitbox_count; i++) {
        HitBox *hb = &app->hitboxes[i];
        if (hb->nav_idx == app->selected_idx) {
            execute_hitbox(app, hb, hb->x + hb->w / 2.0, BTN_LEFT);
            return;
        }
    }
}

static bool handle_click_at(EdgeApp *app, EdgeOutput *out, double x, double y, uint32_t button) {
    (void)out;
    for (int i = app->hitbox_count - 1; i >= 0; i--) {
        HitBox *hb = &app->hitboxes[i];
        if (x >= hb->x && x <= hb->x + hb->w && y >= hb->y && y <= hb->y + hb->h) {
            return execute_hitbox(app, hb, x, button);
        }
    }

    if (app->mode != MODE_CLOSED) {
        set_mode(app, MODE_CLOSED);
        return true;
    }
    return false;
}

static int count_nav_items(EdgeApp *app) {
    int max_nav = -1;
    for (int i = 0; i < app->hitbox_count; i++) {
        if (app->hitboxes[i].nav_idx > max_nav) {
            max_nav = app->hitboxes[i].nav_idx;
        }
    }
    return max_nav + 1;
}

/* ========================================================================== */
/* Animation & Wayland Listeners                                              */
/* ========================================================================== */
#define OPEN_MS 70.0
#define CLOSE_MS 55.0

static int64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

/* Moves the progress toward its target by the time since the last step (any screen's), so the
 * open takes the same 70 ms at 60 Hz and at 165 Hz, and two screens don't run it twice as fast.
 * Until 2026-11-06 each frame closed 55% of the gap: about 40 ms at 165 Hz, a pop. Returns
 * whether it is still moving. 120 ms until 2026-11-06 felt slow to the owner ("laggy when called":
 * the frames were all there, 165 Hz, 14 ms from the key): 70 ms open, 55 ms close. */
static bool step_animation(EdgeApp *app) {
    double target = (app->mode == MODE_CLOSED) ? 0.0 : 1.0;
    int64_t now = now_ns();
    double dt_ms = app->anim_last_ns ? (double)(now - app->anim_last_ns) / 1e6 : 1000.0 / 60.0;
    if (dt_ms > 50.0) dt_ms = 50.0;  /* a stalled frame does not jump to the end */
    app->anim_last_ns = now;
    double step = dt_ms / (target > app->anim_progress ? OPEN_MS : CLOSE_MS);
    if (fabs(target - app->anim_progress) <= step) {
        app->anim_progress = target;
        app->anim_last_ns = 0;
        return false;
    }
    app->anim_progress += target > app->anim_progress ? step : -step;
    return true;
}

static void on_frame_done(void *data, struct wl_callback *cb, uint32_t time_ms) {
    (void)time_ms;
    EdgeOutput *out = data;
    if (cb) wl_callback_destroy(cb);
    out->frame_cb = NULL;

    EdgeApp *app = out->app;
    if (app->dragging) {
        render_output(out);
        return;
    }

    if (step_animation(app)) {
        render_output(out);
        request_frame(out);
    } else {
        render_output(out);
    }
}

static const struct wl_callback_listener frame_listener = {
    .done = on_frame_done,
};

static void request_frame(EdgeOutput *out) {
    if (out->frame_cb || !out->configured) return;
    out->frame_cb = wl_surface_frame(out->surface);
    wl_callback_add_listener(out->frame_cb, &frame_listener, out);
    wl_surface_commit(out->surface);
}

static void damage_all(EdgeApp *app) {
    for (EdgeOutput *o = app->outputs; o; o = o->next) {
        render_output(o);
        double target = (app->mode == MODE_CLOSED) ? 0.0 : 1.0;
        if (fabs(target - app->anim_progress) > 0.01) {
            request_frame(o);
        }
    }
}

static EdgeOutput *output_for_surface(EdgeApp *app, struct wl_surface *s) {
    for (EdgeOutput *o = app->outputs; o; o = o->next)
        if (o->surface == s) return o;
    return app->outputs;
}

static void touch_down(void *data, struct wl_touch *touch, uint32_t serial, uint32_t time,
                       struct wl_surface *surface, int32_t id, wl_fixed_t fx, wl_fixed_t fy) {
    (void)touch; (void)serial;
    EdgeApp *app = data;
    EdgeOutput *out = output_for_surface(app, surface);
    double x = wl_fixed_to_double(fx);
    double y = wl_fixed_to_double(fy);

    app->touch_id = id;
    app->touch_down_ms = time;
    app->touch_start_x = x;
    app->touch_start_y = y;
    app->touch_cur_x = x;
    app->touch_cur_y = y;

    if (app->mode == MODE_CLOSED) {
        load_position_config(app);
        app->dragging = true;
        bool in_ctrl_zone = false;
        if (out) {
            in_ctrl_zone = app->control_on_left
                ? (x <= out->width * 0.28)
                : (x >= out->width * 0.72);
        }
        app->target_mode = in_ctrl_zone ? MODE_CONTROL : MODE_CENTER;
        if (app->target_mode == MODE_CENTER) refresh_snapshot(app);
        else refresh_control_data(app);
    }
}

static void touch_motion(void *data, struct wl_touch *touch, uint32_t time, int32_t id,
                         wl_fixed_t fx, wl_fixed_t fy) {
    (void)touch; (void)time;
    EdgeApp *app = data;
    if (id != app->touch_id) return;
    app->touch_cur_x = wl_fixed_to_double(fx);
    app->touch_cur_y = wl_fixed_to_double(fy);

    double raw_dy = app->touch_cur_y - app->touch_start_y;
    double pull_dist = app->bottom_edge ? (-raw_dy) : raw_dy;

    if (app->mode == MODE_CLOSED && app->dragging) {
        double prog = pull_dist / 70.0;
        if (prog < 0.0) prog = 0.0;
        if (prog > 1.0) prog = 1.0;
        app->anim_progress = prog;
        for (EdgeOutput *o = app->outputs; o; o = o->next) request_frame(o);
    } else if (app->mode != MODE_CLOSED && pull_dist < -20.0) {
        app->dragging = true;
        double prog = 1.0 + (pull_dist / 80.0);
        if (prog < 0.0) prog = 0.0;
        if (prog > 1.0) prog = 1.0;
        app->anim_progress = prog;
        for (EdgeOutput *o = app->outputs; o; o = o->next) request_frame(o);
    }
}

static void touch_up(void *data, struct wl_touch *touch, uint32_t serial, uint32_t time, int32_t id) {
    (void)touch; (void)serial;
    EdgeApp *app = data;
    if (id != app->touch_id) return;

    double dx = app->touch_cur_x - app->touch_start_x;
    double raw_dy = app->touch_cur_y - app->touch_start_y;
    double pull_dist = app->bottom_edge ? (-raw_dy) : raw_dy;
    uint32_t held_ms = time - app->touch_down_ms;
    bool was_dragging = app->dragging;
    app->dragging = false;

    if (app->mode == MODE_CLOSED) {
        if (pull_dist > 22.0) {
            app->mode = app->target_mode;
            app->kb_grab = true;
            damage_all(app);
        } else if (hypot(dx, raw_dy) < 16.0) {
            set_mode(app, app->target_mode);
        } else {
            set_mode(app, MODE_CLOSED);
        }
    } else {
        if (was_dragging && pull_dist < -28.0) {
            set_mode(app, MODE_CLOSED);
        } else if (hypot(dx, raw_dy) < 20.0) {
            uint32_t btn = (held_ms >= 350) ? BTN_RIGHT : BTN_LEFT;
            handle_click_at(app, app->outputs, app->touch_cur_x, app->touch_cur_y, btn);
        } else {
            damage_all(app);
        }
    }
}

static void touch_frame(void *data, struct wl_touch *t) { (void)data; (void)t; }
static void touch_cancel(void *data, struct wl_touch *t) {
    (void)t;
    EdgeApp *app = data;
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

static void pointer_enter(void *data, struct wl_pointer *p, uint32_t s,
                          struct wl_surface *surface, wl_fixed_t fx, wl_fixed_t fy) {
    (void)p; (void)s;
    EdgeApp *app = data;
    app->pointer_output = output_for_surface(app, surface);
    app->pointer_x = wl_fixed_to_double(fx);
    app->pointer_y = wl_fixed_to_double(fy);
    if (app->mode != MODE_CLOSED && !app->kb_grab) {
        app->kb_grab = true;
        if (app->mode == MODE_CENTER) refresh_snapshot(app);
        damage_all(app);
    }
}

static void pointer_leave(void *data, struct wl_pointer *p, uint32_t s, struct wl_surface *surface) {
    (void)p; (void)s; (void)surface;
    EdgeApp *app = data;
    if (app->mode != MODE_CLOSED && app->kb_grab) {
        app->kb_grab = false;
        for (EdgeOutput *o = app->outputs; o; o = o->next) {
            update_surface_input_region(o);
            wl_surface_commit(o->surface);
        }
    }
}

static void pointer_motion(void *data, struct wl_pointer *p, uint32_t t, wl_fixed_t fx, wl_fixed_t fy) {
    (void)p; (void)t;
    EdgeApp *app = data;
    app->pointer_x = wl_fixed_to_double(fx);
    app->pointer_y = wl_fixed_to_double(fy);

    if (app->mode == MODE_CLOSED) return;

    for (int i = app->hitbox_count - 1; i >= 0; i--) {
        HitBox *hb = &app->hitboxes[i];
        if (hb->nav_idx >= 0 &&
            app->pointer_x >= hb->x && app->pointer_x <= hb->x + hb->w &&
            app->pointer_y >= hb->y && app->pointer_y <= hb->y + hb->h) {
            if (app->selected_idx != hb->nav_idx) {
                app->selected_idx = hb->nav_idx;
                damage_all(app);
            }
            break;
        }
    }
}

static void pointer_button(void *data, struct wl_pointer *p, uint32_t s, uint32_t t,
                           uint32_t button, uint32_t state) {
    (void)p; (void)s; (void)t;
    EdgeApp *app = data;
    if (state != WL_POINTER_BUTTON_STATE_PRESSED) return;
    if (app->mode == MODE_CLOSED) {
        EdgeOutput *out = app->pointer_output ? app->pointer_output : app->outputs;
        bool in_ctrl_zone = false;
        if (out) {
            in_ctrl_zone = app->control_on_left
                ? (app->pointer_x <= out->width * 0.28)
                : (app->pointer_x >= out->width * 0.72);
        }
        set_mode(app, in_ctrl_zone ? MODE_CONTROL : MODE_CENTER);
        return;
    }
    handle_click_at(app, app->pointer_output, app->pointer_x, app->pointer_y, button);
}

static void pointer_axis(void *data, struct wl_pointer *p, uint32_t t, uint32_t axis, wl_fixed_t val) {
    (void)p; (void)t;
    EdgeApp *app = data;
    if (app->mode != MODE_CONTROL || axis != WL_POINTER_AXIS_VERTICAL_SCROLL) return;
    double v = wl_fixed_to_double(val);
    if (v < -0.5) {
        app->volume_pct = (app->volume_pct <= 95) ? app->volume_pct + 5 : 100;
        app->volume_muted = false;
        spawn_detached("pactl set-sink-volume @DEFAULT_SINK@ +5% && pactl set-sink-mute @DEFAULT_SINK@ 0");
        damage_all(app);
    } else if (v > 0.5) {
        app->volume_pct = (app->volume_pct >= 5) ? app->volume_pct - 5 : 0;
        spawn_detached("pactl set-sink-volume @DEFAULT_SINK@ -5%");
        damage_all(app);
    }
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

static void keyboard_keymap(void *data, struct wl_keyboard *kb, uint32_t format, int32_t fd, uint32_t size) {
    (void)kb;
    EdgeApp *app = data;
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
    EdgeApp *app = data;
    if (state != WL_KEYBOARD_KEY_STATE_PRESSED || !app->xkb_state || app->mode == MODE_CLOSED) return;

    uint32_t keycode = key + 8;
    xkb_keysym_t sym = xkb_state_key_get_one_sym(app->xkb_state, keycode);
    bool super_down = xkb_state_mod_name_is_active(app->xkb_state, XKB_MOD_NAME_LOGO, XKB_STATE_MODS_EFFECTIVE) > 0;
    bool ctrl_down = xkb_state_mod_name_is_active(app->xkb_state, XKB_MOD_NAME_CTRL, XKB_STATE_MODS_EFFECTIVE) > 0;
    bool shift_down = xkb_state_mod_name_is_active(app->xkb_state, XKB_MOD_NAME_SHIFT, XKB_STATE_MODS_EFFECTIVE) > 0;

    if (sym == XKB_KEY_Escape) {
        if (app->tray_ctx_idx >= 0) {
            app->tray_ctx_idx = -1;
            damage_all(app);
        } else if (app->subpanel != SUB_NONE) {
            app->subpanel = SUB_NONE;
            damage_all(app);
        } else {
            set_mode(app, MODE_CLOSED);
        }
        return;
    }

    if (super_down) {
        if (sym == XKB_KEY_w || sym == XKB_KEY_W) {
            set_mode(app, app->mode == MODE_APPS ? MODE_CLOSED : MODE_APPS);
            return;
        }
        if (sym == XKB_KEY_l || sym == XKB_KEY_L) {
            set_mode(app, app->mode == MODE_CENTER ? MODE_CLOSED : MODE_CENTER);
            return;
        }
        if (sym == XKB_KEY_a || sym == XKB_KEY_A) {
            set_mode(app, app->mode == MODE_CONTROL ? MODE_CLOSED : MODE_CONTROL);
            return;
        }
        if (sym == XKB_KEY_v || sym == XKB_KEY_V) {
            set_mode(app, app->mode == MODE_CLIPBOARD ? MODE_CLOSED : MODE_CLIPBOARD);
            return;
        }
        if ((shift_down && (sym == XKB_KEY_c || sym == XKB_KEY_C)) || sym == XKB_KEY_q || sym == XKB_KEY_Q) {
            close_selected_item(app);
            return;
        }
    }
    if (sym == XKB_KEY_Delete || (ctrl_down && (sym == XKB_KEY_w || sym == XKB_KEY_W))) {
        close_selected_item(app);
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

    int total = count_nav_items(app);

    /* the lists (launcher, windows, clipboard): one column, so Up and Down move along it (on a
     * panel at the bottom the list grows upward from the search field: Up goes away from it),
     * Tab and Shift+Tab too, Page Up and Page Down by a screenful; Left and Right do nothing */
    if (app->mode == MODE_APPS || app->mode == MODE_CENTER || app->mode == MODE_CLIPBOARD) {
        int count = app->mode == MODE_APPS ? app->filtered_app_count
                  : app->mode == MODE_CENTER ? app->filtered_win_count : app->filtered_clip_count;
        int step = 0;
        bool away = app->bottom_edge ? sym == XKB_KEY_Up : sym == XKB_KEY_Down;
        bool toward = app->bottom_edge ? sym == XKB_KEY_Down : sym == XKB_KEY_Up;
        if (away || sym == XKB_KEY_Tab) step = 1;
        else if (toward || sym == XKB_KEY_ISO_Left_Tab) step = -1;
        else if (sym == XKB_KEY_Page_Down || sym == XKB_KEY_Next) step = app->bottom_edge ? -LIST_ROWS : LIST_ROWS;
        else if (sym == XKB_KEY_Page_Up || sym == XKB_KEY_Prior) step = app->bottom_edge ? LIST_ROWS : -LIST_ROWS;
        else if (sym == XKB_KEY_Left || sym == XKB_KEY_Right) return;
        if (step != 0) {
            if (count > 0) {
                int next = app->selected_idx + step;
                if (step == 1 || step == -1) next = (next + count) % count;  /* single steps wrap */
                else if (next < 0) next = 0;
                else if (next >= count) next = count - 1;
                app->selected_idx = next;
                list_follow(app, count);
            }
            damage_all(app);
            return;
        }
    }

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
        if (app->mode == MODE_CENTER) {
            /* one row of windows: Left and Right move along it */
            return;
        }
        if (app->mode == MODE_CONTROL && app->selected_idx < 2 && app->subpanel == SUB_NONE) {
            toggle_subpanel(app, app->selected_idx == 0 ? SUB_WIFI : SUB_BT);
            return;
        } else if (total > 0) {
            int step = (app->mode == MODE_CLIPBOARD || (app->mode == MODE_CONTROL && app->selected_idx >= 6)) ? 1 : 2;
            int next = app->selected_idx + step;
            app->selected_idx = (next < total) ? next : total - 1;
        }
        damage_all(app);
        return;
    }
    if (sym == XKB_KEY_Up) {
        if (app->mode != MODE_CENTER && total > 0) {
            int step = (app->mode == MODE_CLIPBOARD || (app->mode == MODE_CONTROL && app->selected_idx > 6)) ? 1 : 2;
            int prev = app->selected_idx - step;
            app->selected_idx = (prev >= 0) ? prev : 0;
        }
        damage_all(app);
        return;
    }

    if (super_down || ctrl_down) return;

    char utf8[16] = {0};
    int len = xkb_state_key_get_utf8(app->xkb_state, keycode, utf8, sizeof(utf8));
    if (len > 0 && (unsigned char)utf8[0] >= 32 && (unsigned char)utf8[0] != 127) {
        /* typing in the control panel looks for an app */
        if (app->mode == MODE_CONTROL) {
            app->mode = MODE_APPS;
            app->target_mode = MODE_APPS;
            app->subpanel = SUB_NONE;
            load_all_apps(app);
        }
        if (app->search_len + (size_t)len < sizeof(app->search) - 1) {
            memcpy(app->search + app->search_len, utf8, (size_t)len);
            app->search_len += (size_t)len;
            app->search[app->search_len] = '\0';
            update_filters(app);
            if (app->mode == MODE_CLIPBOARD) {
                app->selected_idx = 0;
            } else {
                app->selected_idx = 0;
            }
            damage_all(app);
        }
    }
}

static void keyboard_modifiers(void *data, struct wl_keyboard *kb, uint32_t s,
                               uint32_t dep, uint32_t lat, uint32_t lock, uint32_t grp) {
    (void)kb; (void)s;
    EdgeApp *app = data;
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
    EdgeApp *app = data;
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

static void layer_surface_configure(void *data, struct zwlr_layer_surface_v1 *ls,
                                    uint32_t serial, uint32_t width, uint32_t height) {
    EdgeOutput *out = data;
    zwlr_layer_surface_v1_ack_configure(ls, serial);
    if (width > 0) out->width = (int32_t)width;
    if (height > 0) out->height = (int32_t)height;
    out->configured = true;
    render_output(out);
    /* remapped mid-animation (opened over a fullscreen window): keep animating */
    EdgeApp *app = out->app;
    double target = (app->mode == MODE_CLOSED) ? 0.0 : 1.0;
    if (app->dragging || fabs(target - app->anim_progress) > 0.01) request_frame(out);
}

static void layer_surface_closed(void *data, struct zwlr_layer_surface_v1 *ls) {
    (void)data; (void)ls;
}

static const struct zwlr_layer_surface_v1_listener layer_surface_listener = {
    .configure = layer_surface_configure,
    .closed = layer_surface_closed,
};

static void setup_layer_surface(EdgeOutput *out) {
    EdgeApp *app = out->app;
    if (!app->layer_shell || !app->compositor || out->layer_surface) return;

    out->surface = wl_compositor_create_surface(app->compositor);
    out->layer_surface = zwlr_layer_shell_v1_get_layer_surface(
        app->layer_shell, out->surface, out->wl_output,
        ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY, "infynitae-edge");

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

/* ========================================================================== */
/* Fullscreen windows (wlr-foreign-toplevel): the edge unmaps over them      */
/* ========================================================================== */
static bool output_has_fullscreen(EdgeOutput *out) {
    for (FsToplevel *t = out->app->toplevels; t; t = t->next) {
        if (!t->fullscreen) continue;
        for (int i = 0; i < t->n_outputs; i++)
            if (t->outputs[i] == out->wl_output) return true;
    }
    return false;
}

static void fs_title(void *d, struct zwlr_foreign_toplevel_handle_v1 *h, const char *s) {
    (void)d; (void)h; (void)s;
}
static void fs_app_id(void *d, struct zwlr_foreign_toplevel_handle_v1 *h, const char *s) {
    (void)d; (void)h; (void)s;
}
static void fs_output_enter(void *d, struct zwlr_foreign_toplevel_handle_v1 *h,
                            struct wl_output *o) {
    (void)h;
    FsToplevel *t = d;
    for (int i = 0; i < t->n_outputs; i++)
        if (t->outputs[i] == o) return;
    if (t->n_outputs < FS_MAX_OUTPUTS) t->outputs[t->n_outputs++] = o;
}
static void fs_output_leave(void *d, struct zwlr_foreign_toplevel_handle_v1 *h,
                            struct wl_output *o) {
    (void)h;
    FsToplevel *t = d;
    for (int i = 0; i < t->n_outputs; i++) {
        if (t->outputs[i] == o) {
            t->outputs[i] = t->outputs[--t->n_outputs];
            return;
        }
    }
}
static void fs_state(void *d, struct zwlr_foreign_toplevel_handle_v1 *h, struct wl_array *st) {
    (void)h;
    FsToplevel *t = d;
    t->pending_fullscreen = false;
    uint32_t *s;
    wl_array_for_each(s, st) {
        /* maximized counts: the compositor also says so of a window as large as the view. The
         * strips must not take a click near the screen's edge from an app that fills it */
        if (*s == ZWLR_FOREIGN_TOPLEVEL_HANDLE_V1_STATE_FULLSCREEN ||
            *s == ZWLR_FOREIGN_TOPLEVEL_HANDLE_V1_STATE_MAXIMIZED) t->pending_fullscreen = true;
    }
}

static EdgeApp *fs_app; /* the one app, for the toplevel callbacks */

static void fs_changed(EdgeApp *app) {
    for (EdgeOutput *o = app->outputs; o; o = o->next) render_output(o);
}
static void fs_done(void *d, struct zwlr_foreign_toplevel_handle_v1 *h) {
    (void)h;
    FsToplevel *t = d;
    /* output enter/leave is applied at once; a change there or of the state redraws */
    t->fullscreen = t->pending_fullscreen;
    if (fs_app) fs_changed(fs_app);
}
static void fs_closed(void *d, struct zwlr_foreign_toplevel_handle_v1 *h) {
    FsToplevel *t = d;
    if (fs_app) {
        for (FsToplevel **p = &fs_app->toplevels; *p; p = &(*p)->next) {
            if (*p == t) {
                *p = t->next;
                break;
            }
        }
    }
    zwlr_foreign_toplevel_handle_v1_destroy(h);
    bool was = t->fullscreen;
    free(t);
    if (was && fs_app) fs_changed(fs_app);
}

static const struct zwlr_foreign_toplevel_handle_v1_listener fs_handle_listener = {
    .title = fs_title,
    .app_id = fs_app_id,
    .output_enter = fs_output_enter,
    .output_leave = fs_output_leave,
    .state = fs_state,
    .done = fs_done,
    .closed = fs_closed,
};

static void fs_toplevel(void *d, struct zwlr_foreign_toplevel_manager_v1 *m,
                        struct zwlr_foreign_toplevel_handle_v1 *h) {
    (void)m;
    EdgeApp *app = d;
    FsToplevel *t = calloc(1, sizeof(FsToplevel));
    if (!t) return;
    t->handle = h;
    t->next = app->toplevels;
    app->toplevels = t;
    zwlr_foreign_toplevel_handle_v1_add_listener(h, &fs_handle_listener, t);
}
static void fs_finished(void *d, struct zwlr_foreign_toplevel_manager_v1 *m) {
    (void)d; (void)m;
}
static const struct zwlr_foreign_toplevel_manager_v1_listener fs_manager_listener = {
    .toplevel = fs_toplevel,
    .finished = fs_finished,
};

static void registry_global(void *data, struct wl_registry *reg, uint32_t name,
                            const char *interface, uint32_t version) {
    (void)version;
    EdgeApp *app = data;
    if (strcmp(interface, zwlr_foreign_toplevel_manager_v1_interface.name) == 0) {
        /* v2: the fullscreen state; v3's parent event is not listened to */
        fs_app = app;
        app->toplevel_manager = wl_registry_bind(
            reg, name, &zwlr_foreign_toplevel_manager_v1_interface, version < 2 ? version : 2);
        zwlr_foreign_toplevel_manager_v1_add_listener(app->toplevel_manager,
                                                      &fs_manager_listener, app);
    } else if (strcmp(interface, wl_compositor_interface.name) == 0) {
        app->compositor = wl_registry_bind(reg, name, &wl_compositor_interface, 4);
    } else if (strcmp(interface, wl_shm_interface.name) == 0) {
        app->shm = wl_registry_bind(reg, name, &wl_shm_interface, 1);
    } else if (strcmp(interface, zwlr_layer_shell_v1_interface.name) == 0) {
        app->layer_shell = wl_registry_bind(reg, name, &zwlr_layer_shell_v1_interface, 1);
    } else if (strcmp(interface, wl_seat_interface.name) == 0) {
        app->seat = wl_registry_bind(reg, name, &wl_seat_interface, 5);
        wl_seat_add_listener(app->seat, &seat_listener, app);
    } else if (strcmp(interface, wl_output_interface.name) == 0) {
        EdgeOutput *out = calloc(1, sizeof(EdgeOutput));
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
    EdgeApp *app = data;
    EdgeOutput **curr = &app->outputs;
    while (*curr) {
        if ((*curr)->registry_name == name) {
            EdgeOutput *rm = *curr;
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

static struct wl_display *connect_wayland_auto(void) {
    struct wl_display *d = wl_display_connect(NULL);
    if (d) return d;
    const char *rt = getenv("XDG_RUNTIME_DIR");
    if (!rt) rt = "/run/user/1000";
    for (int i = 9; i >= 0; i--) {
        char sock[64], full[256];
        snprintf(sock, sizeof(sock), "wayland-%d", i);
        snprintf(full, sizeof(full), "%s/%s", rt, sock);
        struct stat st;
        if (stat(full, &st) == 0) {
            setenv("WAYLAND_DISPLAY", sock, 1);
            d = wl_display_connect(sock);
            if (d) return d;
        }
    }
    return NULL;
}

/* The edge the pid file names, or 0: a pid file left by an edge that crashed may name another
 * process by now, which a signal meant for the edge would end (SIGUSR1 terminates by default). */
static pid_t edge_pid(const char *pfile) {
    pid_t pid = 0;
    FILE *pf = fopen(pfile, "r");
    if (!pf) return 0;
    if (fscanf(pf, "%d", &pid) != 1) pid = 0;
    fclose(pf);
    if (pid <= 0 || pid == getpid()) return 0;
    char path[64], comm[32] = {0};
    snprintf(path, sizeof(path), "/proc/%d/comm", (int)pid);
    FILE *cf = fopen(path, "r");
    if (!cf) return 0;
    if (!fgets(comm, sizeof(comm), cf)) comm[0] = '\0';
    fclose(cf);
    comm[strcspn(comm, "\n")] = '\0';
    return strcmp(comm, "infynitae-edge") == 0 ? pid : 0;
}

/* --render MODE WxH OUT.png: one panel fully open, with fixed made-up content, into a PNG, through
 * the same drawing code as the live panels; connects to nothing, signals nothing, starts nothing
 * (tools/test-edge-render). MODE: apps, windows, control or clipboard. */
static int render_to_png(const char *mode_name, const char *size, const char *out_path) {
    int w = 0, h = 0;
    if (sscanf(size, "%dx%d", &w, &h) != 2 || w < 200 || h < 200 || w > 8192 || h > 8192) {
        fprintf(stderr, "[infynitae-edge] --render needs WxH, e.g. 1920x1080\n");
        return 2;
    }
    static EdgeApp app;
    memset(&app, 0, sizeof(app));
    app.async_fd = -1;
    app.tray_ctx_idx = -1;
    load_position_config(&app);

    static const char *names[][2] = {
        {"Firefox", "Web Browser"}, {"Files", "File Manager"}, {"Terminal", "Terminal Emulator"},
        {"Steam", "Games"}, {"Discord", "Chat"}, {"Text Editor", "Editor"}, {"Settings", "System"},
        {"Image Viewer", "Graphics"}, {"Music", "Audio Player"}, {"Calculator", "Accessories"},
        {"System Monitor", "System"}, {"Videos", "Video Player"},
    };
    app.app_count = (int)(sizeof(names) / sizeof(names[0]));
    for (int i = 0; i < app.app_count; i++) {
        snprintf(app.apps[i].id, sizeof(app.apps[i].id), "app%d.desktop", i);
        snprintf(app.apps[i].name, sizeof(app.apps[i].name), "%s", names[i][0]);
        snprintf(app.apps[i].generic, sizeof(app.apps[i].generic), "%s", names[i][1]);
    }
    static const char *wins[][2] = {
        {"firefox", "Infynitae - GitHub - Mozilla Firefox"}, {"kitty", "~/infynitae: cargo test"},
        {"discord", "Discord"}, {"steam", "Steam"}, {"thunar", "Downloads"}, {"mousepad", "notes.txt"},
        {"cs2", "Counter-Strike 2"}, {"mpv", "video.mkv"}, {"gimp", "GNU Image Manipulation Program"},
        {"btop", "btop"},
    };
    app.win_count = (int)(sizeof(wins) / sizeof(wins[0]));
    for (int i = 0; i < app.win_count; i++) {
        snprintf(app.windows[i].app_id, sizeof(app.windows[i].app_id), "%s", wins[i][0]);
        snprintf(app.windows[i].title, sizeof(app.windows[i].title), "%s", wins[i][1]);
    }
    app.windows[1].focused = true;
    app.windows[3].attention = true;
    static const char *clips[] = {"git push origin main", "https://github.com/vysaely/infynitae",
                                  "The quick brown fox jumps over the lazy dog", "192.168.1.20", "sudo pacman -Syu"};
    app.clip_count = 5;
    for (int i = 0; i < app.clip_count; i++) snprintf(app.clip_items[i].preview, sizeof(app.clip_items[i].preview), "%s", clips[i]);
    app.wifi_on = true;
    snprintf(app.wifi_label, sizeof(app.wifi_label), "HomeNet");
    app.bt_on = true;
    snprintf(app.bt_label, sizeof(app.bt_label), "Headphones");
    app.volume_pct = 60;
    app.battery_pct = -1;
    snprintf(app.power_profile, sizeof(app.power_profile), "%s", getenv("INFYNITAE_EDGE_RENDER_POWER") ? getenv("INFYNITAE_EDGE_RENDER_POWER") : "balanced");
    static const char *trays[][2] = {{"Steam", "2159 MB"}, {"RustDesk", "127 MB"}, {"Gameoverlayui", "66 MB"}};
    app.tray_count = 3;
    for (int i = 0; i < 3; i++) {
        snprintf(app.tray_apps[i].name, sizeof(app.tray_apps[i].name), "%s", trays[i][0]);
        snprintf(app.tray_apps[i].mem, sizeof(app.tray_apps[i].mem), "%s", trays[i][1]);
    }
    static const char *notes[][2] = {{"Discord", "New message from a friend"}, {"Steam", "Download complete"},
                                     {"Infynitae", "Screen is being captured — The red dot is shown while an app records the screen"}};
    app.notif_count = 3;
    for (int i = 0; i < 3; i++) {
        app.notifs[i].id = (uint32_t)(i + 1);
        snprintf(app.notifs[i].app, sizeof(app.notifs[i].app), "%s", notes[i][0]);
        snprintf(app.notifs[i].text, sizeof(app.notifs[i].text), "%s", notes[i][1]);
    }

    EdgeMode mode;
    if (strcmp(mode_name, "apps") == 0) mode = MODE_APPS;
    else if (strcmp(mode_name, "windows") == 0) mode = MODE_CENTER;
    else if (strcmp(mode_name, "control") == 0) mode = MODE_CONTROL;
    else if (strcmp(mode_name, "clipboard") == 0) mode = MODE_CLIPBOARD;
    else {
        fprintf(stderr, "[infynitae-edge] --render: apps, windows, control or clipboard\n");
        return 2;
    }
    app.mode = app.target_mode = mode;
    app.anim_progress = 1.0;
    app.selected_idx = 1;
    if (mode == MODE_CONTROL) app.selected_idx = 0;
    update_filters(&app);

    cairo_surface_t *surf = cairo_image_surface_create(CAIRO_FORMAT_ARGB32, w, h);
    cairo_t *cr = cairo_create(surf);
    /* a plain desktop behind, so the panel's edges show */
    cairo_set_source_rgb(cr, 0.16, 0.17, 0.20);
    cairo_paint(cr);
    PangoLayout *layout = pango_cairo_create_layout(cr);
    int rect[4];
    draw_panels(&app, cr, layout, w, h, rect);
    g_object_unref(layout);
    cairo_destroy(cr);
    cairo_status_t st = cairo_surface_write_to_png(surf, out_path);
    cairo_surface_destroy(surf);
    return st == CAIRO_STATUS_SUCCESS ? 0 : 1;
}

int main(int argc, char **argv) {
    if (argc == 5 && strcmp(argv[1], "--render") == 0) return render_to_png(argv[2], argv[3], argv[4]);
    const char *pfile = pid_file_path();

    if (argc > 1) {
        pid_t running_pid = edge_pid(pfile);
        if (strcmp(argv[1], "toggle") == 0 || strcmp(argv[1], "center") == 0) {
            if (running_pid > 0 && kill(running_pid, SIGUSR1) == 0) return 0;
        } else if (strcmp(argv[1], "control") == 0) {
            if (running_pid > 0 && kill(running_pid, SIGUSR2) == 0) return 0;
        } else if (strcmp(argv[1], "clipboard") == 0) {
            if (running_pid > 0 && kill(running_pid, SIGRTMIN) == 0) return 0;
        } else if (strcmp(argv[1], "apps") == 0) {
            if (running_pid > 0 && kill(running_pid, SIGRTMIN + 1) == 0) return 0;
        } else if (strcmp(argv[1], "stop") == 0) {
            if (running_pid > 0) kill(running_pid, SIGTERM);
            return 0;
        }
    }

    pid_t old_pid = edge_pid(pfile);
    if (old_pid > 0) {
        kill(old_pid, SIGTERM);
        usleep(50000);
    }

    FILE *pf = fopen(pfile, "w");
    if (pf) {
        fprintf(pf, "%d\n", getpid());
        fclose(pf);
    }

    if (pipe2(g_wake_pipe, O_NONBLOCK | O_CLOEXEC) < 0) g_wake_pipe[0] = g_wake_pipe[1] = -1;
    signal(SIGUSR1, on_sigusr1);
    signal(SIGUSR2, on_sigusr2);
    signal(SIGRTMIN, on_sigrtmin);
    signal(SIGRTMIN + 1, on_sigapps);
    signal(SIGTERM, on_sigterm);
    signal(SIGINT, on_sigterm);
    reap_children_without_waiting();

    EdgeApp app = {0};
    app.running = true;
    app.mode = MODE_CLOSED;
    app.target_mode = MODE_CENTER;
    app.touch_id = -1;
    app.tray_ctx_idx = -1;
    app.async_fd = -1;
    app.volume_pct = 75;
    app.battery_pct = -1;
    strcpy(app.power_profile, "balanced");
    strcpy(app.wifi_label, "On");
    strcpy(app.bt_label, "Off");

    load_position_config(&app);
    load_all_apps(&app);
    setup_sni_watcher(&app);

    app.xkb_ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    app.display = connect_wayland_auto();
    if (!app.display) {
        fprintf(stderr, "[infynitae-edge] Cannot connect to Wayland display\n");
        return 1;
    }

    app.registry = wl_display_get_registry(app.display);
    wl_registry_add_listener(app.registry, &registry_listener, &app);
    wl_display_roundtrip(app.display);
    for (EdgeOutput *o = app.outputs; o; o = o->next) setup_layer_surface(o);
    wl_display_roundtrip(app.display);

    start_async_refresh(&app, MODE_CONTROL);

    if (argc > 1 && (strcmp(argv[1], "toggle") == 0 || strcmp(argv[1], "center") == 0 || strcmp(argv[1], "open") == 0)) {
        set_mode(&app, MODE_CENTER);
    } else if (argc > 1 && strcmp(argv[1], "control") == 0) {
        set_mode(&app, MODE_CONTROL);
    } else if (argc > 1 && strcmp(argv[1], "clipboard") == 0) {
        set_mode(&app, MODE_CLIPBOARD);
    } else if (argc > 1 && strcmp(argv[1], "apps") == 0) {
        set_mode(&app, MODE_APPS);
    }

    GMainContext *glib_ctx = g_main_context_default();
    g_main_context_acquire(glib_ctx);
    while (app.running) {
        if (g_sig_cmd == 1) {
            g_sig_cmd = 0;
            set_mode(&app, app.mode == MODE_CENTER ? MODE_CLOSED : MODE_CENTER);
        } else if (g_sig_cmd == 2) {
            g_sig_cmd = 0;
            set_mode(&app, app.mode == MODE_CONTROL ? MODE_CLOSED : MODE_CONTROL);
        } else if (g_sig_cmd == 4) {
            g_sig_cmd = 0;
            set_mode(&app, app.mode == MODE_CLIPBOARD ? MODE_CLOSED : MODE_CLIPBOARD);
        } else if (g_sig_cmd == 5) {
            g_sig_cmd = 0;
            set_mode(&app, app.mode == MODE_APPS ? MODE_CLOSED : MODE_APPS);
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

        /* GLib's sources (D-Bus: the tray's icons and menus) are polled here with ours: GDBus reads
         * its socket on a thread of its own and wakes the main context through a fd of GLib's, so
         * polling the socket would see a signal before it can be dispatched, or not at all */
        enum { MAX_GLIB_FDS = 16 };
        gint glib_prio = 0, glib_timeout = -1;
        g_main_context_prepare(glib_ctx, &glib_prio);
        GPollFD gfds[MAX_GLIB_FDS];
        gint n_glib = g_main_context_query(glib_ctx, glib_prio, &glib_timeout, gfds, MAX_GLIB_FDS);
        if (n_glib > MAX_GLIB_FDS) n_glib = MAX_GLIB_FDS;

        struct pollfd pfds[3 + MAX_GLIB_FDS];
        pfds[0].fd = wl_display_get_fd(app.display);
        pfds[0].events = POLLIN;
        pfds[0].revents = 0;
        nfds_t nfds = 1;

        if (app.async_fd >= 0) {
            pfds[nfds].fd = app.async_fd;
            pfds[nfds].events = POLLIN | POLLHUP;
            pfds[nfds].revents = 0;
            nfds++;
        }

        nfds_t glib_first = nfds;
        for (gint i = 0; i < n_glib; i++) {
            pfds[nfds].fd = gfds[i].fd;
            pfds[nfds].events = gfds[i].events;
            pfds[nfds].revents = 0;
            nfds++;
        }

        nfds_t wake_idx = nfds;
        if (g_wake_pipe[0] >= 0) {
            pfds[nfds].fd = g_wake_pipe[0];
            pfds[nfds].events = POLLIN;
            pfds[nfds].revents = 0;
            nfds++;
        }

        int timeout = (app.mode != MODE_CLOSED) ? 1000 : -1;
        if (glib_timeout >= 0 && (timeout < 0 || glib_timeout < timeout)) timeout = glib_timeout;
        int ret = poll(pfds, nfds, timeout);
        for (gint i = 0; i < n_glib; i++) gfds[i].revents = pfds[glib_first + i].revents;
        if (g_wake_pipe[0] >= 0 && (pfds[wake_idx].revents & POLLIN)) {
            char drain[64];
            while (read(g_wake_pipe[0], drain, sizeof(drain)) > 0) {}
        }
        if (ret > 0 && (pfds[0].revents & POLLIN)) {
            if (wl_display_read_events(app.display) < 0 ||
                wl_display_dispatch_pending(app.display) < 0) {
                break;
            }
        } else {
            wl_display_cancel_read(app.display);
        }

        if (app.async_fd >= 0 && (pfds[1].revents & (POLLIN | POLLHUP))) {
            ssize_t r;
            while ((r = read(app.async_fd, app.async_buf + app.async_len,
                             sizeof(app.async_buf) - 1 - app.async_len)) > 0) {
                app.async_len += (size_t)r;
            }
            if (r == 0 || (r < 0 && errno != EAGAIN && errno != EWOULDBLOCK)) {
                app.async_buf[app.async_len] = '\0';
                close(app.async_fd);
                app.async_fd = -1;
                if (app.async_kind == MODE_CENTER) {
                    parse_snapshot_buf(&app, app.async_buf);
                } else if (app.async_kind == MODE_CONTROL) {
                    parse_control_buf(&app, app.async_buf);
                } else if (app.async_kind == 3) {
                    parse_subpanel_buf(&app, app.async_buf);
                } else if (app.async_kind == 4) {
                    parse_clipboard_buf(&app, app.async_buf);
                }
                if (app.mode != MODE_CLOSED) {
                    damage_all(&app);
                }
            }
        }

        if (g_main_context_check(glib_ctx, glib_prio, gfds, n_glib)) g_main_context_dispatch(glib_ctx);
    }

    unlink(pfile);
    wl_display_disconnect(app.display);
    return 0;
}
