/*
 * infynitae-bus-watch: one process on the system bus for two session helpers, in place of two
 * shells, two `gdbus monitor`, systemd-inhibit, its pkttyagent and `sleep infinity`.
 *   infynitae-bus-watch [--udisks CMD] [--sleep CMD]
 * --udisks: runs `CMD scan` at start, then `CMD signal LINE` for each UDisks2 InterfacesAdded and
 * InterfacesRemoved, LINE as `gdbus monitor` prints it (live/infynitae-automount reads that).
 * --sleep: holds a logind "delay" inhibitor for sleep and runs `CMD start`; when sleep is announced
 * (PrepareForSleep true) it runs `CMD sleep`, and closes the inhibitor only when that exits 0 (the
 * session is locked: live/infynitae-sleep-lock), so without a lock the sleep waits for logind's
 * own limit. After the wake (PrepareForSleep false) it runs `CMD wake` and takes the inhibitor
 * again. Each feed runs one command at a time, in the order the signals came.
 * Started by session.sh; ends with the session.
 */

#include <errno.h>
#include <signal.h>
#include <fcntl.h>
#include <gio/gio.h>
#include <gio/gunixfdlist.h>
#include <glib-unix.h>
#include <stdio.h>
#include <string.h>
#include <sys/file.h>
#include <sys/wait.h>
#include <unistd.h>

typedef enum { AFTER_NOTHING, AFTER_RELEASE_IF_LOCKED, AFTER_TAKE } After;

typedef struct {
    char **argv;
    After after;
} Job;

typedef struct {
    const char *cmd;
    const char *log; // the log file, below $XDG_STATE_HOME/infynitae
    GQueue jobs;
    gboolean busy;
} Feed;

static Feed udisks = {.log = "automount.log"};
static Feed sleep_feed = {.log = "sleep-lock.log"};
static GDBusConnection *bus;
static int inhibitor = -1;
static GMainLoop *loop;

// a line in the feed's log, in the scripts' format (date '+%F %T.%3N')
static void log_line(Feed *f, const char *fmt, ...) G_GNUC_PRINTF(2, 3);
static void log_line(Feed *f, const char *fmt, ...) {
    const char *state = g_getenv("XDG_STATE_HOME");
    char *path = state && *state ? g_build_filename(state, "infynitae", f->log, NULL)
                                 : g_build_filename(g_get_home_dir(), ".local/state/infynitae", f->log, NULL);
    FILE *out = fopen(path, "a");
    g_free(path);
    if (!out)
        return;
    GDateTime *now = g_date_time_new_now_local();
    char *stamp = g_date_time_format(now, "%F %T");
    fprintf(out, "%s.%03d ", stamp, g_date_time_get_microsecond(now) / 1000);
    g_free(stamp);
    g_date_time_unref(now);
    va_list ap;
    va_start(ap, fmt);
    vfprintf(out, fmt, ap);
    va_end(ap);
    fputc('\n', out);
    fclose(out);
}

static void take(void) {
    if (inhibitor >= 0 || !bus)
        return;
    GError *err = NULL;
    GUnixFDList *fds = NULL;
    GVariant *r = g_dbus_connection_call_with_unix_fd_list_sync(
        bus, "org.freedesktop.login1", "/org/freedesktop/login1", "org.freedesktop.login1.Manager",
        "Inhibit", g_variant_new("(ssss)", "sleep", "Infynitae", "Locking the screen before sleep", "delay"),
        G_VARIANT_TYPE("(h)"), G_DBUS_CALL_FLAGS_NONE, -1, NULL, &fds, NULL, &err);
    if (r) {
        gint32 h;
        g_variant_get(r, "(h)", &h);
        // g_unix_fd_list_get gives a close-on-exec copy: the commands run below never hold it
        inhibitor = g_unix_fd_list_get(fds, h, &err);
        g_variant_unref(r);
        g_object_unref(fds);
    }
    if (inhibitor < 0) {
        log_line(&sleep_feed, "no sleep inhibitor from logind: %s", err ? err->message : "no file descriptor");
        g_clear_error(&err);
    }
}

static void release(void) {
    if (inhibitor >= 0)
        close(inhibitor);
    inhibitor = -1;
}

static void run_next(Feed *f);

static void job_done(GPid pid, gint status, gpointer data) {
    Feed *f = data;
    Job *j = g_queue_pop_head(&f->jobs);
    g_spawn_close_pid(pid);
    if (j->after == AFTER_RELEASE_IF_LOCKED && WIFEXITED(status) && WEXITSTATUS(status) == 0)
        release();
    else if (j->after == AFTER_TAKE)
        take();
    g_strfreev(j->argv);
    g_free(j);
    f->busy = FALSE;
    run_next(f);
}

static void run_next(Feed *f) {
    while (!f->busy && !g_queue_is_empty(&f->jobs)) {
        Job *j = g_queue_peek_head(&f->jobs);
        GError *err = NULL;
        GPid pid;
        // without G_SPAWN_LEAVE_DESCRIPTORS_OPEN only stdin, stdout and stderr pass to the command
        if (g_spawn_async(NULL, j->argv, NULL, G_SPAWN_DO_NOT_REAP_CHILD, NULL, NULL, &pid, &err)) {
            f->busy = TRUE;
            g_child_watch_add(pid, job_done, f);
            return;
        }
        log_line(f, "cannot run %s: %s", j->argv[0], err->message);
        g_error_free(err);
        g_queue_pop_head(&f->jobs);
        g_strfreev(j->argv);
        g_free(j);
    }
}

static void queue(Feed *f, const char *what, const char *line, After after) {
    Job *j = g_new0(Job, 1);
    j->argv = g_new0(char *, 4);
    j->argv[0] = g_strdup(f->cmd);
    j->argv[1] = g_strdup(what);
    j->argv[2] = g_strdup(line);
    j->after = after;
    g_queue_push_tail(&f->jobs, j);
    run_next(f);
}

static void on_udisks(GDBusConnection *c, const char *sender, const char *path, const char *iface,
                      const char *member, GVariant *params, gpointer data) {
    char *args = g_variant_print(params, TRUE);
    char *line = g_strdup_printf("%s: %s.%s %s", path, iface, member, args);
    queue(&udisks, "signal", line, AFTER_NOTHING);
    g_free(line);
    g_free(args);
}

static void on_sleep(GDBusConnection *c, const char *sender, const char *path, const char *iface,
                     const char *member, GVariant *params, gpointer data) {
    gboolean going;
    g_variant_get(params, "(b)", &going);
    if (going)
        queue(&sleep_feed, "sleep", NULL, AFTER_RELEASE_IF_LOCKED);
    else
        queue(&sleep_feed, "wake", NULL, AFTER_TAKE);
}

static gboolean reconnect(gpointer data);

static void on_closed(GDBusConnection *c, gboolean remote, GError *err, gpointer data) {
    // logind keeps the inhibitor while its file is open, bus or not: it stays held
    g_clear_object(&bus);
    g_timeout_add_seconds(2, reconnect, NULL);
}

static gboolean connect_bus(void) {
    GError *err = NULL;
    char *address = g_dbus_address_get_for_bus_sync(G_BUS_TYPE_SYSTEM, NULL, &err);
    if (address) {
        bus = g_dbus_connection_new_for_address_sync(
            address, G_DBUS_CONNECTION_FLAGS_AUTHENTICATION_CLIENT | G_DBUS_CONNECTION_FLAGS_MESSAGE_BUS_CONNECTION,
            NULL, NULL, &err);
        g_free(address);
    }
    if (!bus) {
        g_printerr("infynitae-bus-watch: no system bus: %s\n", err->message);
        g_error_free(err);
        return FALSE;
    }
    g_signal_connect(bus, "closed", G_CALLBACK(on_closed), NULL);
    if (udisks.cmd)
        g_dbus_connection_signal_subscribe(bus, "org.freedesktop.UDisks2", "org.freedesktop.DBus.ObjectManager",
                                           NULL, "/org/freedesktop/UDisks2", NULL, G_DBUS_SIGNAL_FLAGS_NONE,
                                           on_udisks, NULL, NULL);
    if (sleep_feed.cmd) {
        g_dbus_connection_signal_subscribe(bus, "org.freedesktop.login1", "org.freedesktop.login1.Manager",
                                           "PrepareForSleep", "/org/freedesktop/login1", NULL,
                                           G_DBUS_SIGNAL_FLAGS_NONE, on_sleep, NULL, NULL);
        take();
    }
    return TRUE;
}

// the system bus restarted (an update): its signals are watched again, 2 s apart until it answers
static gboolean reconnect(gpointer data) {
    if (!connect_bus())
        return G_SOURCE_CONTINUE;
    if (udisks.cmd)
        log_line(&udisks, "watching again after the system bus restarted");
    if (sleep_feed.cmd)
        log_line(&sleep_feed, "watching again after the system bus restarted");
    return G_SOURCE_REMOVE;
}

static gboolean on_stop(gpointer data) {
    g_main_loop_quit(loop);
    return G_SOURCE_REMOVE;
}

int main(int argc, char **argv) {
    for (int i = 1; i + 1 < argc; i += 2) {
        if (strcmp(argv[i], "--udisks") == 0)
            udisks.cmd = argv[i + 1];
        else if (strcmp(argv[i], "--sleep") == 0)
            sleep_feed.cmd = argv[i + 1];
    }
    if (argc % 2 == 0 || (!udisks.cmd && !sleep_feed.cmd)) {
        g_printerr("usage: infynitae-bus-watch [--udisks CMD] [--sleep CMD]\n");
        return 2;
    }

    // one per session: two would mount the same drive twice and race for the same sleep
    const char *run = g_getenv("XDG_RUNTIME_DIR");
    char *dir = g_build_filename(run && *run ? run : "/tmp", "infynitae", NULL);
    g_mkdir_with_parents(dir, 0700);
    char *lock_path = g_build_filename(dir, "bus-watch.lock", NULL);
    int lock = open(lock_path, O_WRONLY | O_CREAT | O_CLOEXEC, 0600);
    if (lock < 0) {
        g_printerr("infynitae-bus-watch: %s: %s\n", lock_path, g_strerror(errno));
        return 1;
    }
    if (flock(lock, LOCK_EX | LOCK_NB) != 0)
        return 0;
    g_free(lock_path);
    g_free(dir);

    loop = g_main_loop_new(NULL, FALSE);
    g_unix_signal_add(SIGTERM, on_stop, NULL);
    g_unix_signal_add(SIGINT, on_stop, NULL);
    g_unix_signal_add(SIGHUP, on_stop, NULL);

    // subscribed before the scan, so a drive plugged in during it is not missed
    if (!connect_bus())
        g_timeout_add_seconds(2, reconnect, NULL);
    if (udisks.cmd)
        queue(&udisks, "scan", NULL, AFTER_NOTHING);
    if (sleep_feed.cmd)
        queue(&sleep_feed, "start", NULL, AFTER_NOTHING);

    g_main_loop_run(loop);
    release();
    return 0;
}
