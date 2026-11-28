/*
 * org.freedesktop.impl.portal.Screenshot for Infynitae's portal (xdg-desktop-portal-wlr's before
 * 2026-11-25): an app's screenshot is taken by infynitae-shot, the whole screen under the pointer,
 * or a region or window you pick when the app asks for an interactive one. xdg-desktop-portal
 * asks whether the app may take it. The picture goes to $XDG_RUNTIME_DIR/infynitae/screenshot.png
 * (the next one replaces it); xdg-desktop-portal hands it to a sandboxed app. PickColor is not
 * offered: an app asking for it gets an error.
 */

#include <gio/gio.h>
#include <stdio.h>

#include "remote-desktop.h"
#include "screenshot.h"

#define SS_IFACE "org.freedesktop.impl.portal.Screenshot"

static const gchar ss_xml[] =
  "<node>"
  "  <interface name='org.freedesktop.impl.portal.Screenshot'>"
  "    <property name='version' type='u' access='read'/>"
  "    <method name='Screenshot'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='s' name='parent_window' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "  </interface>"
  "</node>";

typedef struct {
    GDBusMethodInvocation *inv;
    char *file;
} Shot;

static void shot_done(GObject *source, GAsyncResult *res, gpointer data) {
    Shot *shot = data;
    GError *error = NULL;
    GVariantBuilder results;
    g_variant_builder_init(&results, G_VARIANT_TYPE_VARDICT);
    guint32 response = 0;
    if (g_subprocess_wait_check_finish(G_SUBPROCESS(source), res, &error)) {
        char *uri = g_filename_to_uri(shot->file, NULL, NULL);
        g_variant_builder_add(&results, "{sv}", "uri", g_variant_new_string(uri));
        g_free(uri);
    } else {
        /* infynitae-shot ends with 1 when the pick was cancelled, and says why on a failure */
        printf("[infynitae-portal] Screenshot not taken: %s\n", error->message);
        fflush(stdout);
        g_clear_error(&error);
        response = 1;
    }
    g_dbus_method_invocation_return_value(shot->inv, g_variant_new("(ua{sv})", response, &results));
    g_object_unref(source);
    g_free(shot->file);
    g_free(shot);
}

static void method_call(GDBusConnection *c, const gchar *sender, const gchar *path,
                        const gchar *iface, const gchar *method, GVariant *params,
                        GDBusMethodInvocation *inv, gpointer data) {
    if (!from_portal(c, sender)) {
        g_dbus_method_invocation_return_dbus_error(inv, "org.freedesktop.DBus.Error.AccessDenied",
                                                   "only xdg-desktop-portal may call this");
        return;
    }
    GVariant *options;
    gboolean interactive = FALSE;
    g_variant_get(params, "(&o&s&s@a{sv})", NULL, NULL, NULL, &options);
    g_variant_lookup(options, "interactive", "b", &interactive);
    g_variant_unref(options);

    char *dir = g_build_filename(g_get_user_runtime_dir(), "infynitae", NULL);
    g_mkdir_with_parents(dir, 0700);
    Shot *shot = g_new0(Shot, 1);
    shot->inv = inv;
    shot->file = g_build_filename(dir, "screenshot.png", NULL);
    g_free(dir);
    char *bin = g_build_filename(g_get_home_dir(), ".local", "bin", "infynitae-shot", NULL);
    GError *error = NULL;
    GSubprocess *p = g_subprocess_new(G_SUBPROCESS_FLAGS_NONE, &error, bin,
                                      interactive ? "region" : "screen", shot->file, NULL);
    g_free(bin);
    if (!p) {
        g_printerr("[infynitae-portal] Screenshot: %s\n", error->message);
        g_clear_error(&error);
        g_dbus_method_invocation_return_value(inv, g_variant_new_parsed("(@u 2, @a{sv} {})"));
        g_free(shot->file);
        g_free(shot);
        return;
    }
    g_subprocess_wait_check_async(p, NULL, shot_done, shot);
}

static GVariant *get_property(GDBusConnection *c, const gchar *sender, const gchar *path,
                              const gchar *iface, const gchar *prop, GError **error, gpointer data) {
    return g_variant_new_uint32(1);
}

static const GDBusInterfaceVTable vtable = { method_call, get_property, NULL, { 0 } };

void screenshot_init(GDBusConnection *conn, const char *object_path) {
    GError *error = NULL;
    GDBusNodeInfo *node = g_dbus_node_info_new_for_xml(ss_xml, &error);
    if (node && g_dbus_connection_register_object(conn, object_path, node->interfaces[0], &vtable,
                                                  NULL, NULL, &error))
        return;
    g_printerr("[infynitae-portal] %s not registered: %s\n", SS_IFACE, error->message);
    g_clear_error(&error);
}
