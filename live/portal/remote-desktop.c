/*
 * Remote desktop for Infynitae's portal: org.freedesktop.impl.portal.RemoteDesktop, and
 * org.freedesktop.impl.portal.ScreenCast so that a remote desktop session has its screens.
 *
 * ScreenCast is xdg-desktop-portal-wlr's, passed through: every call goes to it unchanged (its
 * chooser, its PipeWire streams) under the same session handle, so screen sharing works as it
 * did. A remote desktop session adds the input. The first time an app asks you are asked
 * (live/remote/remote-desktop-ask: this time, always, or deny); then its pointer drives a virtual
 * pointer (zwlr_virtual_pointer_v1; one per shared screen for absolute motion, bound to it) and
 * its keys a virtual keyboard that uploads the keymap the compositor gave this client, so the
 * compositor takes it as a real keyboard: the shortcuts work, and while locked it types nothing
 * (owner, 2026-10-25). env.sh routes the two interfaces here only when REMOTE_DESKTOP is on.
 */

#define _GNU_SOURCE
#include <gio/gio.h>
#include <glib-unix.h>
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>

#include "remote-desktop.h"
#include "virtual-keyboard-protocol.h"
#include "wlr-virtual-pointer-protocol.h"
#include "xdg-output-protocol.h"

#define OBJECT_PATH "/org/freedesktop/portal/desktop"
#define RD_IFACE "org.freedesktop.impl.portal.RemoteDesktop"
#define SC_IFACE "org.freedesktop.impl.portal.ScreenCast"
#define SESSION_IFACE "org.freedesktop.impl.portal.Session"
#define WLR_NAME "org.freedesktop.impl.portal.desktop.wlr"

#define DEVICE_KEYBOARD 1
#define DEVICE_POINTER 2

static const gchar rd_xml[] =
  "<node>"
  "  <interface name='org.freedesktop.impl.portal.RemoteDesktop'>"
  "    <property name='AvailableDeviceTypes' type='u' access='read'/>"
  "    <property name='version' type='u' access='read'/>"
  "    <method name='CreateSession'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='SelectDevices'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='Start'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='s' name='parent_window' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='NotifyPointerMotion'>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='d' name='dx' direction='in'/>"
  "      <arg type='d' name='dy' direction='in'/>"
  "    </method>"
  "    <method name='NotifyPointerMotionAbsolute'>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='stream' direction='in'/>"
  "      <arg type='d' name='x' direction='in'/>"
  "      <arg type='d' name='y' direction='in'/>"
  "    </method>"
  "    <method name='NotifyPointerButton'>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='i' name='button' direction='in'/>"
  "      <arg type='u' name='state' direction='in'/>"
  "    </method>"
  "    <method name='NotifyPointerAxis'>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='d' name='dx' direction='in'/>"
  "      <arg type='d' name='dy' direction='in'/>"
  "    </method>"
  "    <method name='NotifyPointerAxisDiscrete'>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='axis' direction='in'/>"
  "      <arg type='i' name='steps' direction='in'/>"
  "    </method>"
  "    <method name='NotifyKeyboardKeycode'>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='i' name='keycode' direction='in'/>"
  "      <arg type='u' name='state' direction='in'/>"
  "    </method>"
  "    <method name='NotifyKeyboardKeysym'>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='i' name='keysym' direction='in'/>"
  "      <arg type='u' name='state' direction='in'/>"
  "    </method>"
  "  </interface>"
  "  <interface name='org.freedesktop.impl.portal.ScreenCast'>"
  "    <property name='AvailableSourceTypes' type='u' access='read'/>"
  "    <property name='AvailableCursorModes' type='u' access='read'/>"
  "    <property name='version' type='u' access='read'/>"
  "    <method name='CreateSession'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='SelectSources'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='Start'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='s' name='parent_window' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "  </interface>"
  "  <interface name='org.freedesktop.impl.portal.Session'>"
  "    <property name='version' type='u' access='read'/>"
  "    <method name='Close'/>"
  "    <signal name='Closed'/>"
  "  </interface>"
  "</node>";

/* ---- the compositor: outputs, the seat's keymap, the virtual devices --------------------- */

typedef struct {
    uint32_t name;
    struct wl_output *output;
    struct zxdg_output_v1 *xdg;
    int32_t x, y, w, h;
} Output;

static struct wl_display *wl;
static struct wl_seat *seat;
static struct wl_keyboard *seat_keyboard;
static struct zwlr_virtual_pointer_manager_v1 *vp_manager;
static struct zwp_virtual_keyboard_manager_v1 *vk_manager;
static struct zxdg_output_manager_v1 *xdg_output_manager;
static GList *outputs; /* Output* */
static char *keymap_text;
static uint32_t keymap_size;
static struct xkb_context *xkb_ctx;
static struct xkb_keymap *xkb_keymap;

static void xdg_output_position(void *data, struct zxdg_output_v1 *x, int32_t px, int32_t py) {
    Output *o = data;
    o->x = px;
    o->y = py;
}
static void xdg_output_size(void *data, struct zxdg_output_v1 *x, int32_t w, int32_t h) {
    Output *o = data;
    o->w = w;
    o->h = h;
}
static void xdg_output_noop(void *data, struct zxdg_output_v1 *x) {}
static void xdg_output_str(void *data, struct zxdg_output_v1 *x, const char *s) {}
static const struct zxdg_output_v1_listener xdg_output_listener = {
    xdg_output_position, xdg_output_size, xdg_output_noop, xdg_output_str, xdg_output_str,
};

static void output_watch(Output *o) {
    if (xdg_output_manager && o->output && !o->xdg) {
        o->xdg = zxdg_output_manager_v1_get_xdg_output(xdg_output_manager, o->output);
        zxdg_output_v1_add_listener(o->xdg, &xdg_output_listener, o);
    }
}

/* the seat's keymap, as the compositor sends it: the virtual keyboard uploads it back */
static void kb_keymap(void *data, struct wl_keyboard *k, uint32_t format, int32_t fd, uint32_t size) {
    if (format == WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1) {
        char *map = mmap(NULL, size, PROT_READ, MAP_PRIVATE, fd, 0);
        if (map != MAP_FAILED) {
            g_free(keymap_text);
            keymap_text = g_malloc(size + 1);
            memcpy(keymap_text, map, size);
            keymap_text[size] = '\0';
            keymap_size = size;
            munmap(map, size);
            if (xkb_keymap)
                xkb_keymap_unref(xkb_keymap);
            xkb_keymap = xkb_keymap_new_from_string(xkb_ctx, keymap_text, XKB_KEYMAP_FORMAT_TEXT_V1,
                                                    XKB_KEYMAP_COMPILE_NO_FLAGS);
        }
    }
    close(fd);
}
static void kb_enter(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *f, struct wl_array *a) {}
static void kb_leave(void *d, struct wl_keyboard *k, uint32_t s, struct wl_surface *f) {}
static void kb_key(void *d, struct wl_keyboard *k, uint32_t s, uint32_t t, uint32_t key, uint32_t st) {}
static void kb_mods(void *d, struct wl_keyboard *k, uint32_t s, uint32_t a, uint32_t b, uint32_t c, uint32_t g) {}
static void kb_repeat(void *d, struct wl_keyboard *k, int32_t r, int32_t delay) {}
static const struct wl_keyboard_listener kb_listener = {
    kb_keymap, kb_enter, kb_leave, kb_key, kb_mods, kb_repeat,
};

static void registry_global(void *data, struct wl_registry *r, uint32_t name, const char *iface,
                            uint32_t version) {
    if (strcmp(iface, wl_seat_interface.name) == 0 && !seat) {
        seat = wl_registry_bind(r, name, &wl_seat_interface, 1);
        seat_keyboard = wl_seat_get_keyboard(seat);
        wl_keyboard_add_listener(seat_keyboard, &kb_listener, NULL);
    } else if (strcmp(iface, wl_output_interface.name) == 0) {
        Output *o = g_new0(Output, 1);
        o->name = name;
        o->output = wl_registry_bind(r, name, &wl_output_interface, 1);
        outputs = g_list_append(outputs, o);
        output_watch(o);
    } else if (strcmp(iface, zwlr_virtual_pointer_manager_v1_interface.name) == 0 && version >= 2) {
        vp_manager = wl_registry_bind(r, name, &zwlr_virtual_pointer_manager_v1_interface, 2);
    } else if (strcmp(iface, zwp_virtual_keyboard_manager_v1_interface.name) == 0) {
        vk_manager = wl_registry_bind(r, name, &zwp_virtual_keyboard_manager_v1_interface, 1);
    } else if (strcmp(iface, zxdg_output_manager_v1_interface.name) == 0) {
        xdg_output_manager = wl_registry_bind(r, name, &zxdg_output_manager_v1_interface, 2);
        for (GList *l = outputs; l; l = l->next)
            output_watch(l->data);
    }
}

static void registry_global_remove(void *data, struct wl_registry *r, uint32_t name) {
    for (GList *l = outputs; l; l = l->next) {
        Output *o = l->data;
        if (o->name == name) {
            if (o->xdg)
                zxdg_output_v1_destroy(o->xdg);
            wl_output_destroy(o->output);
            outputs = g_list_delete_link(outputs, l);
            g_free(o);
            return;
        }
    }
}

static const struct wl_registry_listener registry_listener = {
    registry_global, registry_global_remove,
};

static gboolean wl_readable(gint fd, GIOCondition cond, gpointer data) {
    if (!wl || wl_display_dispatch(wl) < 0) {
        g_printerr("[infynitae-portal] remote desktop: the compositor went away\n");
        return G_SOURCE_REMOVE;
    }
    return G_SOURCE_CONTINUE;
}

/* The compositor, connected once: everything the virtual devices need, or FALSE. */
static gboolean wl_ready(void) {
    if (wl)
        return vp_manager && vk_manager && seat && keymap_text;
    wl = wl_display_connect(NULL);
    if (!wl) {
        g_printerr("[infynitae-portal] remote desktop: no Wayland display\n");
        return FALSE;
    }
    xkb_ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    struct wl_registry *registry = wl_display_get_registry(wl);
    wl_registry_add_listener(registry, &registry_listener, NULL);
    wl_display_roundtrip(wl); /* the globals */
    wl_display_roundtrip(wl); /* the outputs' places, the keymap */
    g_unix_fd_add(wl_display_get_fd(wl), G_IO_IN, wl_readable, NULL);
    if (!vp_manager || !vk_manager || !seat || !keymap_text)
        g_printerr("[infynitae-portal] remote desktop: the compositor lacks virtual input\n");
    return vp_manager && vk_manager && seat && keymap_text;
}

static uint32_t now_ms(void) {
    return (uint32_t)(g_get_monotonic_time() / 1000);
}

/* ---- sessions --------------------------------------------------------------------------- */

typedef struct {
    uint32_t node;
    int32_t x, y, w, h; /* where the stream's screen is, logical px */
    struct zwlr_virtual_pointer_v1 *pointer; /* bound to that screen, for absolute motion */
} Stream;

typedef struct {
    char *handle;
    char *app_id;
    gboolean remote;  /* a RemoteDesktop session (else a ScreenCast one) */
    gboolean cast;    /* xdg-desktop-portal-wlr has it too */
    gboolean started;
    guint32 devices;
    guint reg_id;
    guint closed_sub;
    GArray *streams; /* Stream */
    struct zwlr_virtual_pointer_v1 *pointer;
    struct zwp_virtual_keyboard_v1 *keyboard;
} Session;

static GDBusConnection *conn;
static GDBusNodeInfo *node;
static GList *rd_sessions; /* Session* */

static Session *session_find(const char *handle) {
    for (GList *l = rd_sessions; l; l = l->next)
        if (g_strcmp0(((Session *)l->data)->handle, handle) == 0)
            return l->data;
    return NULL;
}

static void devices_destroy(Session *s) {
    for (guint i = 0; s->streams && i < s->streams->len; i++) {
        Stream *st = &g_array_index(s->streams, Stream, i);
        if (st->pointer)
            zwlr_virtual_pointer_v1_destroy(st->pointer);
        st->pointer = NULL;
    }
    if (s->pointer)
        zwlr_virtual_pointer_v1_destroy(s->pointer);
    if (s->keyboard)
        zwp_virtual_keyboard_v1_destroy(s->keyboard);
    s->pointer = NULL;
    s->keyboard = NULL;
    if (wl)
        wl_display_flush(wl);
}

/* The session ends: its devices go (the compositor releases what they held), the cast with it. */
static void session_end(Session *s, gboolean tell_wlr, gboolean emit_closed) {
    devices_destroy(s);
    if (s->cast && tell_wlr)
        g_dbus_connection_call(conn, WLR_NAME, s->handle, SESSION_IFACE, "Close", NULL, NULL,
                               G_DBUS_CALL_FLAGS_NONE, -1, NULL, NULL, NULL);
    if (emit_closed)
        g_dbus_connection_emit_signal(conn, NULL, s->handle, SESSION_IFACE, "Closed", NULL, NULL);
    if (s->closed_sub)
        g_dbus_connection_signal_unsubscribe(conn, s->closed_sub);
    if (s->reg_id)
        g_dbus_connection_unregister_object(conn, s->reg_id);
    rd_sessions = g_list_remove(rd_sessions, s);
    if (s->streams)
        g_array_unref(s->streams);
    g_free(s->handle);
    g_free(s->app_id);
    g_free(s);
}

static void wlr_session_closed(GDBusConnection *c, const gchar *sender, const gchar *path,
                               const gchar *iface, const gchar *signal, GVariant *params,
                               gpointer data) {
    Session *s = session_find(path);
    if (s)
        session_end(s, FALSE, TRUE);
}

static void method_call(GDBusConnection *c, const gchar *sender, const gchar *path,
                        const gchar *iface, const gchar *method, GVariant *params,
                        GDBusMethodInvocation *inv, gpointer data);
static GVariant *get_property(GDBusConnection *c, const gchar *sender, const gchar *path,
                              const gchar *iface, const gchar *prop, GError **error, gpointer data);
static const GDBusInterfaceVTable vtable = { method_call, get_property, NULL, { 0 } };

static Session *session_new(const char *handle, const char *app_id, gboolean remote) {
    Session *s = g_new0(Session, 1);
    s->handle = g_strdup(handle);
    s->app_id = g_strdup(app_id);
    s->remote = remote;
    s->streams = g_array_new(FALSE, TRUE, sizeof(Stream));
    s->reg_id = g_dbus_connection_register_object(
        conn, handle, g_dbus_node_info_lookup_interface(node, SESSION_IFACE), &vtable, NULL, NULL, NULL);
    rd_sessions = g_list_append(rd_sessions, s);
    return s;
}

static void session_cast(Session *s) {
    s->cast = TRUE;
    s->closed_sub = g_dbus_connection_signal_subscribe(conn, WLR_NAME, SESSION_IFACE, "Closed",
                                                       s->handle, NULL, G_DBUS_SIGNAL_FLAGS_NONE,
                                                       wlr_session_closed, NULL, NULL);
}

/* ---- xdg-desktop-portal-wlr, passed through --------------------------------------------- */

typedef enum { STEP_CREATE, STEP_SELECT_AFTER_CREATE, STEP_SELECT, STEP_START, STEP_RD_START } Step;

typedef struct {
    GDBusMethodInvocation *inv;
    Step step;
    char *handle;
    char *session;
    char *app_id;
    char *parent;
    GVariant *options;
} Call;

static void call_free(Call *call) {
    g_free(call->handle);
    g_free(call->session);
    g_free(call->app_id);
    g_free(call->parent);
    if (call->options)
        g_variant_unref(call->options);
    g_free(call);
}

static void reply(GDBusMethodInvocation *inv, guint32 response, GVariantBuilder *results) {
    GVariantBuilder empty;
    if (!results) {
        g_variant_builder_init(&empty, G_VARIANT_TYPE_VARDICT);
        results = &empty;
    }
    g_dbus_method_invocation_return_value(inv, g_variant_new("(ua{sv})", response, results));
}

static void wlr_call(const char *method, GVariant *params, Call *call);

/* the devices of a started remote desktop session: a pointer, one per stream, a keyboard */
static gboolean devices_create(Session *s) {
    if (!wl_ready())
        return FALSE;
    if (s->devices & DEVICE_POINTER) {
        s->pointer = zwlr_virtual_pointer_manager_v1_create_virtual_pointer(vp_manager, seat);
        for (guint i = 0; i < s->streams->len; i++) {
            Stream *st = &g_array_index(s->streams, Stream, i);
            for (GList *l = outputs; l; l = l->next) {
                Output *o = l->data;
                if (o->x == st->x && o->y == st->y) {
                    st->pointer = zwlr_virtual_pointer_manager_v1_create_virtual_pointer_with_output(
                        vp_manager, seat, o->output);
                    break;
                }
            }
        }
    }
    if (s->devices & DEVICE_KEYBOARD) {
        int fd = memfd_create("infynitae-keymap", MFD_CLOEXEC);
        if (fd < 0 || write(fd, keymap_text, keymap_size) != (ssize_t)keymap_size) {
            if (fd >= 0)
                close(fd);
            return FALSE;
        }
        s->keyboard = zwp_virtual_keyboard_manager_v1_create_virtual_keyboard(vk_manager, seat);
        zwp_virtual_keyboard_v1_keymap(s->keyboard, WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1, fd, keymap_size);
        close(fd);
    }
    wl_display_flush(wl);
    return TRUE;
}

/* the streams xdg-desktop-portal-wlr started: kept for absolute motion, and handed on */
static void keep_streams(Session *s, GVariant *results, GVariantBuilder *out) {
    GVariant *streams = g_variant_lookup_value(results, "streams", G_VARIANT_TYPE("a(ua{sv})"));
    if (!streams)
        return;
    GVariantIter it;
    guint32 node_id;
    GVariant *props;
    g_variant_iter_init(&it, streams);
    while (g_variant_iter_next(&it, "(u@a{sv})", &node_id, &props)) {
        Stream st = { .node = node_id };
        g_variant_lookup(props, "position", "(ii)", &st.x, &st.y);
        g_variant_lookup(props, "size", "(ii)", &st.w, &st.h);
        g_array_append_val(s->streams, st);
        g_variant_unref(props);
    }
    g_variant_builder_add(out, "{sv}", "streams", streams);
}

static void rd_finish_start(Call *call, GVariant *wlr_results) {
    Session *s = session_find(call->session);
    if (!s) {
        reply(call->inv, 2, NULL);
        return;
    }
    GVariantBuilder results;
    g_variant_builder_init(&results, G_VARIANT_TYPE_VARDICT);
    if (wlr_results)
        keep_streams(s, wlr_results, &results);
    if (!devices_create(s)) {
        g_variant_builder_clear(&results);
        reply(call->inv, 2, NULL);
        return;
    }
    s->started = TRUE;
    g_variant_builder_add(&results, "{sv}", "devices", g_variant_new_uint32(s->devices));
    printf("[infynitae-portal] Remote desktop started: app='%s', devices %u, streams %u\n",
           s->app_id, s->devices, s->streams->len);
    fflush(stdout);
    reply(call->inv, 0, &results);
}

static void wlr_done(GObject *source, GAsyncResult *res, gpointer data) {
    Call *call = data;
    GError *error = NULL;
    GVariant *ret = g_dbus_connection_call_finish(conn, res, &error);
    if (!ret) {
        g_printerr("[infynitae-portal] xdg-desktop-portal-wlr: %s\n", error->message);
        g_clear_error(&error);
        reply(call->inv, 2, NULL);
        call_free(call);
        return;
    }
    guint32 response;
    GVariant *results;
    g_variant_get(ret, "(u@a{sv})", &response, &results);
    Session *s = session_find(call->session);
    switch (call->step) {
    case STEP_CREATE:
        if (response == 0 && !s)
            session_cast(session_new(call->session, call->app_id, FALSE));
        g_dbus_method_invocation_return_value(call->inv, ret);
        break;
    case STEP_SELECT_AFTER_CREATE:
        /* a remote desktop session's cast exists now: then its sources */
        if (response == 0 && s) {
            session_cast(s);
            Call *next = g_new0(Call, 1);
            *next = *call;
            next->step = STEP_SELECT;
            g_free(call);
            call = NULL;
            wlr_call("SelectSources",
                     g_variant_new("(oos@a{sv})", next->handle, next->session, next->app_id,
                                   next->options),
                     next);
        } else {
            g_dbus_method_invocation_return_value(call->inv, ret);
        }
        break;
    case STEP_SELECT:
    case STEP_START:
        g_dbus_method_invocation_return_value(call->inv, ret);
        break;
    case STEP_RD_START:
        if (response == 0)
            rd_finish_start(call, results);
        else
            g_dbus_method_invocation_return_value(call->inv, ret);
        break;
    }
    g_variant_unref(results);
    g_variant_unref(ret);
    if (call)
        call_free(call);
}

static void wlr_call(const char *method, GVariant *params, Call *call) {
    /* no timeout: the chooser waits for you */
    g_dbus_connection_call(conn, WLR_NAME, OBJECT_PATH, SC_IFACE, method, params,
                           G_VARIANT_TYPE("(ua{sv})"), G_DBUS_CALL_FLAGS_NONE, G_MAXINT, NULL,
                           wlr_done, call);
}

/* ---- asking you ------------------------------------------------------------------------- */

static char *allowed_file(void) {
    return g_build_filename(g_get_user_state_dir(), "infynitae", "remote-desktop-allowed", NULL);
}

/* apps outside a sandbox have no id: one name for all of them */
static const char *app_key(const char *app_id) {
    return (app_id && *app_id) ? app_id : "(unsandboxed)";
}

static gboolean always_allowed(const char *app_id) {
    char *path = allowed_file(), *text = NULL;
    gboolean yes = FALSE;
    if (g_file_get_contents(path, &text, NULL, NULL)) {
        char **lines = g_strsplit(text, "\n", -1);
        for (char **l = lines; *l && !yes; l++)
            yes = g_strcmp0(g_strstrip(*l), app_key(app_id)) == 0;
        g_strfreev(lines);
    }
    g_free(text);
    g_free(path);
    return yes;
}

static void remember(const char *app_id) {
    char *path = allowed_file(), *dir = g_path_get_dirname(path);
    g_mkdir_with_parents(dir, 0700);
    FILE *f = fopen(path, "a");
    if (f) {
        fprintf(f, "%s\n", app_key(app_id));
        fclose(f);
    }
    g_free(dir);
    g_free(path);
}

static void start_after_consent(Call *call) {
    Session *s = session_find(call->session);
    if (s && s->cast) {
        call->step = STEP_RD_START;
        wlr_call("Start",
                 g_variant_new("(ooss@a{sv})", call->handle, call->session, call->app_id,
                               call->parent, call->options),
                 call);
        return;
    }
    rd_finish_start(call, NULL);
    call_free(call);
}

static void asked(GObject *source, GAsyncResult *res, gpointer data) {
    Call *call = data;
    char *out = NULL;
    g_subprocess_communicate_utf8_finish(G_SUBPROCESS(source), res, &out, NULL, NULL);
    char *answer = g_strstrip(out ? out : g_strdup(""));
    if (g_strcmp0(answer, "always") == 0)
        remember(call->app_id);
    if (g_strcmp0(answer, "always") == 0 || g_strcmp0(answer, "once") == 0) {
        start_after_consent(call);
    } else {
        printf("[infynitae-portal] Remote desktop denied: app='%s'\n", call->app_id);
        fflush(stdout);
        reply(call->inv, 1, NULL);
        call_free(call);
    }
    g_free(answer);
    g_object_unref(source);
}

static void ask(Call *call) {
    char *script = g_build_filename(g_get_home_dir(), "infynitae", "live", "remote",
                                    "remote-desktop-ask", NULL);
    GError *error = NULL;
    GSubprocess *p = g_subprocess_new(G_SUBPROCESS_FLAGS_STDOUT_PIPE, &error, script,
                                      call->app_id, NULL);
    g_free(script);
    if (!p) {
        g_printerr("[infynitae-portal] remote desktop: cannot ask: %s\n", error->message);
        g_clear_error(&error);
        reply(call->inv, 2, NULL);
        call_free(call);
        return;
    }
    g_subprocess_communicate_utf8_async(p, NULL, NULL, asked, call);
}

/* ---- input ------------------------------------------------------------------------------ */

/* the keycode (and whether Shift) that types `keysym` with the seat's keymap, first layout */
static gboolean keysym_to_key(uint32_t keysym, uint32_t *key, gboolean *shift) {
    if (!xkb_keymap)
        return FALSE;
    xkb_keycode_t min = xkb_keymap_min_keycode(xkb_keymap), max = xkb_keymap_max_keycode(xkb_keymap);
    for (xkb_level_index_t level = 0; level < 2; level++) {
        for (xkb_keycode_t kc = min; kc <= max; kc++) {
            const xkb_keysym_t *syms;
            int n = xkb_keymap_key_get_syms_by_level(xkb_keymap, kc, 0, level, &syms);
            for (int i = 0; i < n; i++) {
                if (syms[i] == keysym) {
                    *key = kc - 8;
                    *shift = level == 1;
                    return TRUE;
                }
            }
        }
    }
    return FALSE;
}

#define KEY_LEFTSHIFT 42

static void notify(Session *s, const char *method, GVariant *params) {
    guint32 t = now_ms();
    if (g_strcmp0(method, "NotifyPointerMotion") == 0 && s->pointer) {
        double dx, dy;
        g_variant_get(params, "(o@a{sv}dd)", NULL, NULL, &dx, &dy);
        zwlr_virtual_pointer_v1_motion(s->pointer, t, wl_fixed_from_double(dx), wl_fixed_from_double(dy));
        zwlr_virtual_pointer_v1_frame(s->pointer);
    } else if (g_strcmp0(method, "NotifyPointerMotionAbsolute") == 0) {
        guint32 stream;
        double x, y;
        g_variant_get(params, "(o@a{sv}udd)", NULL, NULL, &stream, &x, &y);
        for (guint i = 0; i < s->streams->len; i++) {
            Stream *st = &g_array_index(s->streams, Stream, i);
            if (st->node == stream && st->pointer && st->w > 0 && st->h > 0) {
                zwlr_virtual_pointer_v1_motion_absolute(st->pointer, t, (uint32_t)CLAMP(x, 0, st->w),
                                                        (uint32_t)CLAMP(y, 0, st->h), st->w, st->h);
                zwlr_virtual_pointer_v1_frame(st->pointer);
            }
        }
    } else if (g_strcmp0(method, "NotifyPointerButton") == 0 && s->pointer) {
        gint32 button;
        guint32 state;
        g_variant_get(params, "(o@a{sv}iu)", NULL, NULL, &button, &state);
        zwlr_virtual_pointer_v1_button(s->pointer, t, button,
                                       state ? WL_POINTER_BUTTON_STATE_PRESSED : WL_POINTER_BUTTON_STATE_RELEASED);
        zwlr_virtual_pointer_v1_frame(s->pointer);
    } else if (g_strcmp0(method, "NotifyPointerAxis") == 0 && s->pointer) {
        GVariant *options;
        double dx, dy;
        gboolean finish = FALSE;
        g_variant_get(params, "(o@a{sv}dd)", NULL, &options, &dx, &dy);
        g_variant_lookup(options, "finish", "b", &finish);
        g_variant_unref(options);
        zwlr_virtual_pointer_v1_axis_source(s->pointer, WL_POINTER_AXIS_SOURCE_FINGER);
        if (dy != 0)
            zwlr_virtual_pointer_v1_axis(s->pointer, t, WL_POINTER_AXIS_VERTICAL_SCROLL, wl_fixed_from_double(dy));
        if (dx != 0)
            zwlr_virtual_pointer_v1_axis(s->pointer, t, WL_POINTER_AXIS_HORIZONTAL_SCROLL, wl_fixed_from_double(dx));
        if (finish) {
            zwlr_virtual_pointer_v1_axis_stop(s->pointer, t, WL_POINTER_AXIS_VERTICAL_SCROLL);
            zwlr_virtual_pointer_v1_axis_stop(s->pointer, t, WL_POINTER_AXIS_HORIZONTAL_SCROLL);
        }
        zwlr_virtual_pointer_v1_frame(s->pointer);
    } else if (g_strcmp0(method, "NotifyPointerAxisDiscrete") == 0 && s->pointer) {
        guint32 axis;
        gint32 steps;
        g_variant_get(params, "(o@a{sv}ui)", NULL, NULL, &axis, &steps);
        zwlr_virtual_pointer_v1_axis_source(s->pointer, WL_POINTER_AXIS_SOURCE_WHEEL);
        zwlr_virtual_pointer_v1_axis_discrete(s->pointer, t,
                                              axis ? WL_POINTER_AXIS_HORIZONTAL_SCROLL : WL_POINTER_AXIS_VERTICAL_SCROLL,
                                              wl_fixed_from_double(steps * 15.0), steps);
        zwlr_virtual_pointer_v1_frame(s->pointer);
    } else if (g_strcmp0(method, "NotifyKeyboardKeycode") == 0 && s->keyboard) {
        gint32 key;
        guint32 state;
        g_variant_get(params, "(o@a{sv}iu)", NULL, NULL, &key, &state);
        zwp_virtual_keyboard_v1_key(s->keyboard, t, key, state ? 1 : 0);
    } else if (g_strcmp0(method, "NotifyKeyboardKeysym") == 0 && s->keyboard) {
        gint32 keysym;
        guint32 state, key;
        gboolean shift;
        g_variant_get(params, "(o@a{sv}iu)", NULL, NULL, &keysym, &state);
        if (keysym_to_key(keysym, &key, &shift)) {
            if (shift && state)
                zwp_virtual_keyboard_v1_key(s->keyboard, t, KEY_LEFTSHIFT, 1);
            zwp_virtual_keyboard_v1_key(s->keyboard, t, key, state ? 1 : 0);
            if (shift && !state)
                zwp_virtual_keyboard_v1_key(s->keyboard, t, KEY_LEFTSHIFT, 0);
        }
    }
    if (wl)
        wl_display_flush(wl);
}

/* ---- the interfaces --------------------------------------------------------------------- */

static Call *call_new(GDBusMethodInvocation *inv, Step step, const char *handle, const char *session,
                      const char *app_id, const char *parent, GVariant *options) {
    Call *call = g_new0(Call, 1);
    call->inv = inv;
    call->step = step;
    call->handle = g_strdup(handle);
    call->session = g_strdup(session);
    call->app_id = g_strdup(app_id);
    call->parent = g_strdup(parent);
    call->options = options ? g_variant_ref(options) : NULL;
    return call;
}

static void method_call(GDBusConnection *c, const gchar *sender, const gchar *path,
                        const gchar *iface, const gchar *method, GVariant *params,
                        GDBusMethodInvocation *inv, gpointer data) {
    if (g_strcmp0(iface, SESSION_IFACE) == 0) {
        Session *s = session_find(path);
        if (s)
            session_end(s, TRUE, FALSE);
        g_dbus_method_invocation_return_value(inv, NULL);
        return;
    }

    if (g_str_has_prefix(method, "Notify")) {
        const char *session;
        g_variant_get_child(params, 0, "&o", &session);
        Session *s = session_find(session);
        if (s && s->remote && s->started)
            notify(s, method, params);
        g_dbus_method_invocation_return_value(inv, NULL);
        return;
    }

    const char *handle, *session, *app_id, *parent = "";
    GVariant *options;
    if (g_strcmp0(method, "Start") == 0)
        g_variant_get(params, "(&o&o&s&s@a{sv})", &handle, &session, &app_id, &parent, &options);
    else
        g_variant_get(params, "(&o&o&s@a{sv})", &handle, &session, &app_id, &options);
    Session *s = session_find(session);

    if (g_strcmp0(iface, RD_IFACE) == 0) {
        if (g_strcmp0(method, "CreateSession") == 0) {
            if (!s)
                session_new(session, app_id, TRUE);
            reply(inv, 0, NULL);
        } else if (g_strcmp0(method, "SelectDevices") == 0) {
            guint32 types = DEVICE_KEYBOARD | DEVICE_POINTER;
            g_variant_lookup(options, "types", "u", &types);
            if (s)
                s->devices = types & (DEVICE_KEYBOARD | DEVICE_POINTER);
            reply(inv, s ? 0 : 2, NULL);
        } else if (g_strcmp0(method, "Start") == 0) {
            if (!s || !s->remote) {
                reply(inv, 2, NULL);
            } else {
                if (!s->devices)
                    s->devices = DEVICE_KEYBOARD | DEVICE_POINTER;
                Call *call = call_new(inv, STEP_RD_START, handle, session, app_id, parent, options);
                if (always_allowed(app_id))
                    start_after_consent(call);
                else
                    ask(call);
            }
        }
    } else if (g_strcmp0(iface, SC_IFACE) == 0) {
        if (g_strcmp0(method, "CreateSession") == 0) {
            wlr_call("CreateSession", params,
                     call_new(inv, STEP_CREATE, handle, session, app_id, NULL, options));
        } else if (g_strcmp0(method, "SelectSources") == 0) {
            if (s && s->remote && !s->cast) {
                /* xdg-desktop-portal-wlr hears of a remote desktop session at its sources */
                char *create = g_strconcat(handle, "_cast", NULL);
                Call *call = call_new(inv, STEP_SELECT_AFTER_CREATE, handle, session, app_id, NULL, options);
                GVariantBuilder none;
                g_variant_builder_init(&none, G_VARIANT_TYPE_VARDICT);
                wlr_call("CreateSession", g_variant_new("(oosa{sv})", create, session, app_id, &none), call);
                g_free(create);
            } else {
                wlr_call("SelectSources", params,
                         call_new(inv, STEP_SELECT, handle, session, app_id, NULL, options));
            }
        } else if (g_strcmp0(method, "Start") == 0) {
            wlr_call("Start", params,
                     call_new(inv, STEP_START, handle, session, app_id, parent, options));
        }
    }
    g_variant_unref(options);
}

/* xdg-desktop-portal-wlr's ScreenCast properties, read once */
static GVariant *wlr_property(const char *prop, guint32 fallback) {
    GVariant *ret = g_dbus_connection_call_sync(conn, WLR_NAME, OBJECT_PATH,
                                                "org.freedesktop.DBus.Properties", "Get",
                                                g_variant_new("(ss)", SC_IFACE, prop),
                                                G_VARIANT_TYPE("(v)"), G_DBUS_CALL_FLAGS_NONE, 3000,
                                                NULL, NULL);
    if (!ret)
        return g_variant_new_uint32(fallback);
    GVariant *v;
    g_variant_get(ret, "(v)", &v);
    g_variant_unref(ret);
    return v;
}

static GVariant *get_property(GDBusConnection *c, const gchar *sender, const gchar *path,
                              const gchar *iface, const gchar *prop, GError **error, gpointer data) {
    if (g_strcmp0(iface, RD_IFACE) == 0) {
        if (g_strcmp0(prop, "AvailableDeviceTypes") == 0)
            return g_variant_new_uint32(DEVICE_KEYBOARD | DEVICE_POINTER);
        return g_variant_new_uint32(1);
    }
    if (g_strcmp0(iface, SC_IFACE) == 0) {
        if (g_strcmp0(prop, "AvailableSourceTypes") == 0)
            return wlr_property(prop, 1);
        if (g_strcmp0(prop, "AvailableCursorModes") == 0)
            return wlr_property(prop, 1 | 2);
        return wlr_property(prop, 4);
    }
    return g_variant_new_uint32(1);
}

void remote_desktop_init(GDBusConnection *connection, const char *object_path) {
    conn = connection;
    GError *error = NULL;
    node = g_dbus_node_info_new_for_xml(rd_xml, &error);
    if (!node) {
        g_printerr("[infynitae-portal] remote desktop: %s\n", error->message);
        g_clear_error(&error);
        return;
    }
    const char *ifaces[] = { RD_IFACE, SC_IFACE };
    for (size_t i = 0; i < 2; i++) {
        if (!g_dbus_connection_register_object(conn, object_path,
                                               g_dbus_node_info_lookup_interface(node, ifaces[i]),
                                               &vtable, NULL, NULL, &error)) {
            g_printerr("[infynitae-portal] %s not registered: %s\n", ifaces[i], error->message);
            g_clear_error(&error);
        }
    }
}
