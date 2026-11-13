/* org.freedesktop.impl.portal.Settings (see settings.h). Owner, 2026-11-06: "i want all the apps to
 * take the theme from the system ... as minimal and lightweight as possible": no daemon of its
 * own, it lives in infynitae-portal, which already watches the config. */
#include "settings.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define APPEARANCE "org.freedesktop.appearance"
#define SETTINGS_IFACE "org.freedesktop.impl.portal.Settings"

static const gchar settings_xml[] =
    "<node>"
    "  <interface name='org.freedesktop.impl.portal.Settings'>"
    "    <method name='ReadAll'>"
    "      <arg type='as' name='namespaces' direction='in'/>"
    "      <arg type='a{sa{sv}}' name='value' direction='out'/>"
    "    </method>"
    "    <method name='Read'>"
    "      <arg type='s' name='namespace' direction='in'/>"
    "      <arg type='s' name='key' direction='in'/>"
    "      <arg type='v' name='value' direction='out'/>"
    "    </method>"
    "    <signal name='SettingChanged'>"
    "      <arg type='s' name='namespace'/>"
    "      <arg type='s' name='key'/>"
    "      <arg type='v' name='value'/>"
    "    </signal>"
    "    <property name='version' type='u' access='read'/>"
    "  </interface>"
    "</node>";

static GDBusConnection *settings_conn;
static char *settings_path;
static char *settings_config;
static PortalAppearance current;

static int hex_digit(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

/* "#rrggbb" (quotes and spaces around allowed) into 0..1; FALSE when it is not one */
static gboolean parse_hex(const char *v, double out[3]) {
    while (*v == ' ' || *v == '"' || *v == '\'') v++;
    if (*v != '#') return FALSE;
    v++;
    for (int i = 0; i < 3; i++) {
        int a = hex_digit(v[2 * i]), b = a < 0 ? -1 : hex_digit(v[2 * i + 1]);
        if (a < 0 || b < 0) return FALSE;
        out[i] = (a * 16 + b) / 255.0;
    }
    return TRUE;
}

/* the value of KEY=... on a line of a shell-style config (unquoted, comments and indented lines
 * skipped); the last one wins, as when the shell sources it */
static gboolean config_value(const char *text, const char *key, char *out, size_t size) {
    gboolean found = FALSE;
    size_t klen = strlen(key);
    for (const char *line = text; line && *line;) {
        const char *end = strchr(line, '\n');
        size_t len = end ? (size_t)(end - line) : strlen(line);
        if (len > klen && strncmp(line, key, klen) == 0 && line[klen] == '=') {
            const char *v = line + klen + 1;
            size_t vlen = len - klen - 1;
            if (vlen && (v[0] == '"' || v[0] == '\'')) {
                char q = v[0];
                v++;
                vlen--;
                const char *close = memchr(v, q, vlen);
                if (close) vlen = (size_t)(close - v);
            } else {
                const char *hash = memchr(v, '#', vlen);
                if (hash && hash > v && (hash[-1] == ' ' || hash[-1] == '\t')) vlen = (size_t)(hash - v);
                while (vlen && (v[vlen - 1] == ' ' || v[vlen - 1] == '\t' || v[vlen - 1] == '\r')) vlen--;
            }
            if (vlen >= size) vlen = size - 1;
            memcpy(out, v, vlen);
            out[vlen] = '\0';
            found = TRUE;
        }
        line = end ? end + 1 : NULL;
    }
    return found;
}

void portal_appearance_read(const char *config_path, PortalAppearance *out) {
    out->color_scheme = 1; /* COLOR_SCHEME's default: prefer-dark */
    parse_hex("#7aa2f7", out->accent);
    out->contrast = 0;
    gchar *text = NULL;
    if (!config_path || !g_file_get_contents(config_path, &text, NULL, NULL)) return;
    char v[128];
    if (config_value(text, "COLOR_SCHEME", v, sizeof(v))) {
        if (strcmp(v, "prefer-light") == 0 || strcmp(v, "light") == 0) out->color_scheme = 2;
        else if (strcmp(v, "default") == 0 || strcmp(v, "none") == 0) out->color_scheme = 0;
        else out->color_scheme = 1;
    }
    /* ACCENT drives apps; fall back to BORDER_ACTIVE for configs written before ACCENT existed */
    double rgb[3];
    if (config_value(text, "ACCENT", v, sizeof(v)) && parse_hex(v, rgb)) {
        memcpy(out->accent, rgb, sizeof(rgb));
    } else if (config_value(text, "BORDER_ACTIVE", v, sizeof(v)) && parse_hex(v, rgb)) {
        memcpy(out->accent, rgb, sizeof(rgb));
    }
    g_free(text);
}

static GVariant *appearance_value(const PortalAppearance *a, const char *key) {
    if (strcmp(key, "color-scheme") == 0) return g_variant_new_uint32(a->color_scheme);
    if (strcmp(key, "accent-color") == 0) return g_variant_new("(ddd)", a->accent[0], a->accent[1], a->accent[2]);
    if (strcmp(key, "contrast") == 0) return g_variant_new_uint32(a->contrast);
    return NULL;
}

static const char *const KEYS[] = {"color-scheme", "accent-color", "contrast"};

/* a namespace pattern from ReadAll: exact, or ending in '*' as a prefix; none means all */
static gboolean namespace_wanted(const gchar *const *patterns) {
    if (!patterns || !patterns[0]) return TRUE;
    for (int i = 0; patterns[i]; i++) {
        const char *p = patterns[i];
        size_t n = strlen(p);
        if (n == 0) return TRUE;
        if (p[n - 1] == '*' ? strncmp(APPEARANCE, p, n - 1) == 0 : strcmp(APPEARANCE, p) == 0) return TRUE;
    }
    return FALSE;
}

static void settings_method(GDBusConnection *connection, const gchar *sender, const gchar *object_path,
                            const gchar *interface_name, const gchar *method_name, GVariant *parameters,
                            GDBusMethodInvocation *invocation, gpointer user_data) {
    (void)connection; (void)sender; (void)object_path; (void)interface_name; (void)user_data;
    if (g_strcmp0(method_name, "ReadAll") == 0) {
        const gchar **patterns = NULL;
        g_variant_get(parameters, "(^a&s)", &patterns);
        GVariantBuilder all;
        g_variant_builder_init(&all, G_VARIANT_TYPE("a{sa{sv}}"));
        if (namespace_wanted(patterns)) {
            GVariantBuilder ns;
            g_variant_builder_init(&ns, G_VARIANT_TYPE("a{sv}"));
            for (size_t i = 0; i < G_N_ELEMENTS(KEYS); i++)
                g_variant_builder_add(&ns, "{sv}", KEYS[i], appearance_value(&current, KEYS[i]));
            g_variant_builder_add(&all, "{sa{sv}}", APPEARANCE, &ns);
        }
        g_free(patterns);
        g_dbus_method_invocation_return_value(invocation, g_variant_new("(a{sa{sv}})", &all));
    } else if (g_strcmp0(method_name, "Read") == 0) {
        const gchar *ns = NULL, *key = NULL;
        g_variant_get(parameters, "(&s&s)", &ns, &key);
        GVariant *v = g_strcmp0(ns, APPEARANCE) == 0 ? appearance_value(&current, key) : NULL;
        if (!v) {
            g_dbus_method_invocation_return_dbus_error(invocation, "org.freedesktop.portal.Error.NotFound",
                                                       "Requested setting not found");
            return;
        }
        g_dbus_method_invocation_return_value(invocation, g_variant_new("(v)", v));
    } else {
        g_dbus_method_invocation_return_dbus_error(invocation, "org.freedesktop.DBus.Error.UnknownMethod",
                                                   "Unknown method");
    }
}

static GVariant *settings_property(GDBusConnection *connection, const gchar *sender, const gchar *object_path,
                                   const gchar *interface_name, const gchar *property_name, GError **error,
                                   gpointer user_data) {
    (void)connection; (void)sender; (void)object_path; (void)interface_name; (void)user_data;
    if (g_strcmp0(property_name, "version") == 0) return g_variant_new_uint32(1);
    g_set_error(error, G_DBUS_ERROR, G_DBUS_ERROR_UNKNOWN_PROPERTY, "No such property");
    return NULL;
}

static const GDBusInterfaceVTable settings_vtable = {settings_method, settings_property, NULL, {0}};

void portal_settings_init(GDBusConnection *connection, const char *path, const char *config_path) {
    settings_conn = connection;
    g_free(settings_path);
    settings_path = g_strdup(path);
    g_free(settings_config);
    settings_config = g_strdup(config_path);
    portal_appearance_read(settings_config, &current);
    GError *error = NULL;
    GDBusNodeInfo *info = g_dbus_node_info_new_for_xml(settings_xml, &error);
    if (!info) {
        fprintf(stderr, "[infynitae-portal] Settings: %s\n", error->message);
        g_error_free(error);
        return;
    }
    if (!g_dbus_connection_register_object(connection, path, info->interfaces[0], &settings_vtable, NULL, NULL,
                                           &error)) {
        fprintf(stderr, "[infynitae-portal] Settings not registered: %s\n", error->message);
        g_error_free(error);
    } else {
        printf("[infynitae-portal] Registered %s on %s\n", SETTINGS_IFACE, path);
        fflush(stdout);
    }
    g_dbus_node_info_unref(info);
}

void portal_settings_config_changed(void) {
    if (!settings_conn) return;
    PortalAppearance now;
    portal_appearance_read(settings_config, &now);
    PortalAppearance before = current;
    current = now;
    for (size_t i = 0; i < G_N_ELEMENTS(KEYS); i++) {
        GVariant *a = appearance_value(&before, KEYS[i]), *b = appearance_value(&now, KEYS[i]);
        g_variant_ref_sink(a);
        g_variant_ref_sink(b);
        if (!g_variant_equal(a, b))
            g_dbus_connection_emit_signal(settings_conn, NULL, settings_path, SETTINGS_IFACE, "SettingChanged",
                                          g_variant_new("(ssv)", APPEARANCE, KEYS[i], b), NULL);
        g_variant_unref(a);
        g_variant_unref(b);
    }
}
