/*
 * Infynitae GlobalShortcuts XDG Desktop Portal Implementation
 * Pure C + GIO. Zero bloat (<2MB RAM, 0% CPU idle).
 * Implements org.freedesktop.impl.portal.GlobalShortcuts for Discord, OBS, WebRTC.
 */

#include <gio/gio.h>
#include <gio/gunixsocketaddress.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>

#define BUS_NAME "org.freedesktop.impl.portal.desktop.infynitae"
#define OBJECT_PATH "/org/freedesktop/portal/desktop"
#define INTERFACE_NAME "org.freedesktop.impl.portal.GlobalShortcuts"

#define PORTAL_MOD_SHIFT (1 << 0)
#define PORTAL_MOD_CTRL  (1 << 1)
#define PORTAL_MOD_ALT   (1 << 2)
#define PORTAL_MOD_LOGO  (1 << 3)

typedef struct {
    char *id;
    char *description;
    char *trigger;
    uint32_t evdev_key;
    uint32_t modifiers;
    gboolean active;
} ShortcutEntry;

typedef struct {
    char *session_handle;
    char *app_id;
    GList *shortcuts; // List of ShortcutEntry*
} SessionEntry;

static GList *sessions = NULL; // List of SessionEntry*
static GDBusConnection *dbus_conn = NULL;
static GDBusNodeInfo *global_node_info = NULL;

static const gchar introspection_xml[] =
  "<node>"
  "  <interface name='org.freedesktop.impl.portal.GlobalShortcuts'>"
  "    <property name='version' type='u' access='read'/>"
  "    <method name='CreateSession'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='BindShortcuts'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='a(sa{sv})' name='shortcuts' direction='in'/>"
  "      <arg type='s' name='parent_window' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='ListShortcuts'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='ConfigureShortcuts'>"
  "      <arg type='o' name='session_handle' direction='in'/>"
  "      <arg type='s' name='parent_window' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "    </method>"
  "    <signal name='Activated'>"
  "      <arg type='o' name='session_handle' direction='out'/>"
  "      <arg type='s' name='shortcut_id' direction='out'/>"
  "      <arg type='t' name='timestamp' direction='out'/>"
  "      <arg type='a{sv}' name='options' direction='out'/>"
  "    </signal>"
  "    <signal name='Deactivated'>"
  "      <arg type='o' name='session_handle' direction='out'/>"
  "      <arg type='s' name='shortcut_id' direction='out'/>"
  "      <arg type='t' name='timestamp' direction='out'/>"
  "      <arg type='a{sv}' name='options' direction='out'/>"
  "    </signal>"
  "    <signal name='ShortcutsChanged'>"
  "      <arg type='o' name='session_handle' direction='out'/>"
  "      <arg type='a(sa{sv})' name='shortcuts' direction='out'/>"
  "    </signal>"
  "  </interface>"
  "  <interface name='org.freedesktop.impl.portal.Session'>"
  "    <property name='version' type='u' access='read'/>"
  "    <method name='Close'/>"
  "    <signal name='Closed'/>"
  "  </interface>"
  "</node>";

static SessionEntry *find_session(const char *session_handle) {
    for (GList *l = sessions; l != NULL; l = l->next) {
        SessionEntry *s = (SessionEntry *)l->data;
        if (g_strcmp0(s->session_handle, session_handle) == 0) {
            return s;
        }
    }
    return NULL;
}

static void free_shortcut(ShortcutEntry *sc) {
    if (!sc) return;
    g_free(sc->id);
    g_free(sc->description);
    g_free(sc->trigger);
    g_free(sc);
}

static void free_session(SessionEntry *s) {
    if (!s) return;
    g_free(s->session_handle);
    g_free(s->app_id);
    g_list_free_full(s->shortcuts, (GDestroyNotify)free_shortcut);
    g_free(s);
}

static uint32_t trigger_to_evdev(const char *trigger) {
    if (!trigger || !*trigger) return 0;
    // Specific virtual triggers from Discord lone modifier wrapper
    if (strstr(trigger, "F15") || strstr(trigger, "F23") || strstr(trigger, "Shift_R") || strstr(trigger, "ShiftRight")) return 54;
    if (strstr(trigger, "F16") || strstr(trigger, "F24") || strstr(trigger, "Control_R") || strstr(trigger, "ControlRight")) return 97;
    if (strstr(trigger, "F13") || strstr(trigger, "F21") || strstr(trigger, "Shift_L") || strstr(trigger, "ShiftLeft")) return 42;
    if (strstr(trigger, "F14") || strstr(trigger, "F22") || strstr(trigger, "Control_L") || strstr(trigger, "ControlLeft")) return 29;
    if (strstr(trigger, "Alt_R") || strstr(trigger, "AltRight")) return 100;
    if (strstr(trigger, "Alt_L") || strstr(trigger, "AltLeft")) return 56;
    if (strstr(trigger, "Pause")) return 119;
    if (strstr(trigger, "Scroll_Lock")) return 70;
    if (strstr(trigger, "Insert")) return 110;
    if (strstr(trigger, "Delete")) return 111;
    if (strstr(trigger, "Home")) return 102;
    if (strstr(trigger, "End")) return 107;
    if (strstr(trigger, "Page_Up") || strstr(trigger, "Prior")) return 104;
    if (strstr(trigger, "Page_Down") || strstr(trigger, "Next")) return 109;
    if (strstr(trigger, "space") || strstr(trigger, "Space")) return 57;
    if (strstr(trigger, "Return") || strstr(trigger, "Enter")) return 28;

    // F1 - F12 (match longest prefix first so F10 doesn't match F1)
    for (int i = 12; i >= 1; i--) {
        char f_name[8];
        snprintf(f_name, sizeof(f_name), "F%d", i);
        const char *p = strstr(trigger, f_name);
        if (p) {
            char next = p[strlen(f_name)];
            if (next >= '0' && next <= '9') continue;
            if (i <= 10) return 58 + i;
            if (i == 11) return 87;
            if (i == 12) return 88;
        }
    }

    // Extract base key token (the part after the last '+' or ' ')
    const char *base = strrchr(trigger, '+');
    base = base ? base + 1 : trigger;
    while (*base == ' ') base++;

    // Single letter a-z / A-Z
    if ((base[0] >= 'a' && base[0] <= 'z' && base[1] == '\0') ||
        (base[0] >= 'A' && base[0] <= 'Z' && base[1] == '\0')) {
        char c = (base[0] >= 'A' && base[0] <= 'Z') ? (char)(base[0] + 32) : base[0];
        static const uint32_t az_map[26] = {
            30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50,
            49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44
        };
        return az_map[c - 'a'];
    }

    // Digits 0-9
    if (base[0] >= '0' && base[0] <= '9' && base[1] == '\0') {
        if (base[0] == '0') return 11;
        return 1 + (base[0] - '0');
    }

    return 0;
}

static uint32_t trigger_to_modifiers(const char *trigger) {
    if (!trigger || !*trigger) return 0;
    if (!strchr(trigger, '+')) return 0;

    uint32_t mods = 0;
    gchar **tokens = g_strsplit(trigger, "+", -1);
    if (!tokens) return 0;

    int count = g_strv_length(tokens);
    for (int i = 0; i < count - 1; i++) {
        gchar *tok = g_strstrip(tokens[i]);
        if (g_ascii_strcasecmp(tok, "ctrl") == 0 || g_ascii_strcasecmp(tok, "control") == 0) {
            mods |= PORTAL_MOD_CTRL;
        } else if (g_ascii_strcasecmp(tok, "shift") == 0) {
            mods |= PORTAL_MOD_SHIFT;
        } else if (g_ascii_strcasecmp(tok, "alt") == 0 || g_ascii_strcasecmp(tok, "mod1") == 0) {
            mods |= PORTAL_MOD_ALT;
        } else if (g_ascii_strcasecmp(tok, "super") == 0 || g_ascii_strcasecmp(tok, "meta") == 0 ||
                   g_ascii_strcasecmp(tok, "logo") == 0 || g_ascii_strcasecmp(tok, "mod4") == 0 ||
                   g_ascii_strcasecmp(tok, "win") == 0) {
            mods |= PORTAL_MOD_LOGO;
        }
    }
    g_strfreev(tokens);
    return mods;
}

static void sync_bound_keys_file(void) {
    const char *runtime_dir = g_get_user_runtime_dir();
    char path[256];
    char tmp_path[256];
    snprintf(path, sizeof(path), "%s/infynitae/bound_keys", runtime_dir);
    snprintf(tmp_path, sizeof(tmp_path), "%s/infynitae/bound_keys.tmp", runtime_dir);

    FILE *f = fopen(tmp_path, "w");
    if (!f) return;

    for (GList *sl = sessions; sl != NULL; sl = sl->next) {
        SessionEntry *sess = (SessionEntry *)sl->data;
        for (GList *k = sess->shortcuts; k != NULL; k = k->next) {
            ShortcutEntry *sc = (ShortcutEntry *)k->data;
            if (sc->evdev_key > 0) {
                fprintf(f, "%u\n", sc->evdev_key);
            }
        }
    }
    fclose(f);
    rename(tmp_path, path);
}

static void emit_portal_signal(const char *session_handle, const char *shortcut_id, const char *signal_name) {
    if (!dbus_conn || !session_handle || !shortcut_id) return;
    guint64 now_ms = (guint64)(g_get_monotonic_time() / 1000);
    GVariantBuilder empty_opts;
    g_variant_builder_init(&empty_opts, G_VARIANT_TYPE("a{sv}"));

    GVariant *params = g_variant_new("(ost@a{sv})",
                                     session_handle,
                                     shortcut_id,
                                     now_ms,
                                     g_variant_builder_end(&empty_opts));
    g_dbus_connection_emit_signal(dbus_conn,
                                  NULL,
                                  OBJECT_PATH,
                                  INTERFACE_NAME,
                                  signal_name,
                                  params,
                                  NULL);
    printf("[infynitae-portal] %s shortcut '%s' on session '%s'\n", signal_name, shortcut_id, session_handle);
    fflush(stdout);
}

static void trigger_shortcut(const char *session_handle, const char *shortcut_id) {
    emit_portal_signal(session_handle, shortcut_id, "Activated");
    emit_portal_signal(session_handle, shortcut_id, "Deactivated");
}

static void dispatch_action(const char *action) {
    if (!action || strlen(action) == 0) return;
    char act[128];
    strncpy(act, action, sizeof(act) - 1);
    act[sizeof(act) - 1] = '\0';
    g_strstrip(act);

    // Format 1: "key:<evdev_code>:<pressed: 1 or 0>[:<modifiers>]"
    if (strncmp(act, "key:", 4) == 0) {
        uint32_t key_code = 0;
        int pressed = 1;
        uint32_t key_mods = 0;
        int parsed = sscanf(act + 4, "%u:%d:%u", &key_code, &pressed, &key_mods);
        if (parsed >= 1) {
            for (GList *sl = sessions; sl != NULL; sl = sl->next) {
                SessionEntry *sess = (SessionEntry *)sl->data;
                for (GList *k = sess->shortcuts; k != NULL; k = k->next) {
                    ShortcutEntry *sc = (ShortcutEntry *)k->data;
                    if (sc->evdev_key == key_code) {
                        if (pressed) {
                            if (parsed >= 3 && sc->modifiers != key_mods) {
                                continue;
                            }
                            sc->active = TRUE;
                            emit_portal_signal(sess->session_handle, sc->id, "Activated");
                        } else {
                            if (sc->active) {
                                sc->active = FALSE;
                                emit_portal_signal(sess->session_handle, sc->id, "Deactivated");
                            } else if (parsed < 3 || sc->modifiers == key_mods) {
                                emit_portal_signal(sess->session_handle, sc->id, "Deactivated");
                            }
                        }
                    }
                }
            }
            return;
        }
    }

    // Format 2: legacy "mute" / "deafen" strings from launch.sh
    gboolean is_mute = (g_ascii_strcasecmp(act, "mute") == 0 || g_ascii_strcasecmp(act, "toggle-mute") == 0);
    gboolean is_deafen = (g_ascii_strcasecmp(act, "deafen") == 0 || g_ascii_strcasecmp(act, "toggle-deafen") == 0);

    for (GList *sl = sessions; sl != NULL; sl = sl->next) {
        SessionEntry *sess = (SessionEntry *)sl->data;
        if (!sess->shortcuts) continue;

        ShortcutEntry *best_match = NULL;
        for (GList *k = sess->shortcuts; k != NULL; k = k->next) {
            ShortcutEntry *sc = (ShortcutEntry *)k->data;
            gchar *desc_lower = g_ascii_strdown(sc->description ? sc->description : "", -1);
            gchar *id_lower = g_ascii_strdown(sc->id ? sc->id : "", -1);

            if (is_mute) {
                if (strstr(desc_lower, "mute") || strstr(id_lower, "mute") ||
                    (sc->trigger && strstr(sc->trigger, "Shift_R")) || sc->evdev_key == 54) {
                    best_match = sc;
                    g_free(desc_lower);
                    g_free(id_lower);
                    break;
                }
            } else if (is_deafen) {
                if (strstr(desc_lower, "deaf") || strstr(id_lower, "deaf") ||
                    (sc->trigger && strstr(sc->trigger, "Control_R")) || sc->evdev_key == 97) {
                    best_match = sc;
                    g_free(desc_lower);
                    g_free(id_lower);
                    break;
                }
            } else {
                if (g_ascii_strcasecmp(id_lower, act) == 0 ||
                    strstr(desc_lower, act)) {
                    best_match = sc;
                    g_free(desc_lower);
                    g_free(id_lower);
                    break;
                }
            }
            g_free(desc_lower);
            g_free(id_lower);
        }

        // Fallback for sessions with 1 or 2 generic keybinds
        if (!best_match) {
            guint len = g_list_length(sess->shortcuts);
            if (is_mute && len >= 1) {
                best_match = (ShortcutEntry *)g_list_nth_data(sess->shortcuts, 0);
            } else if (is_deafen && len >= 2) {
                best_match = (ShortcutEntry *)g_list_nth_data(sess->shortcuts, 1);
            }
        }

        if (best_match) {
            trigger_shortcut(sess->session_handle, best_match->id);
        }
    }
}

// -----------------------------------------------------------------------------
// D-Bus Method Handling
// -----------------------------------------------------------------------------

static const GDBusInterfaceVTable vtable;

static void handle_method_call(GDBusConnection *connection,
                               const gchar *sender,
                               const gchar *object_path,
                               const gchar *interface_name,
                               const gchar *method_name,
                               GVariant *parameters,
                               GDBusMethodInvocation *invocation,
                               gpointer user_data) {
    if (g_strcmp0(interface_name, INTERFACE_NAME) == 0) {
        if (g_strcmp0(method_name, "CreateSession") == 0) {
            const gchar *handle = NULL;
            const gchar *session_handle = NULL;
            const gchar *app_id = NULL;
            GVariant *options = NULL;
            g_variant_get(parameters, "(&o&o&s@a{sv})", &handle, &session_handle, &app_id, &options);

            SessionEntry *sess = find_session(session_handle);
            if (!sess) {
                sess = g_new0(SessionEntry, 1);
                sess->session_handle = g_strdup(session_handle);
                sess->app_id = g_strdup(app_id);
                sessions = g_list_append(sessions, sess);
            }

            if (global_node_info) {
                GDBusInterfaceInfo *sess_iface = g_dbus_node_info_lookup_interface(global_node_info, "org.freedesktop.impl.portal.Session");
                if (sess_iface) {
                    g_dbus_connection_register_object(connection,
                                                      session_handle,
                                                      sess_iface,
                                                      &vtable,
                                                      NULL,
                                                      NULL,
                                                      NULL);
                }
            }

            printf("[infynitae-portal] CreateSession: app='%s', session='%s'\n", app_id, session_handle);
            fflush(stdout);

            GVariantBuilder results;
            g_variant_builder_init(&results, G_VARIANT_TYPE("a{sv}"));
            g_dbus_method_invocation_return_value(invocation, g_variant_new("(ua{sv})", 0, &results));
            if (options) g_variant_unref(options);
            return;
        }

        if (g_strcmp0(method_name, "BindShortcuts") == 0) {
            const gchar *handle = NULL;
            const gchar *session_handle = NULL;
            GVariant *shortcuts_v = NULL;
            const gchar *parent_window = NULL;
            GVariant *options = NULL;
            g_variant_get(parameters, "(&o&o@a(sa{sv})&s@a{sv})", &handle, &session_handle, &shortcuts_v, &parent_window, &options);

            SessionEntry *sess = find_session(session_handle);
            if (!sess) {
                sess = g_new0(SessionEntry, 1);
                sess->session_handle = g_strdup(session_handle);
                sess->app_id = g_strdup("unknown");
                sessions = g_list_append(sessions, sess);
            }

            // Clear previous shortcuts
            g_list_free_full(sess->shortcuts, (GDestroyNotify)free_shortcut);
            sess->shortcuts = NULL;

            GVariantBuilder bound_shortcuts_builder;
            g_variant_builder_init(&bound_shortcuts_builder, G_VARIANT_TYPE("a(sa{sv})"));

            GVariantIter iter;
            g_variant_iter_init(&iter, shortcuts_v);
            const gchar *sc_id = NULL;
            GVariant *sc_opts = NULL;

            while (g_variant_iter_next(&iter, "(&s@a{sv})", &sc_id, &sc_opts)) {
                ShortcutEntry *sc = g_new0(ShortcutEntry, 1);
                sc->id = g_strdup(sc_id);

                const gchar *desc = "";
                const gchar *pref_trigger = "";
                g_variant_lookup(sc_opts, "description", "&s", &desc);
                g_variant_lookup(sc_opts, "preferred_trigger", "&s", &pref_trigger);

                sc->description = g_strdup(desc);
                sc->trigger = g_strdup(pref_trigger);
                sc->evdev_key = trigger_to_evdev(pref_trigger);
                sc->modifiers = trigger_to_modifiers(pref_trigger);
                sc->active = FALSE;
                sess->shortcuts = g_list_append(sess->shortcuts, sc);

                // Build returned shortcut with trigger_description
                GVariantBuilder ret_sc_opts;
                g_variant_builder_init(&ret_sc_opts, G_VARIANT_TYPE("a{sv}"));
                g_variant_builder_add(&ret_sc_opts, "{sv}", "description", g_variant_new_string(desc));
                g_variant_builder_add(&ret_sc_opts, "{sv}", "trigger_description", g_variant_new_string(pref_trigger));

                g_variant_builder_add(&bound_shortcuts_builder, "(sa{sv})", sc_id, &ret_sc_opts);

                printf("[infynitae-portal] Bound shortcut '%s': desc='%s', trigger='%s' (evdev=%u)\n", sc_id, desc, pref_trigger, sc->evdev_key);
                g_variant_unref(sc_opts);
            }

            sync_bound_keys_file();

            GVariantBuilder results;
            g_variant_builder_init(&results, G_VARIANT_TYPE("a{sv}"));
            g_variant_builder_add(&results, "{sv}", "shortcuts", g_variant_builder_end(&bound_shortcuts_builder));

            g_dbus_method_invocation_return_value(invocation, g_variant_new("(ua{sv})", 0, &results));

            if (shortcuts_v) g_variant_unref(shortcuts_v);
            if (options) g_variant_unref(options);
            fflush(stdout);
            return;
        }

        if (g_strcmp0(method_name, "ListShortcuts") == 0) {
            const gchar *handle = NULL;
            const gchar *session_handle = NULL;
            g_variant_get(parameters, "(&o&o)", &handle, &session_handle);

            SessionEntry *sess = find_session(session_handle);
            GVariantBuilder bound_builder;
            g_variant_builder_init(&bound_builder, G_VARIANT_TYPE("a(sa{sv})"));

            if (sess) {
                for (GList *l = sess->shortcuts; l != NULL; l = l->next) {
                    ShortcutEntry *sc = (ShortcutEntry *)l->data;
                    GVariantBuilder sc_opts;
                    g_variant_builder_init(&sc_opts, G_VARIANT_TYPE("a{sv}"));
                    g_variant_builder_add(&sc_opts, "{sv}", "description", g_variant_new_string(sc->description ? sc->description : ""));
                    g_variant_builder_add(&sc_opts, "{sv}", "trigger_description", g_variant_new_string(sc->trigger ? sc->trigger : ""));
                    g_variant_builder_add(&bound_builder, "(sa{sv})", sc->id, &sc_opts);
                }
            }

            GVariantBuilder results;
            g_variant_builder_init(&results, G_VARIANT_TYPE("a{sv}"));
            g_variant_builder_add(&results, "{sv}", "shortcuts", g_variant_builder_end(&bound_builder));

            g_dbus_method_invocation_return_value(invocation, g_variant_new("(ua{sv})", 0, &results));
            return;
        }

        if (g_strcmp0(method_name, "ConfigureShortcuts") == 0) {
            g_dbus_method_invocation_return_value(invocation, NULL);
            return;
        }
    }

    if (g_strcmp0(interface_name, "org.freedesktop.impl.portal.Session") == 0) {
        if (g_strcmp0(method_name, "Close") == 0) {
            SessionEntry *sess = find_session(object_path);
            if (sess) {
                sessions = g_list_remove(sessions, sess);
                free_session(sess);
                sync_bound_keys_file();
            }
            g_dbus_method_invocation_return_value(invocation, NULL);
            return;
        }
    }

    g_dbus_method_invocation_return_error(invocation,
                                          G_DBUS_ERROR,
                                          G_DBUS_ERROR_UNKNOWN_METHOD,
                                          "Method %s not found",
                                          method_name);
}

static GVariant *handle_get_property(GDBusConnection *connection,
                                     const gchar *sender,
                                     const gchar *object_path,
                                     const gchar *interface_name,
                                     const gchar *property_name,
                                     GError **error,
                                     gpointer user_data) {
    if (g_strcmp0(property_name, "version") == 0) {
        return g_variant_new_uint32(2);
    }
    return NULL;
}

static const GDBusInterfaceVTable vtable = {
    handle_method_call,
    handle_get_property,
    NULL,
};

static void on_bus_acquired(GDBusConnection *connection,
                            const gchar *name,
                            gpointer user_data) {
    dbus_conn = connection;
    GError *error = NULL;
    global_node_info = g_dbus_node_info_new_for_xml(introspection_xml, &error);
    if (!global_node_info) {
        fprintf(stderr, "Failed to parse introspection xml: %s\n", error->message);
        g_error_free(error);
        return;
    }

    GDBusInterfaceInfo *iface = g_dbus_node_info_lookup_interface(global_node_info, INTERFACE_NAME);
    g_dbus_connection_register_object(connection,
                                      OBJECT_PATH,
                                      iface,
                                      &vtable,
                                      NULL,
                                      NULL,
                                      &error);
    if (error) {
        fprintf(stderr, "Failed to register object: %s\n", error->message);
        g_error_free(error);
    } else {
        printf("[infynitae-portal] Registered %s on %s\n", INTERFACE_NAME, OBJECT_PATH);
        fflush(stdout);
    }
}

// -----------------------------------------------------------------------------
// UNIX Socket IPC Listener: /run/user/<uid>/infynitae/shortcuts.sock
// -----------------------------------------------------------------------------

static gboolean on_socket_incoming(GSocketService *service,
                                    GSocketConnection *connection,
                                    GObject *source_object,
                                    gpointer user_data) {
    GInputStream *istream = g_io_stream_get_input_stream(G_IO_STREAM(connection));
    char buf[128] = {0};
    gssize bytes = g_input_stream_read(istream, buf, sizeof(buf) - 1, NULL, NULL);
    if (bytes > 0) {
        buf[bytes] = '\0';
        dispatch_action(buf);
    }
    return FALSE;
}

static void setup_socket_listener(void) {
    gchar *sock_dir = g_build_filename(g_get_user_runtime_dir(), "infynitae", NULL);
    gchar *sock_path = g_build_filename(sock_dir, "shortcuts.sock", NULL);

    mkdir(sock_dir, 0700);
    unlink(sock_path);

    GSocketService *service = g_socket_service_new();
    GSocketAddress *addr = g_unix_socket_address_new(sock_path);
    GError *error = NULL;

    if (!g_socket_listener_add_address(G_SOCKET_LISTENER(service),
                                       addr,
                                       G_SOCKET_TYPE_STREAM,
                                       G_SOCKET_PROTOCOL_DEFAULT,
                                       NULL,
                                       NULL,
                                       &error)) {
        fprintf(stderr, "Failed to bind shortcuts socket %s: %s\n", sock_path, error->message);
        g_error_free(error);
    } else {
        chmod(sock_path, 0700);
        g_signal_connect(service, "incoming", G_CALLBACK(on_socket_incoming), NULL);
        g_socket_service_start(service);
        printf("[infynitae-portal] Shortcuts socket listening on %s\n", sock_path);
        fflush(stdout);
    }
    g_object_unref(addr);
    g_free(sock_path);
    g_free(sock_dir);
}

int main(int argc, char *argv[]) {
    setup_socket_listener();

    guint owner_id = g_bus_own_name(G_BUS_TYPE_SESSION,
                                    BUS_NAME,
                                    G_BUS_NAME_OWNER_FLAGS_REPLACE,
                                    on_bus_acquired,
                                    NULL,
                                    NULL,
                                    NULL,
                                    NULL);

    GMainLoop *loop = g_main_loop_new(NULL, FALSE);
    g_main_loop_run(loop);

    g_bus_unown_name(owner_id);
    g_main_loop_unref(loop);
    return 0;
}
