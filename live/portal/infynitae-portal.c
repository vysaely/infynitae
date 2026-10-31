/*
 * Infynitae GlobalShortcuts XDG Desktop Portal Implementation
 * C and GIO; idle, it only waits for D-Bus calls.
 * Implements org.freedesktop.impl.portal.GlobalShortcuts for Discord, OBS, WebRTC,
 * org.freedesktop.ScreenSaver (apps keeping the screens on while they play video), and
 * RemoteDesktop with ScreenCast (remote-desktop.c).
 */

#include <gio/gio.h>
#include <gio/gunixsocketaddress.h>
#include <gio/gunixfdlist.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>

#include "remote-desktop.h"
#include <xkbcommon/xkbcommon.h>

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
    char *trigger;          // the key it is bound to: GLOBAL_SHORTCUTS's, else the app's
    char *preferred;        // the key the app asked for
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
    g_free(sc->preferred);
    g_free(sc);
}

static void free_session(SessionEntry *s) {
    if (!s) return;
    g_free(s->session_handle);
    g_free(s->app_id);
    g_list_free_full(s->shortcuts, (GDestroyNotify)free_shortcut);
    g_free(s);
}

// The key a trigger's last part names ("CTRL+SHIFT+m", "KP_Add", "XF86AudioMute", "F20"…): any key
// of the user's layout (the session's XKB_DEFAULT_*), first in its plain level, then shifted. 0
// when nothing produces it.
static uint32_t keysym_name_to_evdev(const char *name) {
    static struct xkb_keymap *keymap;
    static gboolean tried;
    if (!tried) {
        tried = TRUE;
        struct xkb_context *ctx = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
        if (ctx) {
            keymap = xkb_keymap_new_from_names(ctx, NULL, XKB_KEYMAP_COMPILE_NO_FLAGS);
            if (!keymap) {
                struct xkb_rule_names us = { .layout = "us" };
                keymap = xkb_keymap_new_from_names(ctx, &us, XKB_KEYMAP_COMPILE_NO_FLAGS);
            }
            xkb_context_unref(ctx);
        }
    }
    if (!keymap || !name || !*name) return 0;
    xkb_keysym_t sym = xkb_keysym_from_name(name, XKB_KEYSYM_NO_FLAGS);
    if (sym == XKB_KEY_NoSymbol) sym = xkb_keysym_from_name(name, XKB_KEYSYM_CASE_INSENSITIVE);
    if (sym == XKB_KEY_NoSymbol) return 0;
    xkb_keysym_t lower = xkb_keysym_to_lower(sym);
    xkb_keycode_t min = xkb_keymap_min_keycode(keymap), max = xkb_keymap_max_keycode(keymap);
    for (xkb_level_index_t level = 0; level < 2; level++) {
        for (xkb_keycode_t kc = min; kc <= max && kc >= 8; kc++) {
            xkb_layout_index_t layouts = xkb_keymap_num_layouts_for_key(keymap, kc);
            for (xkb_layout_index_t l = 0; l < layouts; l++) {
                const xkb_keysym_t *syms;
                int n = xkb_keymap_key_get_syms_by_level(keymap, kc, l, level, &syms);
                for (int i = 0; i < n; i++) {
                    if (syms[i] == sym || syms[i] == lower) return kc - 8;
                }
            }
        }
    }
    return 0;
}

static uint32_t trigger_to_evdev(const char *trigger) {
    if (!trigger || !*trigger) return 0;
    const char *last = strrchr(trigger, '+');
    last = last ? last + 1 : trigger;
    while (*last == ' ') last++;
    // Discord sends a lone modifier as F13-F16 (older builds F21-F24): the modifier itself
    static const struct { const char *name; uint32_t evdev; } lone[] = {
        {"F13", 42}, {"F14", 29}, {"F15", 54}, {"F16", 97},
        {"F21", 42}, {"F22", 29}, {"F23", 54}, {"F24", 97},
    };
    for (size_t i = 0; i < G_N_ELEMENTS(lone); i++) {
        if (strcmp(last, lone[i].name) == 0) return lone[i].evdev;
    }
    uint32_t key = keysym_name_to_evdev(last);
    if (key) return key;
    // F17-F20: keyboards and mice that send them, which the layout names as media keys
    int fn = 0;
    if (sscanf(last, "F%d", &fn) == 1 && fn >= 17 && fn <= 20) return 183 + (fn - 13);
    // without a keymap: the names below
    if (strstr(trigger, "Shift_R") || strstr(trigger, "ShiftRight")) return 54;
    if (strstr(trigger, "Control_R") || strstr(trigger, "ControlRight")) return 97;
    if (strstr(trigger, "Shift_L") || strstr(trigger, "ShiftLeft")) return 42;
    if (strstr(trigger, "Control_L") || strstr(trigger, "ControlLeft")) return 29;
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

// The modifier a key is itself: held while it is pressed, so a shortcut on a lone modifier (Discord's
// Right Shift, sent as F15) must not count it.
static uint32_t own_modifier(uint32_t evdev_key) {
    switch (evdev_key) {
        case 42: case 54: return PORTAL_MOD_SHIFT;
        case 29: case 97: return PORTAL_MOD_CTRL;
        case 56: case 100: return PORTAL_MOD_ALT;
        case 125: case 126: return PORTAL_MOD_LOGO;
        default: return 0;
    }
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

// GLOBAL_SHORTCUTS in ~/.config/infynitae/config: the key each shortcut is bound to, whatever
// the app asked for, as "id=trigger" (or "app/id=trigger") pairs split by spaces or commas:
//   GLOBAL_SHORTCUTS="discord-toggle-mute=Shift_R discord-toggle-deafen=Control_R"
// A trigger is a key name ("Shift_R", "F9", "XF86AudioMute"), with modifiers before it
// ("CTRL+SHIFT+m"). NULL when the config has none.
static gchar **read_global_shortcuts(void) {
    gchar *path = g_build_filename(g_get_home_dir(), ".config", "infynitae", "config", NULL);
    gchar *text = NULL;
    gchar *value = NULL;
    if (g_file_get_contents(path, &text, NULL, NULL)) {
        gchar **lines = g_strsplit(text, "\n", -1);
        for (gchar **l = lines; *l; l++) {
            const gchar *p = *l;
            while (*p == ' ' || *p == '\t') p++;
            if (g_str_has_prefix(p, "export ")) p += 7;
            if (!g_str_has_prefix(p, "GLOBAL_SHORTCUTS=")) continue;
            p += strlen("GLOBAL_SHORTCUTS=");
            // the quoted value (the last such line wins, as the shell does)
            gchar quote = (*p == '"' || *p == '\'') ? *p++ : 0;
            const gchar *end = quote ? strchr(p, quote) : p + strcspn(p, " \t#");
            if (!end) end = p + strlen(p);
            g_free(value);
            value = g_strndup(p, end - p);
        }
        g_strfreev(lines);
        g_free(text);
    }
    g_free(path);
    if (!value || !*value) {
        g_free(value);
        return NULL;
    }
    gchar **pairs = g_strsplit_set(value, " \t,", -1);
    g_free(value);
    return pairs;
}

// The trigger GLOBAL_SHORTCUTS gives shortcut `id` of `app_id`, or NULL.
static const gchar *shortcut_override(gchar **pairs, const char *app_id, const char *id) {
    for (gchar **p = pairs; p && *p; p++) {
        const gchar *eq = strchr(*p, '=');
        if (!eq || eq == *p || !eq[1]) continue;
        gsize klen = eq - *p;
        const gchar *slash = memchr(*p, '/', klen);
        if (slash) {
            // "app/id"
            gsize alen = slash - *p;
            if (!app_id || strlen(app_id) != alen || g_ascii_strncasecmp(*p, app_id, alen) != 0)
                continue;
            if (strlen(id) == klen - alen - 1 && g_ascii_strncasecmp(slash + 1, id, klen - alen - 1) == 0)
                return eq + 1;
        } else if (strlen(id) == klen && g_ascii_strncasecmp(*p, id, klen) == 0) {
            return eq + 1;
        }
    }
    return NULL;
}

// Binds `sc` to its key: GLOBAL_SHORTCUTS's for it, else the one the app asked for. TRUE when
// the key changed.
static gboolean bind_trigger(ShortcutEntry *sc, const char *app_id, gchar **pairs) {
    const gchar *chosen = shortcut_override(pairs, app_id, sc->id);
    if (!chosen) chosen = sc->preferred ? sc->preferred : "";
    if (sc->trigger && strcmp(sc->trigger, chosen) == 0) return FALSE;
    g_free(sc->trigger);
    sc->trigger = g_strdup(chosen);
    sc->evdev_key = trigger_to_evdev(chosen);
    sc->modifiers = trigger_to_modifiers(chosen);
    if (*chosen && !sc->evdev_key)
        g_printerr("[infynitae-portal] Shortcut '%s': no key is named '%s'\n", sc->id, chosen);
    return TRUE;
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
            key_mods &= ~own_modifier(key_code);
            for (GList *sl = sessions; sl != NULL; sl = sl->next) {
                SessionEntry *sess = (SessionEntry *)sl->data;
                for (GList *k = sess->shortcuts; k != NULL; k = k->next) {
                    ShortcutEntry *sc = (ShortcutEntry *)k->data;
                    if (sc->evdev_key == key_code) {
                        if (pressed) {
                            // the shortcut's modifiers held, others too (Shift held to run while
                            // talking), as on Windows
                            if (parsed >= 3 && (key_mods & sc->modifiers) != sc->modifiers) {
                                continue;
                            }
                            sc->active = TRUE;
                            emit_portal_signal(sess->session_handle, sc->id, "Activated");
                        } else {
                            // released however the modifiers changed meanwhile
                            if (sc->active) {
                                sc->active = FALSE;
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

            gchar **overrides = read_global_shortcuts();
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
                sc->preferred = g_strdup(pref_trigger);
                bind_trigger(sc, sess->app_id, overrides);
                sc->active = FALSE;
                sess->shortcuts = g_list_append(sess->shortcuts, sc);

                // Build returned shortcut with trigger_description
                GVariantBuilder ret_sc_opts;
                g_variant_builder_init(&ret_sc_opts, G_VARIANT_TYPE("a{sv}"));
                g_variant_builder_add(&ret_sc_opts, "{sv}", "description", g_variant_new_string(desc));
                g_variant_builder_add(&ret_sc_opts, "{sv}", "trigger_description", g_variant_new_string(sc->trigger));

                g_variant_builder_add(&bound_shortcuts_builder, "(sa{sv})", sc_id, &ret_sc_opts);

                printf("[infynitae-portal] Bound shortcut '%s': desc='%s', trigger='%s' (asked '%s', evdev=%u)\n", sc_id, desc, sc->trigger, pref_trigger, sc->evdev_key);
                g_variant_unref(sc_opts);
            }

            g_strfreev(overrides);
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
    remote_desktop_init(connection, OBJECT_PATH);
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

/* ========================================================================== */
/* org.freedesktop.ScreenSaver: apps that keep the screens on over D-Bus      */
/* ========================================================================== */
/*
 * X11 apps through Xwayland, VLC, Steam and some Electron apps call Inhibit while they play
 * video, and UnInhibit after. Each Inhibit takes a logind idle inhibitor ("idle", "block") and
 * holds its fd: the compositor asks logind at the idle timeout and keeps the screens on while
 * one is held. An inhibitor ends with UnInhibit, or when the app that asked leaves the bus
 * (quits or crashes), so none is left behind. Wayland apps use the compositor's own
 * idle-inhibit protocol instead.
 */
#define SS_BUS_NAME "org.freedesktop.ScreenSaver"

typedef struct {
    guint32 cookie;
    char *sender;
    int fd;
    guint watch_id;
} SsInhibitor;

static GList *ss_inhibitors = NULL; /* SsInhibitor* */
static guint32 ss_next_cookie = 1;
static GDBusNodeInfo *ss_node_info = NULL;

static const gchar ss_introspection_xml[] =
  "<node>"
  "  <interface name='org.freedesktop.ScreenSaver'>"
  "    <method name='Inhibit'>"
  "      <arg type='s' name='application_name' direction='in'/>"
  "      <arg type='s' name='reason_for_inhibit' direction='in'/>"
  "      <arg type='u' name='cookie' direction='out'/>"
  "    </method>"
  "    <method name='UnInhibit'>"
  "      <arg type='u' name='cookie' direction='in'/>"
  "    </method>"
  "    <method name='GetActive'>"
  "      <arg type='b' name='active' direction='out'/>"
  "    </method>"
  "  </interface>"
  "</node>";

static void ss_release(SsInhibitor *in) {
    ss_inhibitors = g_list_remove(ss_inhibitors, in);
    if (in->watch_id) g_bus_unwatch_name(in->watch_id);
    if (in->fd >= 0) close(in->fd);
    g_free(in->sender);
    g_free(in);
}

/* the app that asked left the bus: its inhibitors end with it */
static void ss_sender_vanished(GDBusConnection *connection, const gchar *name, gpointer user_data) {
    (void)connection;
    (void)user_data;
    GList *l = ss_inhibitors;
    while (l) {
        GList *next = l->next;
        SsInhibitor *in = l->data;
        if (g_strcmp0(in->sender, name) == 0) ss_release(in);
        l = next;
    }
}

/* a logind idle inhibitor: its fd, -1 when logind refused */
static int ss_logind_inhibit(const char *who, const char *why) {
    GError *err = NULL;
    GDBusConnection *system = g_bus_get_sync(G_BUS_TYPE_SYSTEM, NULL, &err);
    if (!system) {
        g_printerr("[infynitae-portal] ScreenSaver: no system bus: %s\n", err ? err->message : "?");
        g_clear_error(&err);
        return -1;
    }
    GUnixFDList *fds = NULL;
    GVariant *reply = g_dbus_connection_call_with_unix_fd_list_sync(
        system, "org.freedesktop.login1", "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager", "Inhibit",
        g_variant_new("(ssss)", "idle", who, why, "block"), G_VARIANT_TYPE("(h)"),
        G_DBUS_CALL_FLAGS_NONE, 5000, NULL, &fds, NULL, &err);
    g_object_unref(system);
    if (!reply) {
        g_printerr("[infynitae-portal] ScreenSaver: logind refused: %s\n", err ? err->message : "?");
        g_clear_error(&err);
        return -1;
    }
    gint32 index = -1;
    g_variant_get(reply, "(h)", &index);
    g_variant_unref(reply);
    int fd = (fds && index >= 0) ? g_unix_fd_list_get(fds, index, NULL) : -1;
    if (fds) g_object_unref(fds);
    return fd;
}

static void ss_method_call(GDBusConnection *connection, const gchar *sender,
                           const gchar *object_path, const gchar *interface_name,
                           const gchar *method_name, GVariant *parameters,
                           GDBusMethodInvocation *invocation, gpointer user_data) {
    (void)object_path;
    (void)interface_name;
    (void)user_data;
    if (g_strcmp0(method_name, "Inhibit") == 0) {
        const char *app = NULL, *reason = NULL;
        g_variant_get(parameters, "(&s&s)", &app, &reason);
        char *who = g_strdup_printf("%s (ScreenSaver)", (app && *app) ? app : sender);
        int fd = ss_logind_inhibit(who, (reason && *reason) ? reason : "Playing media");
        g_free(who);
        if (fd < 0) {
            g_dbus_method_invocation_return_error(invocation, G_DBUS_ERROR, G_DBUS_ERROR_FAILED,
                                                  "logind did not take the idle inhibitor");
            return;
        }
        SsInhibitor *in = g_new0(SsInhibitor, 1);
        in->cookie = ss_next_cookie++;
        if (ss_next_cookie == 0) ss_next_cookie = 1;
        in->sender = g_strdup(sender);
        in->fd = fd;
        in->watch_id = g_bus_watch_name_on_connection(connection, sender,
                                                       G_BUS_NAME_WATCHER_FLAGS_NONE, NULL,
                                                       ss_sender_vanished, NULL, NULL);
        ss_inhibitors = g_list_prepend(ss_inhibitors, in);
        g_print("[infynitae-portal] ScreenSaver: %s keeps the screens on (cookie %u)\n", app, in->cookie);
        g_dbus_method_invocation_return_value(invocation, g_variant_new("(u)", in->cookie));
    } else if (g_strcmp0(method_name, "UnInhibit") == 0) {
        guint32 cookie = 0;
        g_variant_get(parameters, "(u)", &cookie);
        for (GList *l = ss_inhibitors; l; l = l->next) {
            SsInhibitor *in = l->data;
            /* only the app that took it ends it */
            if (in->cookie == cookie && g_strcmp0(in->sender, sender) == 0) {
                ss_release(in);
                break;
            }
        }
        g_dbus_method_invocation_return_value(invocation, NULL);
    } else if (g_strcmp0(method_name, "GetActive") == 0) {
        g_dbus_method_invocation_return_value(invocation, g_variant_new("(b)", FALSE));
    } else {
        g_dbus_method_invocation_return_error(invocation, G_DBUS_ERROR, G_DBUS_ERROR_UNKNOWN_METHOD,
                                              "unknown method %s", method_name);
    }
}

static const GDBusInterfaceVTable ss_vtable = { ss_method_call, NULL, NULL, { 0 } };

static void ss_on_bus_acquired(GDBusConnection *connection, const gchar *name, gpointer user_data) {
    (void)name;
    (void)user_data;
    ss_node_info = g_dbus_node_info_new_for_xml(ss_introspection_xml, NULL);
    /* apps use either path */
    const char *paths[] = { "/org/freedesktop/ScreenSaver", "/ScreenSaver" };
    for (size_t i = 0; i < 2; i++) {
        GError *err = NULL;
        if (!g_dbus_connection_register_object(connection, paths[i], ss_node_info->interfaces[0],
                                               &ss_vtable, NULL, NULL, &err)) {
            g_printerr("[infynitae-portal] ScreenSaver: %s not registered: %s\n", paths[i],
                       err ? err->message : "?");
            g_clear_error(&err);
        }
    }
}

// The config changed: every bound shortcut takes its key from GLOBAL_SHORTCUTS again, and its
// app hears of a new one (ShortcutsChanged), as when the user rebinds it in a settings panel.
static void on_config_changed(GFileMonitor *monitor, GFile *file, GFile *other,
                              GFileMonitorEvent event, gpointer user_data) {
    (void)monitor; (void)file; (void)other; (void)user_data;
    if (event != G_FILE_MONITOR_EVENT_CHANGES_DONE_HINT && event != G_FILE_MONITOR_EVENT_CREATED)
        return;
    gchar **overrides = read_global_shortcuts();
    gboolean any = FALSE;
    for (GList *sl = sessions; sl; sl = sl->next) {
        SessionEntry *sess = (SessionEntry *)sl->data;
        gboolean changed = FALSE;
        for (GList *k = sess->shortcuts; k; k = k->next) {
            ShortcutEntry *sc = (ShortcutEntry *)k->data;
            if (bind_trigger(sc, sess->app_id, overrides)) {
                sc->active = FALSE;
                changed = TRUE;
                printf("[infynitae-portal] Rebound shortcut '%s' to '%s'\n", sc->id, sc->trigger);
            }
        }
        if (!changed || !dbus_conn) continue;
        any = TRUE;
        GVariantBuilder list;
        g_variant_builder_init(&list, G_VARIANT_TYPE("a(sa{sv})"));
        for (GList *k = sess->shortcuts; k; k = k->next) {
            ShortcutEntry *sc = (ShortcutEntry *)k->data;
            GVariantBuilder opts;
            g_variant_builder_init(&opts, G_VARIANT_TYPE("a{sv}"));
            g_variant_builder_add(&opts, "{sv}", "description", g_variant_new_string(sc->description ? sc->description : ""));
            g_variant_builder_add(&opts, "{sv}", "trigger_description", g_variant_new_string(sc->trigger ? sc->trigger : ""));
            g_variant_builder_add(&list, "(sa{sv})", sc->id, &opts);
        }
        g_dbus_connection_emit_signal(dbus_conn, NULL, OBJECT_PATH, INTERFACE_NAME, "ShortcutsChanged",
                                      g_variant_new("(oa(sa{sv}))", sess->session_handle, &list), NULL);
    }
    g_strfreev(overrides);
    if (any) sync_bound_keys_file();
    fflush(stdout);
}

static void watch_config(void) {
    gchar *path = g_build_filename(g_get_home_dir(), ".config", "infynitae", "config", NULL);
    GFile *file = g_file_new_for_path(path);
    GFileMonitor *monitor = g_file_monitor_file(file, G_FILE_MONITOR_NONE, NULL, NULL);
    if (monitor) g_signal_connect(monitor, "changed", G_CALLBACK(on_config_changed), NULL);
    /* the monitor lives as long as the portal */
    g_object_unref(file);
    g_free(path);
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

    /* not when another desktop's screensaver service already answers in this session */
    guint ss_owner_id = g_bus_own_name(G_BUS_TYPE_SESSION, SS_BUS_NAME, G_BUS_NAME_OWNER_FLAGS_NONE,
                                       ss_on_bus_acquired, NULL, NULL, NULL, NULL);

    watch_config();

    GMainLoop *loop = g_main_loop_new(NULL, FALSE);
    g_main_loop_run(loop);

    g_bus_unown_name(ss_owner_id);
    g_bus_unown_name(owner_id);
    g_main_loop_unref(loop);
    return 0;
}
