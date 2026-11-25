/*
 * The desktop journal, for Infynitae's C helpers: one event appended to
 * $XDG_STATE_HOME/infynitae/journal/journal.tsv (else ~/.local/state/...), in the compositor's
 * format (crates/infynitae-wayland/src/journal.rs): UTC time, event, program, pid, detail and
 * outcome, separated by tabs. One write(2) with O_APPEND, so a line never mixes with the
 * compositor's; the compositor rotates the file. A failure is silent: the journal records, it
 * never stops the prompt or the lock screen.
 */
#ifndef INFYNITAE_JOURNAL_H
#define INFYNITAE_JOURNAL_H

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

/* `in` with tab, newline, carriage return and backslash escaped, into `out` (cut at `size`) */
static void journal_escape(const char *in, char *out, size_t size) {
    size_t n = 0;
    for (; in && *in && n + 3 < size; in++) {
        const char *esc = NULL;
        switch (*in) {
        case '\\': esc = "\\\\"; break;
        case '\t': esc = "\\t"; break;
        case '\n': esc = "\\n"; break;
        case '\r': esc = "\\r"; break;
        }
        if (esc) {
            out[n++] = esc[0];
            out[n++] = esc[1];
        } else {
            out[n++] = *in;
        }
    }
    out[n] = '\0';
}

/* The program of `pid` as the trust code names a native client: the file /proc/<pid>/exe
 * points to. Empty when it cannot be read (the process is gone, or another user's). */
static void journal_program(pid_t pid, char *out, size_t size) {
    char link[64];
    snprintf(link, sizeof(link), "/proc/%d/exe", (int)pid);
    ssize_t n = pid > 0 ? readlink(link, out, size - 1) : -1;
    out[n > 0 ? n : 0] = '\0';
}

/* Appends one event; `pid` 0 is unknown (an empty program and pid). */
static void journal_write(const char *event, pid_t pid, const char *detail, const char *outcome) {
    char dir[PATH_MAX];
    const char *state = getenv("XDG_STATE_HOME");
    const char *home = getenv("HOME");
    if (state && *state) {
        snprintf(dir, sizeof(dir), "%s/infynitae/journal", state);
    } else if (home && *home) {
        snprintf(dir, sizeof(dir), "%s/.local/state/infynitae/journal", home);
    } else {
        return;
    }
    /* each missing folder on the way, as mkdir -p */
    for (char *p = dir + 1;; p++) {
        if (*p == '/' || *p == '\0') {
            char c = *p;
            *p = '\0';
            if (mkdir(dir, 0700) != 0 && errno != EEXIST) return;
            *p = c;
            if (c == '\0') break;
        }
    }
    char path[PATH_MAX + 16];
    snprintf(path, sizeof(path), "%s/journal.tsv", dir);

    char program[PATH_MAX], program_esc[2 * PATH_MAX], detail_esc[1024], when[32], pid_text[16];
    journal_program(pid, program, sizeof(program));
    journal_escape(program, program_esc, sizeof(program_esc));
    journal_escape(detail, detail_esc, sizeof(detail_esc));
    time_t now = time(NULL);
    struct tm tm;
    gmtime_r(&now, &tm);
    strftime(when, sizeof(when), "%Y-%m-%dT%H:%M:%SZ", &tm);
    if (pid > 0) snprintf(pid_text, sizeof(pid_text), "%d", (int)pid);
    else pid_text[0] = '\0';

    char line[4096];
    int len = snprintf(line, sizeof(line), "%s\t%s\t%s\t%s\t%s\t%s\n", when, event, program_esc,
                       pid_text, detail_esc, outcome);
    if (len <= 0) return;
    if ((size_t)len >= sizeof(line)) {
        len = sizeof(line) - 1;
        line[len - 1] = '\n';
    }
    int fd = open(path, O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC, 0600);
    if (fd < 0) return;
    ssize_t w = write(fd, line, (size_t)len);
    (void)w;
    close(fd);
}

#endif
