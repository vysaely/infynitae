/*
 * org.freedesktop.impl.portal.FileChooser for Infynitae's portal (docs/FILE_PICKER.md).
 *
 * Answers OpenFile, SaveFile, and SaveFiles for xdg-desktop-portal by spawning
 * ~/.local/bin/infynitae-files --portal with one JSON request on stdin and reading
 * one JSON reply from stdout. Everything runs asynchronously on GLib's main loop so
 * global shortcuts, settings, and screen sharing never block while a dialog is open.
 *
 * If infynitae-files cannot be started, exits abnormally, or returns invalid JSON,
 * the exact call is forwarded to org.freedesktop.impl.portal.desktop.gtk so the user
 * can still open or save their work.
 */

#include "file-chooser.h"
#include "remote-desktop.h"

#include <ctype.h>
#include <gio/gio.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define FC_IFACE "org.freedesktop.impl.portal.FileChooser"
#define REQ_IFACE "org.freedesktop.impl.portal.Request"
#define GTK_PORTAL_NAME "org.freedesktop.impl.portal.desktop.gtk"
#define PORTAL_OBJ_PATH "/org/freedesktop/portal/desktop"

static const gchar fc_xml[] =
  "<node>"
  "  <interface name='org.freedesktop.impl.portal.FileChooser'>"
  "    <property name='version' type='u' access='read'/>"
  "    <method name='OpenFile'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='s' name='parent_window' direction='in'/>"
  "      <arg type='s' name='title' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='SaveFile'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='s' name='parent_window' direction='in'/>"
  "      <arg type='s' name='title' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "    <method name='SaveFiles'>"
  "      <arg type='o' name='handle' direction='in'/>"
  "      <arg type='s' name='app_id' direction='in'/>"
  "      <arg type='s' name='parent_window' direction='in'/>"
  "      <arg type='s' name='title' direction='in'/>"
  "      <arg type='a{sv}' name='options' direction='in'/>"
  "      <arg type='u' name='response' direction='out'/>"
  "      <arg type='a{sv}' name='results' direction='out'/>"
  "    </method>"
  "  </interface>"
  "</node>";

static const gchar req_xml[] =
  "<node>"
  "  <interface name='org.freedesktop.impl.portal.Request'>"
  "    <method name='Close'/>"
  "  </interface>"
  "</node>";

typedef struct {
    int refcount;
    GDBusConnection *conn;
    GDBusMethodInvocation *inv;
    char *handle;
    char *method;
    GVariant *orig_params;
    guint req_reg_id;
    GSubprocess *proc;
    GCancellable *cancellable;
    gboolean closed;
    gboolean in_fallback;
} FcRequest;

static GDBusNodeInfo *fc_node_info = NULL;
static GDBusNodeInfo *req_node_info = NULL;
static GHashTable *fc_requests = NULL; /* char *handle -> FcRequest * (borrowed) */

static FcRequest *fc_req_ref(FcRequest *r) {
    r->refcount++;
    return r;
}

static void fc_req_unref(FcRequest *r) {
    if (!r || --r->refcount > 0) return;
    if (r->req_reg_id && r->conn)
        g_dbus_connection_unregister_object(r->conn, r->req_reg_id);
    if (r->proc) g_object_unref(r->proc);
    if (r->cancellable) g_object_unref(r->cancellable);
    if (r->orig_params) g_variant_unref(r->orig_params);
    if (r->conn) g_object_unref(r->conn);
    g_free(r->handle);
    g_free(r->method);
    g_free(r);
}

static void fc_req_unregister(FcRequest *r) {
    if (fc_requests && r->handle && g_hash_table_lookup(fc_requests, r->handle) == r)
        g_hash_table_remove(fc_requests, r->handle);
    if (r->req_reg_id && r->conn) {
        g_dbus_connection_unregister_object(r->conn, r->req_reg_id);
        r->req_reg_id = 0;
    }
}

static void fc_req_complete(FcRequest *r, guint32 response, GVariantBuilder *results) {
    if (!r->inv) return;
    fc_req_unregister(r);
    GDBusMethodInvocation *inv = r->inv;
    r->inv = NULL;
    if (results) {
        g_dbus_method_invocation_return_value(inv, g_variant_new("(ua{sv})", response, results));
    } else {
        GVariantBuilder empty;
        g_variant_builder_init(&empty, G_VARIANT_TYPE_VARDICT);
        g_dbus_method_invocation_return_value(inv, g_variant_new("(ua{sv})", response, &empty));
    }
}

static void fc_req_complete_variant(FcRequest *r, GVariant *reply) {
    if (!r->inv) return;
    fc_req_unregister(r);
    GDBusMethodInvocation *inv = r->inv;
    r->inv = NULL;
    g_dbus_method_invocation_return_value(inv, reply);
}

/* ---- JSON writer for the picker stdin request ---- */

static void json_append_str(GString *out, const char *s) {
    g_string_append_c(out, '"');
    if (s) {
        for (const unsigned char *p = (const unsigned char *)s; *p; p++) {
            switch (*p) {
                case '"': g_string_append(out, "\\\""); break;
                case '\\': g_string_append(out, "\\\\"); break;
                case '\b': g_string_append(out, "\\b"); break;
                case '\f': g_string_append(out, "\\f"); break;
                case '\n': g_string_append(out, "\\n"); break;
                case '\r': g_string_append(out, "\\r"); break;
                case '\t': g_string_append(out, "\\t"); break;
                default:
                    if (*p < 0x20)
                        g_string_append_printf(out, "\\u%04x", *p);
                    else
                        g_string_append_c(out, (char)*p);
                    break;
            }
        }
    }
    g_string_append_c(out, '"');
}

static void json_append_b64_variant(GString *out, GVariant *ay) {
    gsize n = 0;
    const guchar *bytes = g_variant_get_fixed_array(ay, &n, 1);
    char *enc = g_base64_encode(bytes, n);
    json_append_str(out, enc);
    g_free(enc);
}

static void json_append_filter(GString *out, GVariant *filter) {
    const char *label = NULL;
    GVariantIter *pats = NULL;
    g_variant_get(filter, "(&sa(us))", &label, &pats);
    g_string_append(out, "{\"label\":");
    json_append_str(out, label);
    g_string_append(out, ",\"patterns\":[");
    guint32 kind = 0;
    const char *pat = NULL;
    gboolean first = TRUE;
    while (g_variant_iter_loop(pats, "(u&s)", &kind, &pat)) {
        if (!first) g_string_append_c(out, ',');
        first = FALSE;
        g_string_append_printf(out, "{\"kind\":%u,\"pattern\":", kind);
        json_append_str(out, pat);
        g_string_append_c(out, '}');
    }
    g_variant_iter_free(pats);
    g_string_append(out, "]}");
}

static void json_append_choices(GString *out, GVariant *choices) {
    g_string_append_c(out, '[');
    GVariantIter iter;
    g_variant_iter_init(&iter, choices);
    const char *id = NULL, *label = NULL, *initial = NULL;
    GVariantIter *opts = NULL;
    gboolean first = TRUE;
    while (g_variant_iter_loop(&iter, "(&s&sa(ss)&s)", &id, &label, &opts, &initial)) {
        if (!first) g_string_append_c(out, ',');
        first = FALSE;
        g_string_append(out, "{\"id\":");
        json_append_str(out, id);
        g_string_append(out, ",\"label\":");
        json_append_str(out, label);
        g_string_append(out, ",\"options\":[");
        const char *opt_id = NULL, *opt_label = NULL;
        gboolean first_opt = TRUE;
        while (g_variant_iter_loop(opts, "(&s&s)", &opt_id, &opt_label)) {
            if (!first_opt) g_string_append_c(out, ',');
            first_opt = FALSE;
            g_string_append(out, "{\"id\":");
            json_append_str(out, opt_id);
            g_string_append(out, ",\"label\":");
            json_append_str(out, opt_label);
            g_string_append_c(out, '}');
        }
        g_string_append(out, "],\"initial\":");
        json_append_str(out, initial);
        g_string_append_c(out, '}');
    }
    g_string_append_c(out, ']');
}

static GBytes *build_request_json(const char *method, const char *app_id,
                                  const char *parent_window, const char *title,
                                  GVariant *options) {
    GString *s = g_string_sized_new(512);
    g_string_append(s, "{\"method\":");
    json_append_str(s, method);
    g_string_append(s, ",\"app_id\":");
    json_append_str(s, app_id);
    g_string_append(s, ",\"parent_window\":");
    json_append_str(s, parent_window);
    g_string_append(s, ",\"title\":");
    json_append_str(s, title);
    g_string_append(s, ",\"options\":{");

    gboolean first = TRUE;
    GVariantIter iter;
    const char *key = NULL;
    GVariant *val = NULL;
    g_variant_iter_init(&iter, options);
    while (g_variant_iter_next(&iter, "{&sv}", &key, &val)) {
        if (strcmp(key, "accept_label") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE_STRING)) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append(s, "\"accept_label\":");
            json_append_str(s, g_variant_get_string(val, NULL));
        } else if (strcmp(key, "modal") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE_BOOLEAN)) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append_printf(s, "\"modal\":%s", g_variant_get_boolean(val) ? "true" : "false");
        } else if (strcmp(key, "multiple") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE_BOOLEAN)) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append_printf(s, "\"multiple\":%s", g_variant_get_boolean(val) ? "true" : "false");
        } else if (strcmp(key, "directory") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE_BOOLEAN)) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append_printf(s, "\"directory\":%s", g_variant_get_boolean(val) ? "true" : "false");
        } else if (strcmp(key, "current_name") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE_STRING)) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append(s, "\"current_name\":");
            json_append_str(s, g_variant_get_string(val, NULL));
        } else if (strcmp(key, "current_folder") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE_BYTESTRING)) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append(s, "\"current_folder_b64\":");
            json_append_b64_variant(s, val);
        } else if (strcmp(key, "current_file") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE_BYTESTRING)) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append(s, "\"current_file_b64\":");
            json_append_b64_variant(s, val);
        } else if (strcmp(key, "files") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE("aay"))) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append(s, "\"files_b64\":[");
            GVariantIter fiter;
            g_variant_iter_init(&fiter, val);
            GVariant *item = NULL;
            gboolean ffirst = TRUE;
            while ((item = g_variant_iter_next_value(&fiter)) != NULL) {
                if (!ffirst) g_string_append_c(s, ',');
                ffirst = FALSE;
                json_append_b64_variant(s, item);
                g_variant_unref(item);
            }
            g_string_append_c(s, ']');
        } else if (strcmp(key, "filters") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE("a(sa(us))"))) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append(s, "\"filters\":[");
            GVariantIter fiter;
            g_variant_iter_init(&fiter, val);
            GVariant *flt = NULL;
            gboolean ffirst = TRUE;
            while ((flt = g_variant_iter_next_value(&fiter)) != NULL) {
                if (!ffirst) g_string_append_c(s, ',');
                ffirst = FALSE;
                json_append_filter(s, flt);
                g_variant_unref(flt);
            }
            g_string_append_c(s, ']');
        } else if (strcmp(key, "current_filter") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE("(sa(us))"))) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append(s, "\"current_filter\":");
            json_append_filter(s, val);
        } else if (strcmp(key, "choices") == 0 && g_variant_is_of_type(val, G_VARIANT_TYPE("a(ssa(ss)s)"))) {
            if (!first) g_string_append_c(s, ',');
            first = FALSE;
            g_string_append(s, "\"choices\":");
            json_append_choices(s, val);
        }
        g_variant_unref(val);
    }

    g_string_append(s, "}}\n");
    return g_string_free_to_bytes(s);
}

/* ---- Strict JSON parser for the picker stdout reply ---- */

typedef struct {
    const char *p;
    const char *end;
} JsonCur;

static void j_ws(JsonCur *c) {
    while (c->p < c->end && (*c->p == ' ' || *c->p == '\t' || *c->p == '\n' || *c->p == '\r'))
        c->p++;
}

static gboolean j_consume(JsonCur *c, char ch) {
    j_ws(c);
    if (c->p < c->end && *c->p == ch) {
        c->p++;
        return TRUE;
    }
    return FALSE;
}

static int hex_val(char ch) {
    if (ch >= '0' && ch <= '9') return ch - '0';
    if (ch >= 'a' && ch <= 'f') return ch - 'a' + 10;
    if (ch >= 'A' && ch <= 'F') return ch - 'A' + 10;
    return -1;
}

static gboolean j_hex4(JsonCur *c, guint32 *out) {
    if (c->end - c->p < 4) return FALSE;
    guint32 v = 0;
    for (int i = 0; i < 4; i++) {
        int h = hex_val(c->p[i]);
        if (h < 0) return FALSE;
        v = (v << 4) | (guint32)h;
    }
    c->p += 4;
    *out = v;
    return TRUE;
}

static gboolean j_string(JsonCur *c, char **out) {
    j_ws(c);
    if (c->p >= c->end || *c->p != '"') return FALSE;
    c->p++;
    GString *s = g_string_new(NULL);
    while (c->p < c->end) {
        unsigned char ch = (unsigned char)*c->p++;
        if (ch == '"') {
            *out = g_string_free(s, FALSE);
            return TRUE;
        }
        if (ch < 0x20) {
            g_string_free(s, TRUE);
            return FALSE;
        }
        if (ch == '\\') {
            if (c->p >= c->end) { g_string_free(s, TRUE); return FALSE; }
            char esc = *c->p++;
            switch (esc) {
                case '"': g_string_append_c(s, '"'); break;
                case '\\': g_string_append_c(s, '\\'); break;
                case '/': g_string_append_c(s, '/'); break;
                case 'b': g_string_append_c(s, '\b'); break;
                case 'f': g_string_append_c(s, '\f'); break;
                case 'n': g_string_append_c(s, '\n'); break;
                case 'r': g_string_append_c(s, '\r'); break;
                case 't': g_string_append_c(s, '\t'); break;
                case 'u': {
                    guint32 cp = 0;
                    if (!j_hex4(c, &cp)) { g_string_free(s, TRUE); return FALSE; }
                    if (cp >= 0xD800 && cp <= 0xDBFF) {
                        guint32 lo = 0;
                        if (c->end - c->p < 6 || c->p[0] != '\\' || c->p[1] != 'u') {
                            g_string_free(s, TRUE);
                            return FALSE;
                        }
                        c->p += 2;
                        if (!j_hex4(c, &lo) || lo < 0xDC00 || lo > 0xDFFF) {
                            g_string_free(s, TRUE);
                            return FALSE;
                        }
                        cp = 0x10000 + (((cp - 0xD800) << 10) | (lo - 0xDC00));
                    } else if (cp >= 0xDC00 && cp <= 0xDFFF) {
                        g_string_free(s, TRUE);
                        return FALSE;
                    }
                    g_string_append_unichar(s, (gunichar)cp);
                    break;
                }
                default:
                    g_string_free(s, TRUE);
                    return FALSE;
            }
        } else {
            g_string_append_c(s, (char)ch);
        }
    }
    g_string_free(s, TRUE);
    return FALSE;
}

static gboolean j_uint(JsonCur *c, guint32 *out) {
    j_ws(c);
    if (c->p >= c->end || !isdigit((unsigned char)*c->p)) return FALSE;
    guint64 v = 0;
    while (c->p < c->end && isdigit((unsigned char)*c->p)) {
        v = v * 10 + (guint64)(*c->p - '0');
        if (v > G_MAXUINT32) return FALSE;
        c->p++;
    }
    *out = (guint32)v;
    return TRUE;
}

static gboolean j_bool(JsonCur *c, gboolean *out) {
    j_ws(c);
    if (c->end - c->p >= 4 && memcmp(c->p, "true", 4) == 0) {
        c->p += 4;
        *out = TRUE;
        return TRUE;
    }
    if (c->end - c->p >= 5 && memcmp(c->p, "false", 5) == 0) {
        c->p += 5;
        *out = FALSE;
        return TRUE;
    }
    return FALSE;
}

static gboolean j_null(JsonCur *c) {
    j_ws(c);
    if (c->end - c->p >= 4 && memcmp(c->p, "null", 4) == 0) {
        c->p += 4;
        return TRUE;
    }
    return FALSE;
}

static gboolean j_skip_value(JsonCur *c, int depth);

static gboolean j_skip_value(JsonCur *c, int depth) {
    if (depth > 32) return FALSE;
    j_ws(c);
    if (c->p >= c->end) return FALSE;
    char ch = *c->p;
    if (ch == '"') {
        char *tmp = NULL;
        if (!j_string(c, &tmp)) return FALSE;
        g_free(tmp);
        return TRUE;
    }
    if (ch == '{') {
        c->p++;
        if (j_consume(c, '}')) return TRUE;
        for (;;) {
            char *k = NULL;
            if (!j_string(c, &k)) return FALSE;
            g_free(k);
            if (!j_consume(c, ':')) return FALSE;
            if (!j_skip_value(c, depth + 1)) return FALSE;
            if (j_consume(c, '}')) return TRUE;
            if (!j_consume(c, ',')) return FALSE;
        }
    }
    if (ch == '[') {
        c->p++;
        if (j_consume(c, ']')) return TRUE;
        for (;;) {
            if (!j_skip_value(c, depth + 1)) return FALSE;
            if (j_consume(c, ']')) return TRUE;
            if (!j_consume(c, ',')) return FALSE;
        }
    }
    gboolean b = FALSE;
    if (j_bool(c, &b) || j_null(c)) return TRUE;
    if (ch == '-' || isdigit((unsigned char)ch)) {
        if (*c->p == '-') c->p++;
        if (c->p >= c->end || !isdigit((unsigned char)*c->p)) return FALSE;
        while (c->p < c->end && isdigit((unsigned char)*c->p)) c->p++;
        if (c->p < c->end && *c->p == '.') {
            c->p++;
            if (c->p >= c->end || !isdigit((unsigned char)*c->p)) return FALSE;
            while (c->p < c->end && isdigit((unsigned char)*c->p)) c->p++;
        }
        if (c->p < c->end && (*c->p == 'e' || *c->p == 'E')) {
            c->p++;
            if (c->p < c->end && (*c->p == '+' || *c->p == '-')) c->p++;
            if (c->p >= c->end || !isdigit((unsigned char)*c->p)) return FALSE;
            while (c->p < c->end && isdigit((unsigned char)*c->p)) c->p++;
        }
        return TRUE;
    }
    return FALSE;
}

static gboolean j_parse_filter(JsonCur *c, GVariant **out_filter) {
    if (j_null(c)) {
        *out_filter = NULL;
        return TRUE;
    }
    if (!j_consume(c, '{')) return FALSE;
    char *label = NULL;
    GVariantBuilder pats;
    g_variant_builder_init(&pats, G_VARIANT_TYPE("a(us)"));
    gboolean has_label = FALSE, has_pats = FALSE;
    gboolean ok = TRUE;
    if (!j_consume(c, '}')) {
        for (;;) {
            char *k = NULL;
            if (!j_string(c, &k) || !j_consume(c, ':')) {
                g_free(k);
                ok = FALSE;
                break;
            }
            if (strcmp(k, "label") == 0) {
                g_free(label);
                label = NULL;
                if (!j_string(c, &label)) ok = FALSE;
                else has_label = TRUE;
            } else if (strcmp(k, "patterns") == 0) {
                if (!j_consume(c, '[')) {
                    ok = FALSE;
                } else {
                    has_pats = TRUE;
                    if (!j_consume(c, ']')) {
                        for (;;) {
                            if (!j_consume(c, '{')) { ok = FALSE; break; }
                            guint32 kind = 0;
                            char *pat = NULL;
                            gboolean has_kind = FALSE, has_pat = FALSE;
                            if (!j_consume(c, '}')) {
                                for (;;) {
                                    char *pk = NULL;
                                    if (!j_string(c, &pk) || !j_consume(c, ':')) {
                                        g_free(pk);
                                        ok = FALSE;
                                        break;
                                    }
                                    if (strcmp(pk, "kind") == 0) {
                                        if (!j_uint(c, &kind)) ok = FALSE;
                                        else has_kind = TRUE;
                                    } else if (strcmp(pk, "pattern") == 0) {
                                        g_free(pat);
                                        pat = NULL;
                                        if (!j_string(c, &pat)) ok = FALSE;
                                        else has_pat = TRUE;
                                    } else if (!j_skip_value(c, 0)) {
                                        ok = FALSE;
                                    }
                                    g_free(pk);
                                    if (!ok) break;
                                    if (j_consume(c, '}')) break;
                                    if (!j_consume(c, ',')) { ok = FALSE; break; }
                                }
                            }
                            if (ok && has_kind && has_pat)
                                g_variant_builder_add(&pats, "(us)", kind, pat);
                            else
                                ok = FALSE;
                            g_free(pat);
                            if (!ok) break;
                            if (j_consume(c, ']')) break;
                            if (!j_consume(c, ',')) { ok = FALSE; break; }
                        }
                    }
                }
            } else if (!j_skip_value(c, 0)) {
                ok = FALSE;
            }
            g_free(k);
            if (!ok) break;
            if (j_consume(c, '}')) break;
            if (!j_consume(c, ',')) { ok = FALSE; break; }
        }
    }
    if (ok && has_label && has_pats) {
        *out_filter = g_variant_ref_sink(g_variant_new("(sa(us))", label, &pats));
    } else {
        g_variant_builder_clear(&pats);
        ok = FALSE;
    }
    g_free(label);
    return ok;
}

typedef struct {
    guint32 response;
    GPtrArray *uris;       /* char * */
    gboolean has_writable;
    gboolean writable;
    GVariant *choices;     /* a(ss), owned */
    GVariant *current_filter; /* (sa(us)), owned */
} PickerReply;

static void picker_reply_clear(PickerReply *r) {
    if (r->uris) g_ptr_array_unref(r->uris);
    if (r->choices) g_variant_unref(r->choices);
    if (r->current_filter) g_variant_unref(r->current_filter);
    memset(r, 0, sizeof(*r));
}

static gboolean parse_picker_reply(const char *data, gsize len, PickerReply *out) {
    memset(out, 0, sizeof(*out));
    out->uris = g_ptr_array_new_with_free_func(g_free);
    JsonCur c = { data, data + len };
    gboolean has_response = FALSE;
    if (!j_consume(&c, '{')) goto fail;
    if (!j_consume(&c, '}')) {
        for (;;) {
            char *k = NULL;
            if (!j_string(&c, &k) || !j_consume(&c, ':')) {
                g_free(k);
                goto fail;
            }
            if (strcmp(k, "response") == 0) {
                g_free(k);
                if (!j_uint(&c, &out->response)) goto fail;
                has_response = TRUE;
            } else if (strcmp(k, "uris") == 0) {
                g_free(k);
                if (!j_consume(&c, '[')) goto fail;
                if (!j_consume(&c, ']')) {
                    for (;;) {
                        char *u = NULL;
                        if (!j_string(&c, &u)) goto fail;
                        g_ptr_array_add(out->uris, u);
                        if (j_consume(&c, ']')) break;
                        if (!j_consume(&c, ',')) goto fail;
                    }
                }
            } else if (strcmp(k, "writable") == 0) {
                g_free(k);
                if (!j_bool(&c, &out->writable)) goto fail;
                out->has_writable = TRUE;
            } else if (strcmp(k, "choices") == 0) {
                g_free(k);
                if (!j_null(&c)) {
                    if (!j_consume(&c, '[')) goto fail;
                    GVariantBuilder cb;
                    g_variant_builder_init(&cb, G_VARIANT_TYPE("a(ss)"));
                    gboolean any = FALSE;
                    if (!j_consume(&c, ']')) {
                        for (;;) {
                            char *cid = NULL, *cval = NULL;
                            if (!j_consume(&c, '[') || !j_string(&c, &cid) || !j_consume(&c, ',')
                                || !j_string(&c, &cval) || !j_consume(&c, ']')) {
                                g_free(cid);
                                g_free(cval);
                                g_variant_builder_clear(&cb);
                                goto fail;
                            }
                            g_variant_builder_add(&cb, "(ss)", cid, cval);
                            any = TRUE;
                            g_free(cid);
                            g_free(cval);
                            if (j_consume(&c, ']')) break;
                            if (!j_consume(&c, ',')) {
                                g_variant_builder_clear(&cb);
                                goto fail;
                            }
                        }
                    }
                    if (any) {
                        if (out->choices) g_variant_unref(out->choices);
                        out->choices = g_variant_ref_sink(g_variant_builder_end(&cb));
                    } else {
                        g_variant_builder_clear(&cb);
                    }
                }
            } else if (strcmp(k, "current_filter") == 0) {
                g_free(k);
                if (out->current_filter) {
                    g_variant_unref(out->current_filter);
                    out->current_filter = NULL;
                }
                if (!j_parse_filter(&c, &out->current_filter)) goto fail;
            } else {
                g_free(k);
                if (!j_skip_value(&c, 0)) goto fail;
            }
            if (j_consume(&c, '}')) break;
            if (!j_consume(&c, ',')) goto fail;
        }
    }
    j_ws(&c);
    if (c.p != c.end || !has_response || (out->response != 0 && out->response != 1))
        goto fail;
    return TRUE;
fail:
    picker_reply_clear(out);
    return FALSE;
}

/* ---- Runtime GTK fallback ---- */

static void fc_fallback_done(GObject *source, GAsyncResult *res, gpointer data) {
    FcRequest *req = data;
    GError *error = NULL;
    GVariant *reply = g_dbus_connection_call_finish(G_DBUS_CONNECTION(source), res, &error);
    if (!req->closed && req->inv) {
        if (reply) {
            fc_req_complete_variant(req, reply);
        } else {
            g_printerr("[infynitae-portal] FileChooser GTK fallback failed: %s\n",
                       error ? error->message : "unknown error");
            fc_req_complete(req, 2, NULL);
        }
    } else if (reply) {
        g_variant_unref(reply);
    }
    g_clear_error(&error);
    fc_req_unref(req);
}

static void fc_start_fallback(FcRequest *req, const char *reason) {
    if (req->closed || !req->inv) return;
    g_printerr("[infynitae-portal] FileChooser %s: %s; falling back to %s\n",
               req->method, reason, GTK_PORTAL_NAME);
    req->in_fallback = TRUE;
    if (req->cancellable) g_object_unref(req->cancellable);
    req->cancellable = g_cancellable_new();
    g_dbus_connection_call(req->conn, GTK_PORTAL_NAME, PORTAL_OBJ_PATH, FC_IFACE,
                           req->method, req->orig_params, G_VARIANT_TYPE("(ua{sv})"),
                           G_DBUS_CALL_FLAGS_NONE, G_MAXINT, req->cancellable,
                           fc_fallback_done, fc_req_ref(req));
}

/* ---- Async completion of infynitae-files --portal ---- */

static void fc_child_done(GObject *source, GAsyncResult *res, gpointer data) {
    FcRequest *req = data;
    GSubprocess *proc = G_SUBPROCESS(source);
    GBytes *stdout_bytes = NULL;
    GError *error = NULL;
    gboolean ok = g_subprocess_communicate_finish(proc, res, &stdout_bytes, NULL, &error);

    if (req->proc == proc) {
        g_object_unref(req->proc);
        req->proc = NULL;
    }

    if (req->closed || !req->inv) {
        if (stdout_bytes) g_bytes_unref(stdout_bytes);
        g_clear_error(&error);
        fc_req_unref(req);
        return;
    }

    if (!ok) {
        char *msg = g_strdup_printf("pipe/wait failed (%s)", error ? error->message : "unknown");
        g_clear_error(&error);
        if (stdout_bytes) g_bytes_unref(stdout_bytes);
        fc_start_fallback(req, msg);
        g_free(msg);
        fc_req_unref(req);
        return;
    }

    if (!g_subprocess_get_successful(proc)) {
        char *msg = g_subprocess_get_if_signaled(proc)
            ? g_strdup_printf("infynitae-files killed by signal %d", g_subprocess_get_term_sig(proc))
            : g_strdup_printf("infynitae-files exited with code %d", g_subprocess_get_exit_status(proc));
        if (stdout_bytes) g_bytes_unref(stdout_bytes);
        fc_start_fallback(req, msg);
        g_free(msg);
        fc_req_unref(req);
        return;
    }

    gsize out_len = 0;
    const char *out_data = stdout_bytes ? g_bytes_get_data(stdout_bytes, &out_len) : "";
    PickerReply reply;
    if (!parse_picker_reply(out_data, out_len, &reply)) {
        if (stdout_bytes) g_bytes_unref(stdout_bytes);
        fc_start_fallback(req, "infynitae-files returned invalid JSON");
        fc_req_unref(req);
        return;
    }
    if (stdout_bytes) g_bytes_unref(stdout_bytes);

    if (reply.response == 1) {
        /* user cancelled: return 1 without triggering GTK fallback */
        picker_reply_clear(&reply);
        fc_req_complete(req, 1, NULL);
        fc_req_unref(req);
        return;
    }

    GVariantBuilder results;
    g_variant_builder_init(&results, G_VARIANT_TYPE_VARDICT);
    GVariantBuilder uris_b;
    g_variant_builder_init(&uris_b, G_VARIANT_TYPE_STRING_ARRAY);
    for (guint i = 0; i < reply.uris->len; i++)
        g_variant_builder_add(&uris_b, "s", (const char *)g_ptr_array_index(reply.uris, i));
    g_variant_builder_add(&results, "{sv}", "uris", g_variant_builder_end(&uris_b));
    if (reply.has_writable)
        g_variant_builder_add(&results, "{sv}", "writable", g_variant_new_boolean(reply.writable));
    if (reply.choices)
        g_variant_builder_add(&results, "{sv}", "choices", reply.choices);
    if (reply.current_filter)
        g_variant_builder_add(&results, "{sv}", "current_filter", reply.current_filter);

    picker_reply_clear(&reply);
    fc_req_complete(req, 0, &results);
    fc_req_unref(req);
}

/* ---- org.freedesktop.impl.portal.Request on each active handle ---- */

static void req_method_call(GDBusConnection *c, const gchar *sender, const gchar *path,
                            const gchar *iface, const gchar *method, GVariant *params,
                            GDBusMethodInvocation *inv, gpointer data) {
    (void)iface;
    (void)params;
    (void)data;
    if (!from_portal(c, sender)) {
        g_dbus_method_invocation_return_dbus_error(inv, "org.freedesktop.DBus.Error.AccessDenied",
                                                   "only xdg-desktop-portal may call this");
        return;
    }
    if (g_strcmp0(method, "Close") != 0) {
        g_dbus_method_invocation_return_dbus_error(inv, "org.freedesktop.DBus.Error.UnknownMethod",
                                                   "Unknown method");
        return;
    }
    FcRequest *req = fc_requests ? g_hash_table_lookup(fc_requests, path) : NULL;
    g_dbus_method_invocation_return_value(inv, NULL);
    if (!req || req->closed) return;

    req->closed = TRUE;
    if (req->proc)
        g_subprocess_send_signal(req->proc, SIGTERM);
    if (req->cancellable)
        g_cancellable_cancel(req->cancellable);
    if (req->in_fallback) {
        g_dbus_connection_call(req->conn, GTK_PORTAL_NAME, req->handle, REQ_IFACE, "Close",
                               NULL, NULL, G_DBUS_CALL_FLAGS_NONE, 2000, NULL, NULL, NULL);
    }
    fc_req_complete(req, 2, NULL);
}

static const GDBusInterfaceVTable req_vtable = { req_method_call, NULL, NULL, { 0 } };

/* ---- org.freedesktop.impl.portal.FileChooser ---- */

static void fc_method_call(GDBusConnection *c, const gchar *sender, const gchar *path,
                           const gchar *iface, const gchar *method, GVariant *params,
                           GDBusMethodInvocation *inv, gpointer data) {
    (void)path;
    (void)iface;
    (void)data;
    if (!from_portal(c, sender)) {
        g_dbus_method_invocation_return_dbus_error(inv, "org.freedesktop.DBus.Error.AccessDenied",
                                                   "only xdg-desktop-portal may call this");
        return;
    }
    if (g_strcmp0(method, "OpenFile") != 0 && g_strcmp0(method, "SaveFile") != 0
        && g_strcmp0(method, "SaveFiles") != 0) {
        g_dbus_method_invocation_return_dbus_error(inv, "org.freedesktop.DBus.Error.UnknownMethod",
                                                   "Unknown method");
        return;
    }

    const char *handle = NULL, *app_id = NULL, *parent_window = NULL, *title = NULL;
    GVariant *options = NULL;
    g_variant_get(params, "(&o&s&s&s@a{sv})", &handle, &app_id, &parent_window, &title, &options);

    FcRequest *req = g_new0(FcRequest, 1);
    req->refcount = 1;
    req->conn = g_object_ref(c);
    req->inv = inv;
    req->handle = g_strdup(handle);
    req->method = g_strdup(method);
    req->orig_params = g_variant_ref(params);

    if (req_node_info && req_node_info->interfaces && req_node_info->interfaces[0]) {
        GError *reg_err = NULL;
        req->req_reg_id = g_dbus_connection_register_object(
            c, handle, req_node_info->interfaces[0], &req_vtable, NULL, NULL, &reg_err);
        if (!req->req_reg_id && reg_err) {
            g_printerr("[infynitae-portal] FileChooser: Request %s not registered: %s\n",
                       handle, reg_err->message);
            g_clear_error(&reg_err);
        }
    }
    if (fc_requests)
        g_hash_table_replace(fc_requests, req->handle, req);

    GBytes *stdin_bytes = build_request_json(method, app_id, parent_window, title, options);
    g_variant_unref(options);

    char *bin = g_build_filename(g_get_home_dir(), ".local", "bin", "infynitae-files", NULL);
    GError *spawn_err = NULL;
    GSubprocess *proc = g_subprocess_new(
        G_SUBPROCESS_FLAGS_STDIN_PIPE | G_SUBPROCESS_FLAGS_STDOUT_PIPE,
        &spawn_err, bin, "--portal", NULL);
    g_free(bin);

    if (!proc) {
        g_bytes_unref(stdin_bytes);
        char *msg = g_strdup_printf("could not start infynitae-files (%s)",
                                    spawn_err ? spawn_err->message : "unknown");
        g_clear_error(&spawn_err);
        fc_start_fallback(req, msg);
        g_free(msg);
        fc_req_unref(req);
        return;
    }

    req->proc = proc;
    req->cancellable = g_cancellable_new();
    g_subprocess_communicate_async(proc, stdin_bytes, req->cancellable,
                                   fc_child_done, fc_req_ref(req));
    g_bytes_unref(stdin_bytes);
    fc_req_unref(req);
}

static GVariant *fc_get_property(GDBusConnection *c, const gchar *sender, const gchar *path,
                                 const gchar *iface, const gchar *prop, GError **error,
                                 gpointer data) {
    (void)c; (void)sender; (void)path; (void)iface; (void)data;
    if (g_strcmp0(prop, "version") == 0)
        return g_variant_new_uint32(4);
    g_set_error(error, G_DBUS_ERROR, G_DBUS_ERROR_UNKNOWN_PROPERTY, "No such property");
    return NULL;
}

static const GDBusInterfaceVTable fc_vtable = { fc_method_call, fc_get_property, NULL, { 0 } };

void file_chooser_init(GDBusConnection *conn, const char *object_path) {
    GError *error = NULL;
    if (!fc_requests)
        fc_requests = g_hash_table_new(g_str_hash, g_str_equal);
    if (!req_node_info)
        req_node_info = g_dbus_node_info_new_for_xml(req_xml, NULL);
    if (!fc_node_info)
        fc_node_info = g_dbus_node_info_new_for_xml(fc_xml, &error);
    if (!fc_node_info) {
        g_printerr("[infynitae-portal] %s introspection failed: %s\n", FC_IFACE,
                   error ? error->message : "unknown");
        g_clear_error(&error);
        return;
    }
    if (!g_dbus_connection_register_object(conn, object_path, fc_node_info->interfaces[0],
                                           &fc_vtable, NULL, NULL, &error)) {
        g_printerr("[infynitae-portal] %s not registered: %s\n", FC_IFACE,
                   error ? error->message : "unknown");
        g_clear_error(&error);
        return;
    }
    printf("[infynitae-portal] Registered %s on %s\n", FC_IFACE, object_path);
    fflush(stdout);
}
