/*
 * tools/bus-stub: logind and UDisks2 for tools/test-sleep-lock and tools/test-automount, on a
 * private bus (DBUS_SYSTEM_BUS_ADDRESS), so live/bus-watch is tested without the real ones.
 *   bus-stub DIR < commands
 * It owns org.freedesktop.login1 and org.freedesktop.UDisks2 and then writes DIR/ready. login1's
 * Inhibit hands out one end of a pipe; DIR/inhibitors holds how many are open, written again
 * when a caller closes its end. Each line read is a signal sent: "PrepareForSleep true" (or
 * false), or a UDisks2 signal as `gdbus monitor` prints it.
 */

#include <gio/gio.h>
#include <gio/gunixfdlist.h>
#include <gio/gunixinputstream.h>
#include <glib-unix.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static const char *dir;
static int open_inhibitors;
static int owned;
static GDBusConnection *bus;
static GDataInputStream *input;

static void write_file(const char *name, const char *text) {
    char *path = g_build_filename(dir, name, NULL);
    char *tmp = g_strconcat(path, ".new", NULL);
    g_file_set_contents(tmp, text, -1, NULL);
    rename(tmp, path);
    g_free(tmp);
    g_free(path);
}

static void write_count(void) {
    char text[16];
    snprintf(text, sizeof text, "%d\n", open_inhibitors);
    write_file("inhibitors", text);
}

// the caller's end closed: the inhibitor is gone, as logind sees it
static gboolean on_inhibitor_closed(int fd, GIOCondition cond, gpointer data) {
    close(fd);
    open_inhibitors--;
    write_count();
    return G_SOURCE_REMOVE;
}

static void on_call(GDBusConnection *c, const char *sender, const char *path, const char *iface,
                    const char *method, GVariant *params, GDBusMethodInvocation *inv, gpointer data) {
    int ends[2];
    if (strcmp(method, "Inhibit") != 0 || pipe(ends) != 0) {
        g_dbus_method_invocation_return_dbus_error(inv, "org.freedesktop.DBus.Error.Failed", "not here");
        return;
    }
    g_unix_fd_add(ends[0], G_IO_HUP | G_IO_ERR, on_inhibitor_closed, NULL);
    GUnixFDList *fds = g_unix_fd_list_new_from_array(&ends[1], 1);
    open_inhibitors++;
    write_count();
    g_dbus_method_invocation_return_value_with_unix_fd_list(inv, g_variant_new("(h)", 0), fds);
    g_object_unref(fds);
}

static void read_next(void);

static void on_line(GObject *src, GAsyncResult *res, gpointer data) {
    char *line = g_data_input_stream_read_line_finish(input, res, NULL, NULL);
    if (!line)
        exit(0);
    GError *err = NULL;
    if (g_str_has_prefix(line, "PrepareForSleep ")) {
        gboolean going = strcmp(line + strlen("PrepareForSleep "), "true") == 0;
        g_dbus_connection_emit_signal(bus, NULL, "/org/freedesktop/login1", "org.freedesktop.login1.Manager",
                                      "PrepareForSleep", g_variant_new("(b)", going), &err);
    } else {
        // "PATH: INTERFACE.MEMBER (ARGS)"
        char *colon = strstr(line, ": ");
        char *space = colon ? strchr(colon + 2, ' ') : NULL;
        char *dot = space ? g_strrstr_len(colon, space - colon, ".") : NULL;
        if (dot) {
            *colon = *dot = *space = '\0';
            const char *type = strcmp(dot + 1, "InterfacesRemoved") == 0 ? "(oas)" : "(oa{sa{sv}})";
            GVariant *args = g_variant_parse(G_VARIANT_TYPE(type), space + 1, NULL, NULL, &err);
            if (args)
                g_dbus_connection_emit_signal(bus, NULL, line, colon + 2, dot + 1, args, &err);
        }
    }
    if (err) {
        g_printerr("bus-stub: %s\n", err->message);
        g_error_free(err);
    }
    g_dbus_connection_flush_sync(bus, NULL, NULL);
    g_free(line);
    read_next();
}

static void read_next(void) {
    g_data_input_stream_read_line_async(input, G_PRIORITY_DEFAULT, NULL, on_line, NULL);
}

static void on_owned(GDBusConnection *c, const char *name, gpointer data) {
    if (++owned == 2) {
        write_file("ready", "");
        read_next();
    }
}

static void on_lost(GDBusConnection *c, const char *name, gpointer data) {
    g_printerr("bus-stub: %s is not ours\n", name);
    exit(1);
}

int main(int argc, char **argv) {
    if (argc != 2) {
        g_printerr("usage: bus-stub DIR < commands\n");
        return 2;
    }
    dir = argv[1];
    GError *err = NULL;
    bus = g_bus_get_sync(G_BUS_TYPE_SYSTEM, NULL, &err);
    if (!bus) {
        g_printerr("bus-stub: %s\n", err->message);
        return 1;
    }
    GDBusNodeInfo *node = g_dbus_node_info_new_for_xml(
        "<node><interface name='org.freedesktop.login1.Manager'>"
        "<method name='Inhibit'><arg type='s' direction='in'/><arg type='s' direction='in'/>"
        "<arg type='s' direction='in'/><arg type='s' direction='in'/><arg type='h' direction='out'/></method>"
        "</interface></node>",
        NULL);
    static const GDBusInterfaceVTable vtable = {.method_call = on_call};
    g_dbus_connection_register_object(bus, "/org/freedesktop/login1", node->interfaces[0], &vtable, NULL, NULL, NULL);
    write_count();
    input = g_data_input_stream_new(g_unix_input_stream_new(0, FALSE));
    g_bus_own_name_on_connection(bus, "org.freedesktop.login1", G_BUS_NAME_OWNER_FLAGS_NONE, on_owned, on_lost, NULL, NULL);
    g_bus_own_name_on_connection(bus, "org.freedesktop.UDisks2", G_BUS_NAME_OWNER_FLAGS_NONE, on_owned, on_lost, NULL, NULL);
    g_main_loop_run(g_main_loop_new(NULL, FALSE));
    return 0;
}
