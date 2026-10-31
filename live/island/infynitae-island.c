/*
 * Infynitae Native Taskbar (Super+W) & System Control / StatusNotifier Tray (Super+A)
 *
 * Barebones, monochrome obsidian geometry (#0b0f14 background, 2px sharp corners,
 * 1px subtle border, zero nested buttons, zero visual bloat):
 *   - Position (`ISLAND_EDGE="bottom|top"`, `ISLAND_CONTROL_SIDE="right|left"`) is read
 *     strictly from `~/.config/infynitae/config` (no UI buttons for it).
 *   - Non-blocking input region: only the drawn bar/panel captures pointer events;
 *     clicking/panning the canvas works freely while open.
 *   - Super+C / Delete / Middle-Click closes the selected/hovered window (in Super+W)
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

#include "../wallpaper/wlr-layer-shell-protocol.h"
#include "wlr-foreign-toplevel-protocol.h"

#define MAX_WINDOWS 24
#define MAX_APPS 160
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
    MODE_CENTER = 1,    /* Super+W: Search + Open Window Tabs */
    MODE_CONTROL = 2,   /* Super+A: System Control + Tray Apps */
    MODE_CLIPBOARD = 3  /* Super+V: Clipboard History */
} IslandMode;

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
} CanvasWindow;

typedef struct {
    char name[64];
    char exec_cmd[256];
} DesktopApp;

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

    int prev_dx, prev_dy, prev_dw, prev_dh;
    int input_x, input_y, input_w, input_h;

    /* Closed over a fullscreen window: no buffer at all, so the window's frames go to the
     * screen as they are (direct scanout, tearing) and the edge strips take no clicks */
    bool unmapped;

    struct IslandOutput *next;
} IslandOutput;

/* A toplevel as wlr-foreign-toplevel reports it: fullscreen, and on which outputs */
#define FS_MAX_OUTPUTS 8
typedef struct FsToplevel {
    struct zwlr_foreign_toplevel_handle_v1 *handle;
    bool fullscreen, pending_fullscreen;
    struct wl_output *outputs[FS_MAX_OUTPUTS];
    int n_outputs;
    struct FsToplevel *next;
} FsToplevel;

typedef struct IslandApp {
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

    IslandOutput *outputs;

    IslandMode mode;
    IslandMode target_mode;
    ControlSubpanel subpanel;
    bool kb_grab;
    double anim_progress;
    bool dragging;
    int32_t touch_id;
    uint32_t touch_down_ms;
    double touch_start_x;
    double touch_start_y;
    double touch_cur_x;
    double touch_cur_y;

    double pointer_x;
    double pointer_y;
    IslandOutput *pointer_output;

    char search[128];
    size_t search_len;
    bool launcher_open;
    int selected_idx;

    CanvasWindow windows[MAX_WINDOWS];
    int win_count;
    int filtered_wins[MAX_WINDOWS];
    int filtered_win_count;

    DesktopApp apps[MAX_APPS];
    int app_count;
    int filtered_apps[MAX_APPS];
    int filtered_app_count;

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
} IslandApp;

enum {
    ACT_NONE = 0,
    ACT_SEARCH_BOX,
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
    ACT_CORNER_EDGE
};

static volatile sig_atomic_t g_sig_cmd = 0;

/* The signals (Super+W, Super+A, Super+V) reach any thread that does not block them, GLib's own
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

static void request_frame(IslandOutput *out);
static void damage_all(IslandApp *app);
static void set_mode(IslandApp *app, IslandMode new_mode);

/* ========================================================================== */
/* Helpers                                                                    */
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

/* `script` run by sh with `a1` and `a2` as "$1" and "$2": a name from outside (a Wi-Fi network's,
 * a Bluetooth device's, an app's) is never read as shell code, whatever quotes it holds. */
/* A tray app's window, opened again: its program by its name (lowercased), else its desktop file */
#define TRAY_LAUNCH_SCRIPT \
    "app=$(printf '%s' \"$1\" | tr '[:upper:]' '[:lower:]'); " \
    "command -v \"$app\" >/dev/null 2>&1 && exec \"$app\" || gtk-launch \"$app\" 2>/dev/null"

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

static void sni_fetch_item_props(IslandApp *app, SniItem *item) {
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
    IslandApp *app = user_data;

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
    IslandApp *app = user_data;

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

static void setup_sni_watcher(IslandApp *app) {
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

static int find_sni_for_tray_app(IslandApp *app, const TrayApp *ta) {
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
static void build_tray_context_menu(IslandApp *app, int tidx) {
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

static void trigger_dbusmenu_click(IslandApp *app, int sni_idx, int32_t item_id) {
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
                char *pct;
                while ((pct = strstr(exec_cmd, " %")) != NULL) {
                    *pct = '\0';
                }
            }
        }
        fclose(f);

        if (no_display || name[0] == '\0' || exec_cmd[0] == '\0') continue;

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
    app->filtered_clip_count = 0;
    for (int i = 0; i < app->clip_count; i++) {
        if (str_contains_ci(app->clip_items[i].preview, app->search)) {
            app->filtered_clips[app->filtered_clip_count++] = i;
        }
    }
    if (app->mode == MODE_CLIPBOARD || app->target_mode == MODE_CLIPBOARD) {
        int shown = app->filtered_clip_count < VISIBLE_CLIP_ITEMS ? app->filtered_clip_count : VISIBLE_CLIP_ITEMS;
        if (shown <= 0) app->selected_idx = 0;
        else if (app->selected_idx >= shown) app->selected_idx = shown - 1;
        return;
    }
    bool show_flyout = (app->launcher_open || app->search_len > 0);
    int max_w = app->filtered_win_count < 6 ? app->filtered_win_count : 6;
    int max_a = show_flyout ? (app->filtered_app_count < VISIBLE_APPS ? app->filtered_app_count : VISIBLE_APPS) : 0;
    int total = max_w + max_a;
    if (total <= 0) app->selected_idx = 0;
    else if (app->selected_idx >= total) app->selected_idx = total - 1;
}

static void parse_snapshot_buf(IslandApp *app, const char *buf) {
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

static void parse_control_buf(IslandApp *app, char *buf) {
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

static void parse_subpanel_buf(IslandApp *app, char *buf) {
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

static void parse_clipboard_buf(IslandApp *app, char *buf) {
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

static void fetch_mako_notifications(IslandApp *app) {
    app->notif_count = 0;
    if (!app->dbus) return;

    const char *methods[2] = { "ListNotifications", "ListHistory" };
    for (int m = 0; m < 2; m++) {
        GVariant *res = g_dbus_connection_call_sync(
            app->dbus, "org.freedesktop.Notifications", "/fr/emersion/Mako",
            "fr.emersion.Mako", methods[m], NULL, G_VARIANT_TYPE("(aa{sv})"),
            G_DBUS_CALL_FLAGS_NONE, 120, NULL, NULL);
        if (!res) continue;

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
}

static bool check_pid_file_alive(const char *path) {
    FILE *f = fopen(path, "r");
    if (!f) return false;
    int pid = 0;
    bool ok = (fscanf(f, "%d", &pid) == 1 && pid > 0 && kill(pid, 0) == 0);
    fclose(f);
    return ok;
}

static void start_async_refresh(IslandApp *app, int kind) {
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
                  "ssid=$(nmcli -t -f active,ssid dev wifi 2>/dev/null | sed -n 's/^yes://p' | head -1); "
                  "wradio=$(nmcli radio wifi 2>/dev/null); "
                  "bt=$(bluetoothctl show 2>/dev/null | grep -q 'Powered: yes' && echo on || echo off); "
                  "btdev=$( [ \"$bt\" = on ] && bluetoothctl devices Connected 2>/dev/null | head -1 | cut -d' ' -f3- ); "
                  "vol=$(LC_ALL=C pactl get-sink-volume @DEFAULT_SINK@ 2>/dev/null | grep -o '[0-9]*%' | head -1 | tr -d '%'); "
                  "mute=$(LC_ALL=C pactl get-sink-mute @DEFAULT_SINK@ 2>/dev/null | awk '{print $2}'); "
                  "pp=$(cat /sys/firmware/acpi/platform_profile 2>/dev/null || powerprofilesctl get 2>/dev/null || echo balanced); "
                  "printf 'SYS\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s\\t%s\\n' \"${wradio:-disabled}\" \"${ssid:-}\" \"$bt\" \"${btdev:-}\" \"${vol:-75}\" \"${mute:-no}\" \"$pp\"; "
                  "sh \"$HOME/infynitae/live/control-menu\" apps-list 2>/dev/null | head -n 6",
                  (char *)NULL);
        } else if (kind == 3) {
            if (app->subpanel == SUB_WIFI) {
                execl("/bin/sh", "sh", "-c",
                      "nmcli -t -f IN-USE,SIGNAL,SECURITY,SSID dev wifi list --rescan no 2>/dev/null "
                      "| sed 's/\\\\:/\\x1f/g' "
                      "| awk -F: '$4 != \"\" { gsub(\"\\x1f\", \":\", $4); if (!seen[$4]++) printf \"%d\\t%s\\t%s\\t%s%%\\n\", ($1==\"*\"), $4, $4, $2 }' "
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

static void refresh_snapshot(IslandApp *app) {
    start_async_refresh(app, MODE_CENTER);
}

static void refresh_control_data(IslandApp *app) {
    start_async_refresh(app, MODE_CONTROL);
}

static void refresh_subpanel_data(IslandApp *app) {
    app->sub_count = 0;
    start_async_refresh(app, 3);
}

static void refresh_clipboard_data(IslandApp *app) {
    start_async_refresh(app, 4);
}

/* ========================================================================== */
/* Cairo + Pango Helpers (Barebones Monochrome Obsidian)                      */
/* ========================================================================== */
static void draw_rect(cairo_t *cr, double x, double y, double w, double h) {
    cairo_rectangle(cr, round(x) + 0.5, round(y) + 0.5, round(w) - 1.0, round(h) - 1.0);
}

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

static void draw_text_right(cairo_t *cr, PangoLayout *layout, const char *font_str,
                            const char *text, double right_x, double cy) {
    PangoFontDescription *desc = pango_font_description_from_string(font_str);
    pango_layout_set_font_description(layout, desc);
    pango_font_description_free(desc);
    pango_layout_set_width(layout, -1);
    pango_layout_set_ellipsize(layout, PANGO_ELLIPSIZE_NONE);
    pango_layout_set_text(layout, text, -1);

    PangoRectangle ink, logical;
    pango_layout_get_extents(layout, &ink, &logical);
    double w = (double)logical.width / (double)PANGO_SCALE;
    if (w > 72.0) {
        pango_layout_set_width(layout, (int)(72.0 * PANGO_SCALE));
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

/* Borderless canvas-window row: NO inner box border, only a subtle flat highlight when selected */
static void draw_bare_item(cairo_t *cr, double x, double y, double w, double h,
                           bool active, bool selected) {
    if (selected) {
        cairo_set_source_rgba(cr, 0.15, 0.17, 0.22, 0.95);
        fill_rect(cr, x, y, w, h);
    } else if (active) {
        cairo_set_source_rgba(cr, 0.10, 0.12, 0.16, 0.78);
        fill_rect(cr, x, y, w, h);
    }
}

static void draw_hline(cairo_t *cr, double x, double y, double w) {
    double inset = (w > 24.0) ? 8.0 : 0.0;
    cairo_set_source_rgba(cr, 0.22, 0.25, 0.34, 0.26);
    cairo_set_line_width(cr, 1.0);
    cairo_move_to(cr, round(x + inset), round(y) + 0.5);
    cairo_line_to(cr, round(x + w - inset), round(y) + 0.5);
    cairo_stroke(cr);
}

static void draw_vline(cairo_t *cr, double x, double y, double h) {
    cairo_set_source_rgba(cr, 0.22, 0.25, 0.34, 0.34);
    cairo_set_line_width(cr, 1.0);
    cairo_move_to(cr, round(x) + 0.5, round(y));
    cairo_line_to(cr, round(x) + 0.5, round(y + h));
    cairo_stroke(cr);
}

static void add_hitbox_nav(IslandApp *app, double x, double y, double w, double h,
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

static void add_hitbox(IslandApp *app, double x, double y, double w, double h, int action_id, int index) {
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

static void load_position_config(IslandApp *app) {
    app->bottom_edge = true;
    app->control_on_left = false;
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
        if (strncmp(p, "ISLAND_EDGE=", 12) == 0) {
            if (strstr(p + 12, "top")) app->bottom_edge = false;
            else if (strstr(p + 12, "bottom")) app->bottom_edge = true;
        } else if (strncmp(p, "ISLAND_CONTROL_SIDE=", 20) == 0) {
            if (strstr(p + 20, "left")) app->control_on_left = true;
            else if (strstr(p + 20, "right")) app->control_on_left = false;
        } else if (strncmp(p, "SURFACE_COLOR=", 14) == 0) {
            parse_hex_rgb(p + 14, app->surface_rgb);
        } else if (strncmp(p, "BORDER_ACTIVE=", 14) == 0) {
            parse_hex_rgb(p + 14, app->border_rgb);
        } else if (strncmp(p, "TEXT_COLOR=", 11) == 0) {
            parse_hex_rgb(p + 11, app->text_rgb);
        }
    }
    fclose(f);
}

static void update_surface_input_region(IslandOutput *out) {
    IslandApp *app = out->app;
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
        if (out->input_w > 0 && out->input_h > 0) {
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
/* Fast Dirty-Rect Renderer (Barebones Clean)                                 */
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

static bool output_has_fullscreen(IslandOutput *out);

static void render_output(IslandOutput *out) {
    if (!out->configured || out->width <= 0 || out->height <= 0) return;

    /* Closed over a fullscreen window: unmapped, nothing drawn over the game */
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

    IslandApp *app = out->app;
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
        IslandMode active_m = (app->mode != MODE_CLOSED) ? app->mode : app->target_mode;

        /* ─────────────────────────────────────────────────────────────────── */
        /* A. MINIMALIST WINDOW STRIP (Super+W) — single outer 1px border      */
        /* ─────────────────────────────────────────────────────────────────── */
        if (active_m == MODE_CENTER) {
            bool show_launcher = (app->launcher_open || app->search_len > 0);
            int shown_apps = show_launcher
                ? (app->filtered_app_count < VISIBLE_APPS ? app->filtered_app_count : VISIBLE_APPS)
                : 0;
            int app_rows = show_launcher ? ((shown_apps > 0 ? shown_apps : 1) + 1) / 2 : 0;

            int max_tabs = 6;
            int tab_cnt = app->filtered_win_count < max_tabs ? app->filtered_win_count : max_tabs;
            double sb_w = 152.0;

            double row_h = 28.0;
            double ext_h = show_launcher ? (1.0 + app_rows * 24.0 + 4.0) : 0.0;
            double max_bar_w = w - 160.0;
            if (max_bar_w > 780.0) max_bar_w = 780.0;
            if (max_bar_w < 420.0) max_bar_w = w - 24.0;

            double bar_w = max_bar_w;
            if (!show_launcher) {
                double ideal_w = sb_w + (tab_cnt > 0 ? tab_cnt * 168.0 : 180.0);
                if (ideal_w < 360.0) ideal_w = 360.0;
                if (ideal_w < bar_w) bar_w = ideal_w;
            }
            double total_h = row_h + ext_h;
            double bar_x = round((w - bar_w) / 2.0);
            double bar_y = round(app->bottom_edge
                ? (h + total_h * (1.0 - ease) - (total_h + 8.0) * ease)
                : (-total_h * (1.0 - ease) + 8.0 * ease));

            cur_dx = (int)bar_x - 2;
            cur_dy = (int)bar_y - 2;
            cur_dw = (int)bar_w + 4;
            cur_dh = (int)total_h + 4;

            /* Single flat dark obsidian window + 1px border from config */
            cairo_set_source_rgba(cr, app->surface_rgb[0], app->surface_rgb[1], app->surface_rgb[2], 0.97 * ease);
            fill_rect(cr, bar_x, bar_y, bar_w, total_h);

            cairo_set_source_rgba(cr, app->border_rgb[0], app->border_rgb[1], app->border_rgb[2], 0.90 * ease);
            cairo_set_line_width(cr, 1.0);
            draw_rect(cr, bar_x, bar_y, bar_w, total_h);
            cairo_stroke(cr);

            double top_y = app->bottom_edge ? (bar_y + ext_h) : bar_y;

            /* 1. Search field (left, borderless) */
            if (app->search_len > 0) {
                cairo_set_source_rgba(cr, 0.94, 0.96, 0.98, 0.96);
                draw_text_left(cr, layout, "Sans 8.5", app->search, bar_x + 10.0, top_y + row_h / 2.0, sb_w - 18.0);
            } else {
                cairo_set_source_rgba(cr, 0.44, 0.48, 0.56, 0.85);
                draw_text_left(cr, layout, "Sans 8.5", "Search…", bar_x + 10.0, top_y + row_h / 2.0, sb_w - 18.0);
            }
            add_hitbox(app, bar_x, top_y, sb_w, row_h, ACT_SEARCH_BOX, 0);
            draw_vline(cr, bar_x + sb_w, top_y + 4.0, row_h - 8.0);

            /* 2. Open Window Tabs (borderless text cells separated by hairlines, flush to right border) */
            double cur_x = bar_x + sb_w + 1.0;
            double right_inner = bar_x + bar_w - 1.0;
            double tabs_avail_w = right_inner - cur_x;
            if (tab_cnt == 0) {
                cairo_set_source_rgba(cr, 0.40, 0.44, 0.52, 0.70);
                draw_text_left(cr, layout, "Sans 8.5", "No open windows", cur_x + 10.0, top_y + row_h / 2.0, tabs_avail_w - 16.0);
            } else {
                double base_tab_w = floor(tabs_avail_w / tab_cnt);

                for (int i = 0; i < tab_cnt; i++) {
                    CanvasWindow *cw = &app->windows[app->filtered_wins[i]];
                    double tx = cur_x + i * base_tab_w;
                    double tw_cell = (i == tab_cnt - 1) ? (right_inner - tx) : base_tab_w;
                    bool sel = (app->selected_idx == i);

                    draw_bare_item(cr, tx, top_y + 1.0, tw_cell, row_h - 2.0, cw->focused, sel);
                    if (i > 0) draw_vline(cr, tx, top_y + 4.0, row_h - 8.0);

                    cairo_set_source_rgba(cr, (cw->focused || sel) ? 0.95 : 0.62,
                                          (cw->focused || sel) ? 0.97 : 0.67,
                                          (cw->focused || sel) ? 1.00 : 0.75, 0.95);
                    draw_text_left(cr, layout, cw->focused ? "Sans Medium 8.5" : "Sans 8.5",
                                   cw->title[0] ? cw->title : cw->app_id,
                                   tx + 10.0, top_y + row_h / 2.0, tw_cell - 18.0);

                    add_hitbox_nav(app, tx, top_y, tw_cell, row_h, ACT_WIN_ITEM, app->filtered_wins[i], i);
                }
            }

            /* 3. Inline App Results inside the same single window frame */
            if (show_launcher) {
                double list_y = app->bottom_edge ? (bar_y + 2.0) : (bar_y + row_h + 1.0);
                draw_hline(cr, bar_x + 1.0, app->bottom_edge ? top_y : (bar_y + row_h), bar_w - 2.0);

                if (shown_apps == 0) {
                    cairo_set_source_rgba(cr, 0.44, 0.48, 0.56, 0.80);
                    draw_text_left(cr, layout, "Sans 8.5", "No matching apps", bar_x + 10.0, list_y + 12.0, bar_w - 20.0);
                } else {
                    double cw = floor((bar_w - 2.0) / 2.0);
                    double ch = 24.0;
                    for (int i = 0; i < shown_apps; i++) {
                        DesktopApp *da = &app->apps[app->filtered_apps[i]];
                        int r = i / 2;
                        int c = i % 2;
                        double ax = bar_x + 1.0 + c * cw;
                        double ay = list_y + r * ch;
                        int nav_i = tab_cnt + i;
                        bool sel = (app->selected_idx == nav_i);

                        draw_bare_item(cr, ax, ay, cw, ch, false, sel);
                        cairo_set_source_rgba(cr, sel ? 0.96 : 0.74, sel ? 0.98 : 0.79, sel ? 1.0 : 0.86, 0.95);
                        draw_text_left(cr, layout, "Sans 8.5", da->name, ax + 10.0, ay + ch / 2.0, cw - 16.0);
                        add_hitbox_nav(app, ax, ay, cw, ch, ACT_APP_LAUNCH, app->filtered_apps[i], nav_i);
                    }
                }
            }
        }

        /* ─────────────────────────────────────────────────────────────────── */
        /* B. MINIMALIST CONTROL WINDOW (Super+A) — single outer 1px border    */
        /* ─────────────────────────────────────────────────────────────────── */
        if (active_m == MODE_CONTROL) {
            int sub_rows = (app->subpanel != SUB_NONE) ? (app->sub_count > 0 ? app->sub_count : 1) : 0;
            double sub_extra_h = (app->subpanel != SUB_NONE) ? (6.0 + sub_rows * 22.0) : 0.0;
            double tray_extra_h = (app->tray_count > 0) ? (6.0 + app->tray_count * 22.0) : 0.0;
            int ctx_rows = (app->tray_ctx_idx >= 0 && app->tray_ctx_idx < app->tray_count)
                ? ((app->ctx_count + 2) / 3) : 0;
            double ctx_extra_h = ctx_rows * 22.0;
            double notif_extra_h = (app->notif_count > 0) ? (26.0 + app->notif_count * 22.0) : 0.0;

            double ctrl_w = 264.0;
            double ctrl_h = 148.0 + sub_extra_h + tray_extra_h + ctx_extra_h + notif_extra_h;
            double ctrl_x = app->control_on_left ? 8.0 : (w - ctrl_w - 8.0);
            double ctrl_y = round(app->bottom_edge
                ? (h + ctrl_h * (1.0 - ease) - (ctrl_h + 8.0) * ease)
                : (-ctrl_h * (1.0 - ease) + 8.0 * ease));

            cur_dx = (int)ctrl_x - 2;
            cur_dy = (int)ctrl_y - 2;
            cur_dw = (int)ctrl_w + 4;
            cur_dh = (int)ctrl_h + 4;

            /* Single flat dark obsidian window + 1px border from config */
            cairo_set_source_rgba(cr, app->surface_rgb[0], app->surface_rgb[1], app->surface_rgb[2], 0.97 * ease);
            fill_rect(cr, ctrl_x, ctrl_y, ctrl_w, ctrl_h);

            cairo_set_source_rgba(cr, app->border_rgb[0], app->border_rgb[1], app->border_rgb[2], 0.90 * ease);
            cairo_set_line_width(cr, 1.0);
            draw_rect(cr, ctrl_x, ctrl_y, ctrl_w, ctrl_h);
            cairo_stroke(cr);

            double rx = ctrl_x + 1.0;
            double rw = ctrl_w - 2.0;
            double ry = ctrl_y + 4.0;

            /* Row 1: Clock & Date (left) + Battery % (right) */
            time_t now = time(NULL);
            struct tm *tm_info = localtime(&now);
            char clock_str[64];
            strftime(clock_str, sizeof(clock_str), "%H:%M  ·  %a %b %d", tm_info);

            cairo_set_source_rgba(cr, 0.90, 0.93, 0.97, 0.95);
            draw_text_left(cr, layout, "Sans Medium 8.5", clock_str, rx + 9.0, ry + 10.0, 190.0);

            if (app->battery_pct >= 0) {
                char bat_s[24];
                snprintf(bat_s, sizeof(bat_s), "%s%d%%", app->battery_charging ? "+" : "", app->battery_pct);
                cairo_set_source_rgba(cr, 0.62, 0.68, 0.76, 0.90);
                draw_text_right(cr, layout, "Sans 8", bat_s, rx + rw - 9.0, ry + 10.0);
            }
            ry += 22.0;
            draw_hline(cr, rx, ry, rw);
            ry += 2.0;

            /* Row 2: Borderless Volume Track (Left-click sets level; Right-click opens Audio Sinks) */
            double vh = 22.0;
            draw_bare_item(cr, rx, ry, rw, vh, app->subpanel == SUB_AUDIO, false);

            cairo_set_source_rgba(cr, 0.56, 0.62, 0.70, 0.90);
            draw_text_left(cr, layout, "Sans 8", "Vol", rx + 9.0, ry + vh / 2.0, 28.0);
            add_hitbox(app, rx, ry, 30.0, vh, ACT_VOL_MUTE, 0);

            double bx = rx + 34.0;
            double bw = rw - 76.0;
            double by = ry + (vh - 2.0) / 2.0;
            cairo_set_source_rgba(cr, 0.16, 0.19, 0.25, 0.95);
            fill_rect(cr, bx, by, bw, 2.0);

            double fill_w = (bw * (app->volume_muted ? 0 : app->volume_pct)) / 100.0;
            if (fill_w > 1.0) {
                cairo_set_source_rgba(cr, 0.82, 0.86, 0.92, 0.95);
                fill_rect(cr, bx, by, fill_w, 2.0);
            }
            add_hitbox(app, bx, ry, bw, vh, ACT_VOL_BAR, (int)bx);

            char v_s[16];
            if (app->volume_muted) strcpy(v_s, "Mute");
            else snprintf(v_s, sizeof(v_s), "%d%%", app->volume_pct);
            double mx = bx + bw + 2.0;
            add_hitbox(app, mx, ry, 40.0, vh, ACT_VOL_MUTE, 0);
            cairo_set_source_rgba(cr, app->volume_muted ? 0.48 : 0.78, app->volume_muted ? 0.52 : 0.82, 0.88, 0.92);
            draw_text_right(cr, layout, "Sans 8", v_s, rx + rw - 9.0, ry + vh / 2.0);

            ry += vh + 2.0;
            draw_hline(cr, rx, ry, rw);
            ry += 2.0;

            /* Rows 3, 4 & 5: Borderless 3×2 Text Toggles (Wi-Fi, BT, DND, Power, Replay, Clip) */
            double tw = floor(rw / 2.0);
            double th = 22.0;
            const char *p_disp = "Balanced";
            if (strcasecmp(app->power_profile, "performance") == 0) p_disp = "Performance";
            else if (strcasecmp(app->power_profile, "power-saver") == 0) p_disp = "Saver";

            struct {
                const char *label;
                const char *val;
                bool active;
                int action;
                bool sub_open;
            } sw[6] = {
                { "Wi-Fi", app->wifi_label, app->wifi_on, ACT_WIFI_TOGGLE, app->subpanel == SUB_WIFI },
                { "BT", app->bt_label, app->bt_on, ACT_BT_TOGGLE, app->subpanel == SUB_BT },
                { "DND", app->dnd_on ? "On" : "Off", app->dnd_on, ACT_DND_TOGGLE, false },
                { "Power", p_disp, strcasecmp(app->power_profile, "performance") == 0, ACT_POWER_PROFILE, false },
                { "Replay", app->replay_on ? "On" : "Off", app->replay_on, ACT_REPLAY_TOGGLE, false },
                { "Clip", "Save", app->replay_on, ACT_REPLAY_SAVE, false }
            };

            draw_vline(cr, rx + tw, ry + 4.0, 3.0 * th - 8.0);

            for (int i = 0; i < 6; i++) {
                int r = i / 2;
                int c = i % 2;
                double sx = rx + c * tw;
                double sy = ry + r * th;
                bool sel = (app->selected_idx == i);

                draw_bare_item(cr, sx, sy, tw, th, false, sel || sw[i].sub_open);
                cairo_set_source_rgba(cr, (sw[i].active || sel) ? 0.92 : 0.56,
                                      (sw[i].active || sel) ? 0.95 : 0.61,
                                      (sw[i].active || sel) ? 0.99 : 0.69, 0.95);
                draw_text_left(cr, layout, sw[i].active ? "Sans Medium 8" : "Sans 8",
                               sw[i].label, sx + 9.0, sy + th / 2.0, 44.0);

                cairo_set_source_rgba(cr, (sw[i].active || sel) ? 0.86 : 0.44,
                                      (sw[i].active || sel) ? 0.90 : 0.49,
                                      (sw[i].active || sel) ? 0.96 : 0.57, 0.92);
                draw_text_right(cr, layout, "Sans 7.5", sw[i].val, sx + tw - 9.0, sy + th / 2.0);

                add_hitbox_nav(app, sx, sy, tw, th, sw[i].action, i, i);
            }

            ry += 3 * th + 2.0;
            int nav_cursor = 6;

            /* Optional Inline Sub-List (Wi-Fi / Bluetooth / Audio Output) */
            if (app->subpanel != SUB_NONE) {
                draw_hline(cr, rx, ry, rw);
                ry += 3.0;
                if (app->sub_count == 0) {
                    cairo_set_source_rgba(cr, 0.44, 0.48, 0.56, 0.80);
                    draw_text_left(cr, layout, "Sans 8", "None found", rx + 9.0, ry + 11.0, rw - 18.0);
                    ry += 22.0;
                } else {
                    for (int i = 0; i < app->sub_count; i++) {
                        SubItem *si = &app->sub_items[i];
                        double sh = 22.0;
                        int nav_i = nav_cursor++;
                        bool sel = (app->selected_idx == nav_i);

                        draw_bare_item(cr, rx, ry, rw, sh, si->active, sel);
                        cairo_set_source_rgba(cr, (si->active || sel) ? 0.95 : 0.68,
                                              (si->active || sel) ? 0.97 : 0.73,
                                              (si->active || sel) ? 1.00 : 0.80, 0.95);
                        draw_text_left(cr, layout, si->active ? "Sans Medium 8" : "Sans 8",
                                       si->label, rx + 9.0, ry + sh / 2.0, rw - 68.0);

                        cairo_set_source_rgba(cr, 0.46, 0.52, 0.60, 0.85);
                        draw_text_right(cr, layout, "Sans 7.5", si->meta, rx + rw - 9.0, ry + sh / 2.0);

                        add_hitbox_nav(app, rx, ry, rw, sh, ACT_SUB_SELECT, i, nav_i);
                        ry += sh;
                    }
                }
                ry += 3.0;
            }

            /* Tray Apps (ONLY actual unmapped background/tray apps; hidden if 0!) */
            if (app->tray_count > 0) {
                draw_hline(cr, rx, ry, rw);
                ry += 3.0;
                for (int i = 0; i < app->tray_count; i++) {
                    TrayApp *ta = &app->tray_apps[i];
                    double row_h = 22.0;
                    int nav_i = nav_cursor++;
                    bool ctx_open = (app->tray_ctx_idx == i);
                    bool sel = (app->selected_idx == nav_i);

                    draw_bare_item(cr, rx, ry, rw, row_h, false, sel || ctx_open);

                    cairo_set_source_rgba(cr, (sel || ctx_open) ? 0.96 : 0.78,
                                          (sel || ctx_open) ? 0.98 : 0.82, 0.90, 0.94);
                    draw_text_left(cr, layout, "Sans 8", ta->name, rx + 9.0, ry + row_h / 2.0, rw - 72.0);

                    cairo_set_source_rgba(cr, 0.44, 0.48, 0.56, 0.85);
                    draw_text_right(cr, layout, "Sans 7.5", ta->mem, rx + rw - 9.0, ry + row_h / 2.0);

                    add_hitbox_nav(app, rx, ry, rw, row_h, ACT_TRAY_ITEM, i, nav_i);
                    ry += row_h;

                    /* App's Own Native Context Menu (from com.canonical.dbusmenu) or Open/Quit/Kill */
                    if (ctx_open && app->ctx_count > 0) {
                        double ch = 22.0;
                        int cols = app->ctx_count < 3 ? app->ctx_count : 3;
                        double cw = floor(rw / cols);

                        for (int k = 0; k < app->ctx_count; k++) {
                            int r = k / 3;
                            int c = k % 3;
                            double cx_b = rx + c * cw;
                            double cy_b = ry + r * ch;
                            int c_nav = nav_cursor++;
                            bool c_sel = (app->selected_idx == c_nav);

                            draw_bare_item(cr, cx_b, cy_b, cw, ch, true, c_sel);
                            cairo_set_source_rgba(cr, c_sel ? 0.98 : 0.76, c_sel ? 0.98 : 0.80, 0.90, 0.95);
                            draw_text_centered(cr, layout, "Sans 8", app->ctx_items[k].label,
                                               cx_b + cw / 2.0, cy_b + ch / 2.0, false);
                            add_hitbox_nav(app, cx_b, cy_b, cw, ch, ACT_TRAY_CTX_CLICK, k, c_nav);
                        }
                        int rows_used = (app->ctx_count + 2) / 3;
                        ry += rows_used * ch;
                    }
                }
                ry += 3.0;
            }

            /* Notification History (from Mako D-Bus ListNotifications + ListHistory; hidden if 0) */
            if (app->notif_count > 0) {
                draw_hline(cr, rx, ry, rw);
                ry += 3.0;
                double hh = 20.0;
                int clr_nav = nav_cursor++;
                bool clr_sel = (app->selected_idx == clr_nav);

                cairo_set_source_rgba(cr, 0.46, 0.51, 0.60, 0.85);
                draw_text_left(cr, layout, "Sans 7.5", "Notifications", rx + 9.0, ry + hh / 2.0, rw - 68.0);

                draw_bare_item(cr, rx + rw - 54.0, ry, 54.0, hh, false, clr_sel);
                cairo_set_source_rgba(cr, clr_sel ? 0.96 : 0.64, clr_sel ? 0.98 : 0.70, 0.80, 0.92);
                draw_text_right(cr, layout, "Sans 7.5", "Clear", rx + rw - 9.0, ry + hh / 2.0);
                add_hitbox_nav(app, rx + rw - 54.0, ry, 54.0, hh, ACT_NOTIF_CLEAR, 0, clr_nav);
                ry += hh;

                for (int i = 0; i < app->notif_count; i++) {
                    NotifItem *ni = &app->notifs[i];
                    double nh = 22.0;
                    int nav_i = nav_cursor++;
                    bool sel = (app->selected_idx == nav_i);

                    draw_bare_item(cr, rx, ry, rw, nh, false, sel);
                    cairo_set_source_rgba(cr, sel ? 0.96 : 0.76, sel ? 0.98 : 0.80, 0.88, 0.94);
                    draw_text_left(cr, layout, "Sans 8", ni->text, rx + 9.0, ry + nh / 2.0, rw - 68.0);

                    cairo_set_source_rgba(cr, 0.44, 0.48, 0.56, 0.85);
                    draw_text_right(cr, layout, "Sans 7.5", ni->app, rx + rw - 9.0, ry + nh / 2.0);

                    add_hitbox_nav(app, rx, ry, rw, nh, ACT_NOTIF_ITEM, i, nav_i);
                    ry += nh;
                }
                ry += 3.0;
            }

            /* Row 6: Borderless Session Strip (Reboot · Power Off · Log Out) */
            draw_hline(cr, rx, ry, rw);
            ry += 2.0;
            double ph = 22.0;
            double pw = floor(rw / 3.0);
            const char *p_labels[3] = { "Reboot", "Power Off", "Log Out" };
            int p_acts[3] = { ACT_REBOOT, ACT_POWEROFF, ACT_LOGOUT };

            for (int i = 0; i < 3; i++) {
                double px_b = rx + i * pw;
                int p_nav = nav_cursor++;
                bool p_sel = (app->selected_idx == p_nav);
                draw_bare_item(cr, px_b, ry, pw, ph, false, p_sel);
                if (i > 0) draw_vline(cr, px_b, ry + 4.0, ph - 8.0);
                cairo_set_source_rgba(cr, p_sel ? 0.96 : 0.66, p_sel ? 0.98 : 0.71, p_sel ? 1.0 : 0.78, 0.90);
                draw_text_centered(cr, layout, "Sans 8", p_labels[i], px_b + pw / 2.0, ry + ph / 2.0, false);
                add_hitbox_nav(app, px_b, ry, pw, ph, p_acts[i], 0, p_nav);
            }
        }

        /* ─────────────────────────────────────────────────────────────────── */
        /* C. MINIMALIST CLIPBOARD BAR (Super+V) — single outer 1px border     */
        /* ─────────────────────────────────────────────────────────────────── */
        if (active_m == MODE_CLIPBOARD) {
            int shown_clips = app->filtered_clip_count < VISIBLE_CLIP_ITEMS
                ? app->filtered_clip_count : VISIBLE_CLIP_ITEMS;
            int rows = shown_clips > 0 ? shown_clips : 1;
            double row_h = 28.0;
            double item_h = 24.0;
            double ext_h = 1.0 + rows * item_h + 4.0;
            double bar_w = w - 160.0;
            if (bar_w > 580.0) bar_w = 580.0;
            if (bar_w < 360.0) bar_w = w - 24.0;

            double total_h = row_h + ext_h;
            double bar_x = round((w - bar_w) / 2.0);
            double bar_y = round(app->bottom_edge
                ? (h + total_h * (1.0 - ease) - (total_h + 8.0) * ease)
                : (-total_h * (1.0 - ease) + 8.0 * ease));

            cur_dx = (int)bar_x - 2;
            cur_dy = (int)bar_y - 2;
            cur_dw = (int)bar_w + 4;
            cur_dh = (int)total_h + 4;

            cairo_set_source_rgba(cr, app->surface_rgb[0], app->surface_rgb[1], app->surface_rgb[2], 0.97 * ease);
            fill_rect(cr, bar_x, bar_y, bar_w, total_h);

            cairo_set_source_rgba(cr, app->border_rgb[0], app->border_rgb[1], app->border_rgb[2], 0.90 * ease);
            cairo_set_line_width(cr, 1.0);
            draw_rect(cr, bar_x, bar_y, bar_w, total_h);
            cairo_stroke(cr);

            double top_y = app->bottom_edge ? (bar_y + ext_h) : bar_y;
            double list_y = app->bottom_edge ? (bar_y + 2.0) : (bar_y + row_h + 1.0);

            if (app->search_len > 0) {
                cairo_set_source_rgba(cr, 0.94, 0.96, 0.98, 0.96);
                draw_text_left(cr, layout, "Sans 8.5", app->search, bar_x + 10.0, top_y + row_h / 2.0, bar_w - 80.0);
            } else {
                cairo_set_source_rgba(cr, 0.44, 0.48, 0.56, 0.85);
                draw_text_left(cr, layout, "Sans 8.5", "Search clipboard…", bar_x + 10.0, top_y + row_h / 2.0, bar_w - 80.0);
            }

            if (app->clip_count > 0) {
                cairo_set_source_rgba(cr, 0.62, 0.68, 0.78, 0.90);
                draw_text_right(cr, layout, "Sans 7.5", "Clear", bar_x + bar_w - 10.0, top_y + row_h / 2.0);
                add_hitbox(app, bar_x + bar_w - 56.0, top_y, 55.0, row_h, ACT_CLIP_CLEAR, 0);
            }

            draw_hline(cr, bar_x + 1.0, app->bottom_edge ? top_y : (bar_y + row_h), bar_w - 2.0);

            if (shown_clips == 0) {
                cairo_set_source_rgba(cr, 0.44, 0.48, 0.56, 0.80);
                draw_text_left(cr, layout, "Sans 8.5",
                               app->clip_count == 0 ? "Clipboard is empty" : "No matching clipboard items",
                               bar_x + 10.0, list_y + item_h / 2.0, bar_w - 20.0);
            } else {
                for (int i = 0; i < shown_clips; i++) {
                    int cidx = app->filtered_clips[i];
                    ClipItem *ci = &app->clip_items[cidx];
                    double cy = list_y + i * item_h;
                    bool sel = (app->selected_idx == i);

                    draw_bare_item(cr, bar_x + 1.0, cy, bar_w - 2.0, item_h, false, sel);
                    cairo_set_source_rgba(cr, sel ? 0.96 : 0.76, sel ? 0.98 : 0.81, sel ? 1.0 : 0.88, 0.95);
                    draw_text_left(cr, layout, "Sans 8.5", ci->preview, bar_x + 10.0, cy + item_h / 2.0, bar_w - 22.0);
                    add_hitbox_nav(app, bar_x + 1.0, cy, bar_w - 2.0, item_h, ACT_CLIP_ITEM, cidx, i);
                }
            }
        }
    }

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
static void set_mode(IslandApp *app, IslandMode new_mode) {
    if (new_mode != MODE_CLOSED) {
        load_position_config(app);
        app->target_mode = new_mode;
        app->mode = new_mode;
        app->kb_grab = true;
        app->search[0] = '\0';
        app->search_len = 0;
        app->launcher_open = false;
        app->subpanel = SUB_NONE;
        app->tray_ctx_idx = -1;
        app->selected_idx = 0;
        if (new_mode == MODE_CENTER) {
            refresh_snapshot(app);
        } else if (new_mode == MODE_CONTROL) {
            refresh_control_data(app);
            app->selected_idx = (app->tray_count > 0) ? 6 : 0;
        } else if (new_mode == MODE_CLIPBOARD) {
            refresh_clipboard_data(app);
        }
    } else {
        app->mode = MODE_CLOSED;
        app->kb_grab = false;
        app->launcher_open = false;
        app->subpanel = SUB_NONE;
        app->tray_ctx_idx = -1;
    }
    damage_all(app);
}

static void toggle_subpanel(IslandApp *app, ControlSubpanel target) {
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

static void close_window_at(IslandApp *app, int widx) {
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

static void quit_tray_at(IslandApp *app, int tidx, const char *sig_name) {
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

static void delete_clip_at(IslandApp *app, int cidx) {
    if (cidx < 0 || cidx >= app->clip_count) return;
    run_cliphist_with_stdin("cliphist delete 2>/dev/null", app->clip_items[cidx].raw_line);
    for (int k = cidx; k < app->clip_count - 1; k++) {
        app->clip_items[k] = app->clip_items[k + 1];
    }
    app->clip_count--;
    update_filters(app);
    damage_all(app);
}

static void close_selected_item(IslandApp *app) {
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

static bool execute_hitbox(IslandApp *app, HitBox *hb, double click_x, uint32_t button) {
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
    case ACT_SEARCH_BOX:
        app->launcher_open = !app->launcher_open;
        update_filters(app);
        damage_all(app);
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
    case ACT_APP_LAUNCH: {
        DesktopApp *da = &app->apps[hb->index];
        set_mode(app, MODE_CLOSED);
        spawn_detached(da->exec_cmd);
        return true;
    }
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
                spawn_script("nmcli connection up id \"$1\" >/dev/null 2>&1 || "
                             "nmcli dev wifi connect \"$1\" >/dev/null 2>&1",
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
        const char *next_p = "balanced";
        if (strcmp(app->power_profile, "balanced") == 0) next_p = "performance";
        else if (strcmp(app->power_profile, "performance") == 0) next_p = "power-saver";
        snprintf(app->power_profile, sizeof(app->power_profile), "%s", next_p);
        char cmd[128];
        snprintf(cmd, sizeof(cmd), "powerprofilesctl set %s 2>/dev/null", next_p);
        spawn_detached(cmd);
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
        set_mode(app, MODE_CLOSED);
        spawn_detached(
            "[ -f ~/.config/infynitae/config ] && . ~/.config/infynitae/config 2>/dev/null || true; "
            ": > ~/.local/state/infynitae/stop; "
            "pkill -f 'infynitae.*restore watch' 2>/dev/null || true; "
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
        set_mode(app, MODE_CLOSED);
        spawn_detached(
            "[ -f ~/.config/infynitae/config ] && . ~/.config/infynitae/config 2>/dev/null || true; "
            ": > ~/.local/state/infynitae/stop; "
            "pkill -f 'infynitae.*restore watch' 2>/dev/null || true; "
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
            "pkill -f 'infynitae.*restore watch' 2>/dev/null || true; "
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

static void activate_selected(IslandApp *app) {
    for (int i = 0; i < app->hitbox_count; i++) {
        HitBox *hb = &app->hitboxes[i];
        if (hb->nav_idx == app->selected_idx) {
            execute_hitbox(app, hb, hb->x + hb->w / 2.0, BTN_LEFT);
            return;
        }
    }
}

static bool handle_click_at(IslandApp *app, IslandOutput *out, double x, double y, uint32_t button) {
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

static int count_nav_items(IslandApp *app) {
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
        app->anim_progress += diff * 0.55;
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

static void touch_down(void *data, struct wl_touch *touch, uint32_t serial, uint32_t time,
                       struct wl_surface *surface, int32_t id, wl_fixed_t fx, wl_fixed_t fy) {
    (void)touch; (void)serial;
    IslandApp *app = data;
    IslandOutput *out = output_for_surface(app, surface);
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
    IslandApp *app = data;
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
        for (IslandOutput *o = app->outputs; o; o = o->next) request_frame(o);
    } else if (app->mode != MODE_CLOSED && pull_dist < -20.0) {
        app->dragging = true;
        double prog = 1.0 + (pull_dist / 80.0);
        if (prog < 0.0) prog = 0.0;
        if (prog > 1.0) prog = 1.0;
        app->anim_progress = prog;
        for (IslandOutput *o = app->outputs; o; o = o->next) request_frame(o);
    }
}

static void touch_up(void *data, struct wl_touch *touch, uint32_t serial, uint32_t time, int32_t id) {
    (void)touch; (void)serial;
    IslandApp *app = data;
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

static void pointer_enter(void *data, struct wl_pointer *p, uint32_t s,
                          struct wl_surface *surface, wl_fixed_t fx, wl_fixed_t fy) {
    (void)p; (void)s;
    IslandApp *app = data;
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
    IslandApp *app = data;
    if (app->mode != MODE_CLOSED && app->kb_grab) {
        app->kb_grab = false;
        for (IslandOutput *o = app->outputs; o; o = o->next) {
            update_surface_input_region(o);
            wl_surface_commit(o->surface);
        }
    }
}

static void pointer_motion(void *data, struct wl_pointer *p, uint32_t t, wl_fixed_t fx, wl_fixed_t fy) {
    (void)p; (void)t;
    IslandApp *app = data;
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
        set_mode(app, in_ctrl_zone ? MODE_CONTROL : MODE_CENTER);
        return;
    }
    handle_click_at(app, app->pointer_output, app->pointer_x, app->pointer_y, button);
}

static void pointer_axis(void *data, struct wl_pointer *p, uint32_t t, uint32_t axis, wl_fixed_t val) {
    (void)p; (void)t;
    IslandApp *app = data;
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
    bool super_down = xkb_state_mod_name_is_active(app->xkb_state, XKB_MOD_NAME_LOGO, XKB_STATE_MODS_EFFECTIVE) > 0;
    bool ctrl_down = xkb_state_mod_name_is_active(app->xkb_state, XKB_MOD_NAME_CTRL, XKB_STATE_MODS_EFFECTIVE) > 0;

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
        if (sym == XKB_KEY_c || sym == XKB_KEY_C || sym == XKB_KEY_q || sym == XKB_KEY_Q) {
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
        if (app->mode == MODE_CENTER && !app->launcher_open && app->search_len == 0) {
            app->launcher_open = true;
            update_filters(app);
        } else if (app->mode == MODE_CONTROL && app->selected_idx < 2 && app->subpanel == SUB_NONE) {
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
        if (total > 0) {
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
        if (app->mode == MODE_CONTROL) {
            app->mode = MODE_CENTER;
            app->target_mode = MODE_CENTER;
            app->subpanel = SUB_NONE;
            refresh_snapshot(app);
        }
        if (app->search_len + (size_t)len < sizeof(app->search) - 1) {
            memcpy(app->search + app->search_len, utf8, (size_t)len);
            app->search_len += (size_t)len;
            app->search[app->search_len] = '\0';
            if (app->mode == MODE_CENTER) app->launcher_open = true;
            update_filters(app);
            if (app->mode == MODE_CLIPBOARD) {
                app->selected_idx = 0;
            } else {
                int sw = app->filtered_win_count < 6 ? app->filtered_win_count : 6;
                app->selected_idx = (sw > 0) ? 0 : sw;
            }
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

static void layer_surface_configure(void *data, struct zwlr_layer_surface_v1 *ls,
                                    uint32_t serial, uint32_t width, uint32_t height) {
    IslandOutput *out = data;
    zwlr_layer_surface_v1_ack_configure(ls, serial);
    if (width > 0) out->width = (int32_t)width;
    if (height > 0) out->height = (int32_t)height;
    out->configured = true;
    render_output(out);
    /* remapped mid-animation (opened over a fullscreen window): keep animating */
    IslandApp *app = out->app;
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

/* ========================================================================== */
/* Fullscreen windows (wlr-foreign-toplevel): the island unmaps over them      */
/* ========================================================================== */
static bool output_has_fullscreen(IslandOutput *out) {
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
        if (*s == ZWLR_FOREIGN_TOPLEVEL_HANDLE_V1_STATE_FULLSCREEN) t->pending_fullscreen = true;
    }
}

static IslandApp *fs_app; /* the one app, for the toplevel callbacks */

static void fs_changed(IslandApp *app) {
    for (IslandOutput *o = app->outputs; o; o = o->next) render_output(o);
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
    IslandApp *app = d;
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
    IslandApp *app = data;
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

/* The island the pid file names, or 0: a pid file left by an island that crashed may name another
 * process by now, which a signal meant for the island would end (SIGUSR1 terminates by default). */
static pid_t island_pid(const char *pfile) {
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
    /* comm is the name cut to 15 characters: "infynitae-islan" */
    return strncmp(comm, "infynitae-islan", 15) == 0 ? pid : 0;
}

int main(int argc, char **argv) {
    const char *pfile = pid_file_path();

    if (argc > 1) {
        pid_t running_pid = island_pid(pfile);
        if (strcmp(argv[1], "toggle") == 0 || strcmp(argv[1], "center") == 0) {
            if (running_pid > 0 && kill(running_pid, SIGUSR1) == 0) return 0;
        } else if (strcmp(argv[1], "control") == 0) {
            if (running_pid > 0 && kill(running_pid, SIGUSR2) == 0) return 0;
        } else if (strcmp(argv[1], "clipboard") == 0) {
            if (running_pid > 0 && kill(running_pid, SIGRTMIN) == 0) return 0;
        } else if (strcmp(argv[1], "stop") == 0) {
            if (running_pid > 0) kill(running_pid, SIGTERM);
            return 0;
        }
    }

    pid_t old_pid = island_pid(pfile);
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
    signal(SIGTERM, on_sigterm);
    signal(SIGINT, on_sigterm);
    signal(SIGCHLD, SIG_IGN);

    IslandApp app = {0};
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
        fprintf(stderr, "[infynitae-island] Cannot connect to Wayland display\n");
        return 1;
    }

    app.registry = wl_display_get_registry(app.display);
    wl_registry_add_listener(app.registry, &registry_listener, &app);
    wl_display_roundtrip(app.display);
    for (IslandOutput *o = app.outputs; o; o = o->next) setup_layer_surface(o);
    wl_display_roundtrip(app.display);

    start_async_refresh(&app, MODE_CONTROL);

    if (argc > 1 && (strcmp(argv[1], "toggle") == 0 || strcmp(argv[1], "center") == 0 || strcmp(argv[1], "open") == 0)) {
        set_mode(&app, MODE_CENTER);
    } else if (argc > 1 && strcmp(argv[1], "control") == 0) {
        set_mode(&app, MODE_CONTROL);
    } else if (argc > 1 && strcmp(argv[1], "clipboard") == 0) {
        set_mode(&app, MODE_CLIPBOARD);
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
