/*
 * Infynitae Native Modular Polkit Authentication Agent
 * Zero-bloat, lightweight PolicyKit authentication dialog for Wayland.
 *
 * Implements PolkitAgentListener, registers with org.freedesktop.PolicyKit1.Authority,
 * and creates transient GTK3 Wayland dialogs upon challenge.
 * Zero-cost idle: 0 Wayland surfaces when dormant (<10MB RAM).
 */

#define POLKIT_AGENT_I_KNOW_API_IS_SUBJECT_TO_CHANGE 1

#include <polkitagent/polkitagent.h>
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

    gboolean authenticated;
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
        gtk_label_set_text(GTK_LABEL(data->lbl_error), text ? text : "Authentication failed");
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
    data->authenticated = gained_authorization;

    if (gained_authorization) {
        if (data->task && !g_task_get_completed(data->task)) {
            g_task_return_boolean(data->task, TRUE);
        }
        free_auth_dialog(data);
    } else {
        if (data->lbl_error) {
            gtk_label_set_text(GTK_LABEL(data->lbl_error), "Authentication failed. Try again.");
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

static void read_cfg_val(const char *line, const char *key, char *dst, size_t dst_sz) {
    size_t klen = strlen(key);
    if (strncmp(line, key, klen) != 0 || line[klen] != '=') return;
    const char *p = line + klen + 1;
    while (*p == ' ' || *p == '\t' || *p == '"' || *p == '\'') p++;
    size_t j = 0;
    while (p[j] && p[j] != '"' && p[j] != '\'' && p[j] != '\n' && p[j] != '\r' && j + 1 < dst_sz) {
        dst[j] = p[j];
        j++;
    }
    if (j > 0) dst[j] = '\0';
}

static void apply_css(void) {
    char surface[32] = "#111317";
    char canvas[32] = "#0b0f14";
    char border_act[32] = "#7aa2f7";
    char border_inact[32] = "#333b54";
    char text_col[32] = "#eef2f8";

    const char *home = g_get_home_dir();
    if (home) {
        gchar *cfg_path = g_build_filename(home, ".config", "infynitae", "config", NULL);
        FILE *f = fopen(cfg_path, "r");
        if (f) {
            char line[256];
            while (fgets(line, sizeof(line), f)) {
                char * s = line;
                while (*s == ' ' || *s == '\t') s++;
                if (*s == '#') continue;
                read_cfg_val(s, "SURFACE_COLOR", surface, sizeof(surface));
                read_cfg_val(s, "CANVAS_COLOR", canvas, sizeof(canvas));
                read_cfg_val(s, "BORDER_ACTIVE", border_act, sizeof(border_act));
                read_cfg_val(s, "BORDER_INACTIVE", border_inact, sizeof(border_inact));
                read_cfg_val(s, "TEXT_COLOR", text_col, sizeof(text_col));
            }
            fclose(f);
        }
        g_free(cfg_path);
    }

    GtkCssProvider *provider = gtk_css_provider_new();
    gchar *css = g_strdup_printf(
        "window.infynitae-polkit {\n"
        "    background-color: %s;\n"
        "    border: 1px solid %s;\n"
        "    border-radius: 0px;\n"
        "    font-family: Sans;\n"
        "}\n"
        "label.title {\n"
        "    color: %s;\n"
        "    font-size: 13px;\n"
        "    font-weight: 600;\n"
        "}\n"
        "label.action {\n"
        "    color: #9aa5b8;\n"
        "    font-size: 11px;\n"
        "}\n"
        "label.error {\n"
        "    color: #f7768e;\n"
        "    font-size: 11px;\n"
        "    font-weight: 600;\n"
        "}\n"
        "entry.pass {\n"
        "    background-color: %s;\n"
        "    color: %s;\n"
        "    border: 1px solid %s;\n"
        "    border-radius: 0px;\n"
        "    padding: 6px 10px;\n"
        "    font-size: 12px;\n"
        "}\n"
        "entry.pass:focus {\n"
        "    border: 1px solid %s;\n"
        "}\n"
        "button.btn-auth {\n"
        "    background-color: #262b38;\n"
        "    color: %s;\n"
        "    font-weight: 600;\n"
        "    border-radius: 0px;\n"
        "    padding: 6px 14px;\n"
        "    border: 1px solid %s;\n"
        "}\n"
        "button.btn-auth:hover {\n"
        "    background-color: #303748;\n"
        "}\n"
        "button.btn-cancel {\n"
        "    background-color: %s;\n"
        "    color: #9aa5b8;\n"
        "    border-radius: 0px;\n"
        "    padding: 6px 14px;\n"
        "    border: 1px solid %s;\n"
        "}\n"
        "button.btn-cancel:hover {\n"
        "    background-color: #1c2028;\n"
        "    color: %s;\n"
        "}\n",
        surface, border_inact, text_col, canvas, text_col, border_inact,
        border_act, text_col, border_act, surface, border_inact, text_col);

    gtk_css_provider_load_from_data(provider, css, -1, NULL);
    g_free(css);
    gtk_style_context_add_provider_for_screen(
        gdk_screen_get_default(),
        GTK_STYLE_PROVIDER(provider),
        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION
    );
    g_object_unref(provider);
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

    PolkitIdentity *identity = NULL;
    if (identities != NULL) {
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

    // Build GUI Window
    data->window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
    gtk_window_set_title(GTK_WINDOW(data->window), "Authentication Required — Infynitae");
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

    GtkWidget *lbl_title = gtk_label_new("🔒 Authentication Required");
    gtk_widget_set_halign(lbl_title, GTK_ALIGN_START);
    gtk_style_context_add_class(gtk_widget_get_style_context(lbl_title), "title");
    gtk_box_pack_start(GTK_BOX(vbox), lbl_title, FALSE, FALSE, 0);

    gchar *action_desc = g_strdup_printf("%s\nAction: %s", message ? message : "Privilege escalation requested.", action_id ? action_id : "");
    GtkWidget *lbl_action = gtk_label_new(action_desc);
    g_free(action_desc);
    gtk_label_set_line_wrap(GTK_LABEL(lbl_action), TRUE);
    gtk_label_set_max_width_chars(GTK_LABEL(lbl_action), 48);
    gtk_widget_set_halign(lbl_action, GTK_ALIGN_START);
    gtk_style_context_add_class(gtk_widget_get_style_context(lbl_action), "action");
    gtk_box_pack_start(GTK_BOX(vbox), lbl_action, FALSE, FALSE, 0);

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

    data->btn_auth = gtk_button_new_with_label("Authenticate");
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
    gtk_init(&argc, &argv);
    apply_css();

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

    g_print("[infynitae-polkit] PolicyKit Authentication Agent registered successfully on D-Bus.\n");
    gtk_main();

    polkit_agent_listener_unregister(reg_handle);
    g_object_unref(listener);
    return 0;
}
