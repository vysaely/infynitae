/*
 * infynitae-polkit: the password prompt for system actions on Wayland.
 * Registers with org.freedesktop.PolicyKit1.Authority and opens a GTK3 dialog when challenged.
 */

#define POLKIT_AGENT_I_KNOW_API_IS_SUBJECT_TO_CHANGE 1

#include <polkitagent/polkitagent.h>
#include <pwd.h>
#include <grp.h>
#include <unistd.h>
#include <gtk/gtk.h>
#include <gdk/gdkwayland.h>

#define INFYNITAE_TYPE_AGENT_LISTENER (infynitae_agent_listener_get_type())
G_DECLARE_FINAL_TYPE(InfynitaeAgentListener, infynitae_agent_listener, INFYNITAE, AGENT_LISTENER, PolkitAgentListener)

struct _InfynitaeAgentListener {
    PolkitAgentListener parent_instance;
};

G_DEFINE_TYPE(InfynitaeAgentListener, infynitae_agent_listener, POLKIT_AGENT_TYPE_LISTENER)

typedef struct {
    InfynitaeAgentListener *listener;
    GTask *task;
    PolkitAgentSession *session;
    /* kept for a new session after a failed attempt (a completed session is finished) */
    PolkitIdentity *identity;
    gchar *cookie;
    GCancellable *cancellable;
    gulong cancellable_handler_id;

    GtkWidget *window;
    GtkWidget *entry;
    GtkWidget *btn_auth;
    GtkWidget *btn_cancel;
    GtkWidget *lbl_error;
    GtkWidget *lbl_prompt;
} AuthDialogData;

static void free_auth_dialog(AuthDialogData *data) {
    if (data->cancellable_handler_id > 0 && data->cancellable != NULL) {
        g_cancellable_disconnect(data->cancellable, data->cancellable_handler_id);
        data->cancellable_handler_id = 0;
    }
    if (data->window) {
        gtk_widget_destroy(data->window);
        data->window = NULL;
    }
    if (data->session) {
        g_signal_handlers_disconnect_by_data(data->session, data);
        g_object_unref(data->session);
        data->session = NULL;
    }
    g_clear_object(&data->identity);
    g_clear_pointer(&data->cookie, g_free);
    if (data->task) {
        g_object_unref(data->task);
        data->task = NULL;
    }
    g_free(data);
}

static void on_cancel_clicked(GtkWidget *widget, gpointer user_data) {
    (void)widget;
    AuthDialogData *data = (AuthDialogData *)user_data;
    if (data->session) {
        polkit_agent_session_cancel(data->session);
    }
    if (data->task && !g_task_get_completed(data->task)) {
        g_task_return_new_error(data->task, POLKIT_ERROR, POLKIT_ERROR_CANCELLED, "Authentication cancelled by user");
    }
    free_auth_dialog(data);
}

static gboolean on_window_key_press(GtkWidget *widget, GdkEventKey *event, gpointer user_data) {
    (void)widget;
    if (event->keyval == GDK_KEY_Escape) {
        on_cancel_clicked(NULL, user_data);
        return TRUE;
    }
    return FALSE;
}

static void on_submit_clicked(GtkWidget *widget, gpointer user_data) {
    (void)widget;
    AuthDialogData *data = (AuthDialogData *)user_data;
    const gchar *password = gtk_entry_get_text(GTK_ENTRY(data->entry));
    if (data->session) {
        gtk_widget_set_sensitive(data->entry, FALSE);
        gtk_widget_set_sensitive(data->btn_auth, FALSE);
        polkit_agent_session_response(data->session, password);
    }
}

static void on_session_request(PolkitAgentSession *session,
                               const gchar *request,
                               gboolean echo_on,
                               gpointer user_data) {
    (void)session;
    (void)echo_on;
    AuthDialogData *data = (AuthDialogData *)user_data;
    if (data->lbl_prompt) {
        gtk_label_set_text(GTK_LABEL(data->lbl_prompt), request ? request : "Password: ");
    }
    if (data->entry) {
        gtk_entry_set_text(GTK_ENTRY(data->entry), "");
        gtk_widget_set_sensitive(data->entry, TRUE);
        gtk_widget_set_sensitive(data->btn_auth, TRUE);
        gtk_widget_grab_focus(data->entry);
    }
}

static void on_session_show_error(PolkitAgentSession *session,
                                  const gchar *text,
                                  gpointer user_data) {
    (void)session;
    AuthDialogData *data = (AuthDialogData *)user_data;
    if (data->lbl_error) {
        gtk_label_set_text(GTK_LABEL(data->lbl_error), text ? text : "Wrong password");
        gtk_widget_show(data->lbl_error);
    }
}

static void on_session_show_info(PolkitAgentSession *session,
                                 const gchar *text,
                                 gpointer user_data) {
    (void)session;
    AuthDialogData *data = (AuthDialogData *)user_data;
    if (data->lbl_error) {
        gtk_label_set_text(GTK_LABEL(data->lbl_error), text ? text : "");
        gtk_widget_show(data->lbl_error);
    }
}

static void on_session_completed(PolkitAgentSession *session,
                                 gboolean gained_authorization,
                                 gpointer user_data);

/* The identity as a person reads it: a user's login name (or uid), a group's name. */
static gchar *identity_name(PolkitIdentity *identity) {
    if (POLKIT_IS_UNIX_USER(identity)) {
        gint uid = polkit_unix_user_get_uid(POLKIT_UNIX_USER(identity));
        struct passwd *pw = getpwuid((uid_t)uid);
        if (pw && pw->pw_name && pw->pw_name[0])
            return g_strdup(pw->pw_name);
        return g_strdup_printf("user %d", uid);
    }
    if (POLKIT_IS_UNIX_GROUP(identity)) {
        gint gid = polkit_unix_group_get_gid(POLKIT_UNIX_GROUP(identity));
        struct group *gr = getgrgid((gid_t)gid);
        return gr && gr->gr_name ? g_strdup_printf("a member of %s", gr->gr_name)
                                 : g_strdup_printf("a member of group %d", gid);
    }
    gchar *text = polkit_identity_to_string(identity);
    return text ? text : g_strdup("unknown account");
}

/* A new polkit session for the identity: one per attempt. Its "request" re-enables the entry. */
static void start_session(AuthDialogData *data) {
    if (data->session) {
        g_signal_handlers_disconnect_by_data(data->session, data);
        g_object_unref(data->session);
    }
    data->session = polkit_agent_session_new(data->identity, data->cookie);
    g_signal_connect(data->session, "request", G_CALLBACK(on_session_request), data);
    g_signal_connect(data->session, "show-error", G_CALLBACK(on_session_show_error), data);
    g_signal_connect(data->session, "show-info", G_CALLBACK(on_session_show_info), data);
    g_signal_connect(data->session, "completed", G_CALLBACK(on_session_completed), data);
    polkit_agent_session_initiate(data->session);
}

static void on_session_completed(PolkitAgentSession *session,
                                 gboolean gained_authorization,
                                 gpointer user_data) {
    (void)session;
    AuthDialogData *data = (AuthDialogData *)user_data;

    if (gained_authorization) {
        if (data->task && !g_task_get_completed(data->task)) {
            g_task_return_boolean(data->task, TRUE);
        }
        free_auth_dialog(data);
    } else {
        if (data->lbl_error) {
            gtk_label_set_text(GTK_LABEL(data->lbl_error), "Wrong password, try again");
            gtk_widget_show(data->lbl_error);
        }
        if (data->entry) {
            gtk_entry_set_text(GTK_ENTRY(data->entry), "");
        }
        /* the finished session takes no more responses: a new one asks again ("request") */
        start_session(data);
    }
}

static void on_cancellable_cancelled(GCancellable *cancellable, gpointer user_data) {
    (void)cancellable;
    AuthDialogData *data = (AuthDialogData *)user_data;
    /* g_cancellable_disconnect from inside this handler deadlocks (GLib docs); a cancellable
     * is cancelled once, so the handler is left connected */
    data->cancellable_handler_id = 0;
    on_cancel_clicked(NULL, data);
}

static int hex_nibble(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

static gboolean parse_hex_bytes(const char *s, unsigned int rgb[3]) {
    if (!s || s[0] != '#') return FALSE;
    for (int i = 1; i <= 6; i++) {
        if (hex_nibble(s[i]) < 0) return FALSE;
    }
    if (hex_nibble(s[7]) >= 0 || s[7] != '\0') return FALSE;
    for (int i = 0; i < 3; i++) {
        rgb[i] = (unsigned int)((hex_nibble(s[1 + i * 2]) << 4) | hex_nibble(s[2 + i * 2]));
    }
    return TRUE;
}

static gboolean read_cfg_val(const char *line, const char *key, char *dst, size_t dst_sz) {
    size_t klen = strlen(key);
    if (strncmp(line, key, klen) != 0 || line[klen] != '=') return FALSE;
    const char *p = line + klen + 1;
    while (*p == ' ' || *p == '\t' || *p == '"' || *p == '\'') p++;
    /* a #rrggbb color only: anything else would end up in the CSS and break it */
    char val[32];
    size_t j = 0;
    while ((g_ascii_isalnum(p[j]) || (j == 0 && p[j] == '#')) && j + 1 < sizeof(val)) {
        val[j] = p[j];
        j++;
    }
    val[j] = '\0';
    unsigned int tmp[3];
    if (val[0] != '#' || !parse_hex_bytes(val, tmp)) return FALSE;
    if (j < dst_sz) {
        memcpy(dst, val, j + 1);
        return TRUE;
    }
    return FALSE;
}

static gboolean read_hex_alpha(const char *line, const char *key, double *out) {
    size_t klen = strlen(key);
    if (strncmp(line, key, klen) != 0 || line[klen] != '=') return FALSE;
    const char *p = line + klen + 1;
    while (*p == ' ' || *p == '\t' || *p == '"' || *p == '\'') p++;
    int hi = hex_nibble(p[0]), lo = hex_nibble(p[1]);
    if (hi < 0 || lo < 0 || hex_nibble(p[2]) >= 0) return FALSE;
    *out = (double)((hi << 4) | lo) / 255.0;
    return TRUE;
}

static gboolean read_radius_val(const char *line, const char *key, int *out) {
    size_t klen = strlen(key);
    if (strncmp(line, key, klen) != 0 || line[klen] != '=') return FALSE;
    const char *p = line + klen + 1;
    while (*p == ' ' || *p == '\t' || *p == '"' || *p == '\'') p++;
    if (*p < '0' || *p > '9') return FALSE;
    char *end = NULL;
    long v = strtol(p, &end, 10);
    if (end == p) return FALSE;
    while (*end == ' ' || *end == '\t' || *end == '"' || *end == '\'' || *end == '\r' || *end == '\n') end++;
    if (*end != '\0' && *end != '#') return FALSE;
    if (v < 0) v = 0;
    if (v > 28) v = 28;
    *out = (int)v;
    return TRUE;
}

static void read_font_val(const char *line, const char *key, char *dst, size_t dst_sz) {
    size_t klen = strlen(key);
    if (strncmp(line, key, klen) != 0 || line[klen] != '=') return;
    const char *p = line + klen + 1;
    while (*p == ' ' || *p == '\t') p++;
    char quote = 0;
    if (*p == '"' || *p == '\'') quote = *p++;
    char val[128];
    size_t j = 0;
    while (*p && *p != '\n' && *p != '\r' && j + 1 < sizeof(val)) {
        if (quote && *p == quote) break;
        if (!quote && *p == '#') break;
        unsigned char ch = (unsigned char)*p;
        if (g_ascii_isalnum(ch) || ch == ' ' || ch == '-' || ch == '_' || ch == ',' || ch == '.') {
            val[j++] = *p++;
        } else {
            return;
        }
    }
    if (quote && *p != quote) return;
    while (j > 0 && (val[j - 1] == ' ' || val[j - 1] == '\t')) j--;
    val[j] = '\0';
    if (j > 0 && j < dst_sz) memcpy(dst, val, j + 1);
}

static gchar *build_css(void) {
    char surface[32] = "#111317";
    char border_act[32] = "#7aa2f7";
    char cfg_accent[32] = "";
    char cfg_border_act[32] = "";
    char accent_text[32] = "";
    char alert[32] = "";
    char border_inact[32] = "#333b54";
    double edge_alpha = 0.90;
    char text_col[32] = "#eef2f8";
    char muted[64] = "";
    char font[128] = "sans-serif";
    int radius = 8;
    int small = -1;

    const char *home = g_getenv("HOME");
    if (!home || !*home) home = g_get_home_dir();
    if (home) {
        gchar *cfg_path = g_build_filename(home, ".config", "infynitae", "config", NULL);
        FILE *f = fopen(cfg_path, "r");
        if (f) {
            char line[256];
            while (fgets(line, sizeof(line), f)) {
                char *s = line;
                while (*s == ' ' || *s == '\t') s++;
                if (*s == '#') continue;
                read_cfg_val(s, "SURFACE_COLOR", surface, sizeof(surface));
                read_cfg_val(s, "ACCENT", cfg_accent, sizeof(cfg_accent));
                read_cfg_val(s, "BORDER_ACTIVE", cfg_border_act, sizeof(cfg_border_act));
                read_cfg_val(s, "BORDER_INACTIVE", border_inact, sizeof(border_inact));
                read_cfg_val(s, "TEXT_COLOR", text_col, sizeof(text_col));
            }
            fclose(f);
            if (cfg_accent[0] != '\0') g_strlcpy(border_act, cfg_accent, sizeof(border_act));
            else if (cfg_border_act[0] != '\0') g_strlcpy(border_act, cfg_border_act, sizeof(border_act));
        }
        g_free(cfg_path);
    }

    /* the resolved look (LOOK_* in ~/.cache/infynitae/look): muted, accent-as-text, small
     * radius, edge opacity and font come from the file when present, falling back otherwise */
    {
        const char *cache = g_getenv("XDG_CACHE_HOME");
        gchar *look = (cache && *cache) ? g_build_filename(cache, "infynitae", "look", NULL)
                                        : g_build_filename(home ? home : "", ".cache", "infynitae", "look", NULL);
        FILE *lf = fopen(look, "r");
        if (lf) {
            char line[256];
            while (fgets(line, sizeof(line), lf)) {
                read_cfg_val(line, "LOOK_CARD", surface, sizeof(surface));
                read_cfg_val(line, "LOOK_EDGE", border_inact, sizeof(border_inact));
                read_hex_alpha(line, "LOOK_EDGE_AA", &edge_alpha);
                read_cfg_val(line, "LOOK_TEXT", text_col, sizeof(text_col));
                read_cfg_val(line, "LOOK_MUTED", muted, sizeof(muted));
                read_cfg_val(line, "LOOK_ACCENT", border_act, sizeof(border_act));
                read_cfg_val(line, "LOOK_ACCENT_TEXT", accent_text, sizeof(accent_text));
                read_cfg_val(line, "LOOK_ALERT", alert, sizeof(alert));
                read_radius_val(line, "LOOK_CORNERS", &radius);
                read_radius_val(line, "LOOK_SMALL_RADIUS", &small);
                read_font_val(line, "LOOK_FONT", font, sizeof(font));
            }
            fclose(lf);
        }
        g_free(look);
    }

    if (small < 0) small = (radius * 55 + 50) / 100;
    if (accent_text[0] == '\0') g_strlcpy(accent_text, border_act, sizeof(accent_text));
    if (alert[0] == '\0') g_strlcpy(alert, accent_text, sizeof(alert));
    if (muted[0] == '\0') {
        unsigned int t_rgb[3], s_rgb[3];
        if (parse_hex_bytes(text_col, t_rgb) && parse_hex_bytes(surface, s_rgb)) {
            unsigned int r = (unsigned int)(t_rgb[0] + ((double)s_rgb[0] - (double)t_rgb[0]) * 0.32 + 0.5);
            unsigned int g = (unsigned int)(t_rgb[1] + ((double)s_rgb[1] - (double)t_rgb[1]) * 0.32 + 0.5);
            unsigned int b = (unsigned int)(t_rgb[2] + ((double)s_rgb[2] - (double)t_rgb[2]) * 0.32 + 0.5);
            g_snprintf(muted, sizeof(muted), "#%02x%02x%02x", r, g, b);
        } else {
            g_snprintf(muted, sizeof(muted), "alpha(%s, 0.68)", text_col);
        }
    }

    char font_css[160];
    if (g_ascii_strcasecmp(font, "sans") == 0 || g_ascii_strcasecmp(font, "sans-serif") == 0) {
        g_snprintf(font_css, sizeof(font_css), "sans-serif");
    } else if (g_ascii_strcasecmp(font, "serif") == 0 || g_ascii_strcasecmp(font, "monospace") == 0) {
        g_snprintf(font_css, sizeof(font_css), "%s", font);
    } else {
        g_snprintf(font_css, sizeof(font_css), "\"%s\", sans-serif", font);
    }

    char edge_alpha_str[G_ASCII_DTOSTR_BUF_SIZE];
    g_ascii_formatd(edge_alpha_str, sizeof(edge_alpha_str), "%.2f", edge_alpha);

    return g_strdup_printf(
        "window.infynitae-polkit {\n"
        "    background-color: %s;\n"
        "    color: %s;\n"
        "    border: 1px solid alpha(%s, %s);\n"
        "    border-radius: %dpx;\n"
        "    font-family: %s;\n"
        "    font-size: 13px;\n"
        "}\n"
        "label.title {\n"
        "    color: %s;\n"
        "    font-size: 15px;\n"
        "    font-weight: 600;\n"
        "}\n"
        "label.action {\n"
        "    color: %s;\n"
        "    font-size: 11px;\n"
        "}\n"
        "label.error {\n"
        "    color: %s;\n"
        "    font-size: 11px;\n"
        "    font-weight: 600;\n"
        "}\n"
        "entry.pass {\n"
        "    background-color: alpha(%s, 0.12);\n"
        "    background-image: none;\n"
        "    color: %s;\n"
        "    border: 1px solid alpha(%s, %s);\n"
        "    border-radius: %dpx;\n"
        "    padding: 6px 10px;\n"
        "    font-size: 13px;\n"
        "    box-shadow: none;\n"
        "}\n"
        "entry.pass:focus {\n"
        "    border: 1px solid %s;\n"
        "}\n"
        "button.btn-auth {\n"
        "    background-color: alpha(%s, 0.16);\n"
        "    background-image: none;\n"
        "    color: %s;\n"
        "    font-size: 12px;\n"
        "    font-weight: 600;\n"
        "    border-radius: %dpx;\n"
        "    padding: 6px 14px;\n"
        "    border: none;\n"
        "    box-shadow: none;\n"
        "    text-shadow: none;\n"
        "}\n"
        "button.btn-auth:hover {\n"
        "    background-color: alpha(%s, 0.22);\n"
        "}\n"
        "button.btn-cancel {\n"
        "    background-color: alpha(%s, 0.08);\n"
        "    background-image: none;\n"
        "    color: %s;\n"
        "    font-size: 12px;\n"
        "    border-radius: %dpx;\n"
        "    padding: 6px 14px;\n"
        "    border: none;\n"
        "    box-shadow: none;\n"
        "    text-shadow: none;\n"
        "}\n"
        "button.btn-cancel:hover {\n"
        "    background-color: alpha(%s, 0.14);\n"
        "    color: %s;\n"
        "}\n",
        surface, text_col, border_inact, edge_alpha_str, radius, font_css, /* the dialog */
        text_col,                                                          /* title */
        muted,                                                             /* action and prompt */
        alert,                                                             /* error */
        text_col, text_col, border_inact, edge_alpha_str, small,           /* the password field */
        border_act,                                                        /* the field focused */
        border_act, accent_text, small,                                    /* Allow */
        border_act,                                                        /* Allow pointed at */
        text_col, muted, small,                                            /* Cancel */
        text_col, text_col);                                               /* Cancel pointed at */
}

static void apply_css(void) {
    static GtkCssProvider *provider = NULL;
    if (!provider) {
        provider = gtk_css_provider_new();
        gtk_style_context_add_provider_for_screen(
            gdk_screen_get_default(),
            GTK_STYLE_PROVIDER(provider),
            GTK_STYLE_PROVIDER_PRIORITY_APPLICATION
        );
    }
    gchar *css = build_css();
    gtk_css_provider_load_from_data(provider, css, -1, NULL);
    g_free(css);
}

/* GTK starts with the first prompt, not with the session: until a program asks for a password,
 * the agent is only its D-Bus registration (GTK's display connection, theme and CSS were most of
 * its memory, held all session for a dialog seldom shown). TRUE once GTK is up. */
static gboolean ensure_gtk(void) {
    static gboolean up;
    if (!up && gtk_init_check(NULL, NULL)) {
        up = TRUE;
    }
    if (up) apply_css();
    return up;
}

static void infynitae_agent_listener_initiate_authentication(
    PolkitAgentListener *listener,
    const gchar *action_id,
    const gchar *message,
    const gchar *icon_name,
    PolkitDetails *details,
    const gchar *cookie,
    GList *identities,
    GCancellable *cancellable,
    GAsyncReadyCallback callback,
    gpointer user_data)
{
    (void)icon_name;
    (void)details;

    AuthDialogData *data = g_new0(AuthDialogData, 1);
    data->listener = INFYNITAE_AGENT_LISTENER(listener);
    data->task = g_task_new(listener, cancellable, callback, user_data);
    data->cancellable = cancellable;

    /* the account whose password is asked: your own when polkit accepts it, else the first
     * one it names; the dialog says which (it is never a guess the user cannot see) */
    PolkitIdentity *identity = NULL;
    for (GList *l = identities; l != NULL; l = l->next) {
        if (POLKIT_IS_UNIX_USER(l->data) &&
            (uid_t)polkit_unix_user_get_uid(POLKIT_UNIX_USER(l->data)) == getuid()) {
            identity = POLKIT_IDENTITY(l->data);
            break;
        }
    }
    if (identity == NULL && identities != NULL) {
        identity = POLKIT_IDENTITY(identities->data);
    }
    if (identity == NULL) {
        g_task_return_new_error(data->task, POLKIT_ERROR, POLKIT_ERROR_FAILED, "No identity provided for authentication");
        g_object_unref(data->task);
        g_free(data);
        return;
    }

    data->identity = g_object_ref(identity);
    data->cookie = g_strdup(cookie);

    if (cancellable && g_cancellable_is_cancelled(cancellable)) {
        /* g_cancellable_connect would run the handler now, freeing data under us */
        g_task_return_new_error(data->task, POLKIT_ERROR, POLKIT_ERROR_CANCELLED, "Authentication cancelled");
        free_auth_dialog(data);
        return;
    }
    if (cancellable) {
        data->cancellable_handler_id = g_cancellable_connect(
            cancellable,
            G_CALLBACK(on_cancellable_cancelled),
            data,
            NULL
        );
    }

    if (!ensure_gtk()) {
        g_task_return_new_error(data->task, POLKIT_ERROR, POLKIT_ERROR_FAILED,
                                "No display to show the password prompt on");
        free_auth_dialog(data);
        return;
    }

    data->window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(data->window), "Password needed");
    gtk_window_set_position(GTK_WINDOW(data->window), GTK_WIN_POS_CENTER);
    gtk_window_set_resizable(GTK_WINDOW(data->window), FALSE);
    gtk_window_set_keep_above(GTK_WINDOW(data->window), TRUE);
    gtk_window_set_modal(GTK_WINDOW(data->window), TRUE);
    gtk_window_set_decorated(GTK_WINDOW(data->window), FALSE);

    GtkStyleContext *context = gtk_widget_get_style_context(data->window);
    gtk_style_context_add_class(context, "infynitae-polkit");

    g_signal_connect(data->window, "key-press-event", G_CALLBACK(on_window_key_press), data);

    GtkWidget *vbox = gtk_box_new(GTK_ORIENTATION_VERTICAL, 14);
    gtk_container_set_border_width(GTK_CONTAINER(vbox), 24);
    gtk_container_add(GTK_CONTAINER(data->window), vbox);

    GtkWidget *lbl_title = gtk_label_new("Password needed");
    gtk_widget_set_halign(lbl_title, GTK_ALIGN_START);
    gtk_style_context_add_class(gtk_widget_get_style_context(lbl_title), "title");
    gtk_box_pack_start(GTK_BOX(vbox), lbl_title, FALSE, FALSE, 0);

    gchar *action_desc = g_strdup_printf("%s\n%s", message ? message : "A program asks to change the system.", action_id ? action_id : "");
    GtkWidget *lbl_action = gtk_label_new(action_desc);
    g_free(action_desc);
    gtk_label_set_line_wrap(GTK_LABEL(lbl_action), TRUE);
    gtk_label_set_max_width_chars(GTK_LABEL(lbl_action), 48);
    gtk_widget_set_halign(lbl_action, GTK_ALIGN_START);
    gtk_style_context_add_class(gtk_widget_get_style_context(lbl_action), "action");
    gtk_box_pack_start(GTK_BOX(vbox), lbl_action, FALSE, FALSE, 0);

    /* whose password: plain text, the identity's user name (or group) */
    gchar *who = identity_name(data->identity);
    gchar *who_text = g_strdup_printf("Password for %s", who);
    GtkWidget *lbl_who = gtk_label_new(who_text);
    g_free(who_text);
    g_free(who);
    gtk_widget_set_halign(lbl_who, GTK_ALIGN_START);
    gtk_box_pack_start(GTK_BOX(vbox), lbl_who, FALSE, FALSE, 0);

    data->lbl_error = gtk_label_new("");
    gtk_widget_set_halign(data->lbl_error, GTK_ALIGN_START);
    gtk_style_context_add_class(gtk_widget_get_style_context(data->lbl_error), "error");
    gtk_widget_set_no_show_all(data->lbl_error, TRUE);
    gtk_box_pack_start(GTK_BOX(vbox), data->lbl_error, FALSE, FALSE, 0);

    data->lbl_prompt = gtk_label_new("Password: ");
    gtk_widget_set_halign(data->lbl_prompt, GTK_ALIGN_START);
    gtk_style_context_add_class(gtk_widget_get_style_context(data->lbl_prompt), "action");
    gtk_box_pack_start(GTK_BOX(vbox), data->lbl_prompt, FALSE, FALSE, 0);

    data->entry = gtk_entry_new();
    gtk_entry_set_visibility(GTK_ENTRY(data->entry), FALSE);
    gtk_style_context_add_class(gtk_widget_get_style_context(data->entry), "pass");
    g_signal_connect(data->entry, "activate", G_CALLBACK(on_submit_clicked), data);
    gtk_box_pack_start(GTK_BOX(vbox), data->entry, FALSE, FALSE, 0);

    GtkWidget *btn_box = gtk_box_new(GTK_ORIENTATION_HORIZONTAL, 12);
    gtk_widget_set_halign(btn_box, GTK_ALIGN_END);
    gtk_box_pack_start(GTK_BOX(vbox), btn_box, FALSE, FALSE, 8);

    data->btn_cancel = gtk_button_new_with_label("Cancel");
    gtk_style_context_add_class(gtk_widget_get_style_context(data->btn_cancel), "btn-cancel");
    g_signal_connect(data->btn_cancel, "clicked", G_CALLBACK(on_cancel_clicked), data);
    gtk_box_pack_start(GTK_BOX(btn_box), data->btn_cancel, FALSE, FALSE, 0);

    data->btn_auth = gtk_button_new_with_label("Allow");
    gtk_style_context_add_class(gtk_widget_get_style_context(data->btn_auth), "btn-auth");
    g_signal_connect(data->btn_auth, "clicked", G_CALLBACK(on_submit_clicked), data);
    gtk_box_pack_start(GTK_BOX(btn_box), data->btn_auth, FALSE, FALSE, 0);

    gtk_widget_show_all(data->window);
    start_session(data);
}

static gboolean infynitae_agent_listener_initiate_authentication_finish(
    PolkitAgentListener *listener,
    GAsyncResult *res,
    GError **error)
{
    (void)listener;
    return g_task_propagate_boolean(G_TASK(res), error);
}

static void infynitae_agent_listener_class_init(InfynitaeAgentListenerClass *klass) {
    PolkitAgentListenerClass *parent = POLKIT_AGENT_LISTENER_CLASS(klass);
    parent->initiate_authentication = infynitae_agent_listener_initiate_authentication;
    parent->initiate_authentication_finish = infynitae_agent_listener_initiate_authentication_finish;
}

static void infynitae_agent_listener_init(InfynitaeAgentListener *listener) {
    (void)listener;
}

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--print-css") == 0) {
        gchar *css = build_css();
        g_print("%s", css);
        g_free(css);
        return 0;
    }

    GError *error = NULL;
    InfynitaeAgentListener *listener = g_object_new(INFYNITAE_TYPE_AGENT_LISTENER, NULL);
    PolkitSubject *subject = polkit_unix_session_new_for_process_sync(getpid(), NULL, &error);
    if (!subject) {
        g_printerr("[infynitae-polkit] Failed to get session subject: %s\n", error ? error->message : "unknown");
        g_clear_error(&error);
        subject = (PolkitSubject *)polkit_system_bus_name_new("org.freedesktop.PolicyKit1");
    }

    gpointer reg_handle = polkit_agent_listener_register(
        POLKIT_AGENT_LISTENER(listener),
        POLKIT_AGENT_REGISTER_FLAGS_NONE,
        subject,
        NULL, // Object path (NULL = default /org/freedesktop/PolicyKit1/AuthenticationAgent)
        NULL,
        &error
    );

    if (!reg_handle) {
        g_printerr("[infynitae-polkit] Failed to register authentication agent: %s\n", error ? error->message : "unknown");
        g_clear_error(&error);
        g_object_unref(listener);
        return 1;
    }

    g_print("[infynitae-polkit] Agent registered on D-Bus.\n");
    /* GTK's events run in this loop too, once a prompt started it (the default main context) */
    GMainLoop *loop = g_main_loop_new(NULL, FALSE);
    g_main_loop_run(loop);
    g_main_loop_unref(loop);

    polkit_agent_listener_unregister(reg_handle);
    g_object_unref(listener);
    return 0;
}
