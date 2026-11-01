/* The edge's app launcher: finding the apps, searching them and remembering what was started.
 * Kept apart from the drawing so tools/test-edge-app-search can run it. */
#ifndef INFYNITAE_EDGE_APP_SEARCH_H
#define INFYNITAE_EDGE_APP_SEARCH_H

#include <dirent.h>
#include <glib.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define MAX_APPS 512
#define MAX_RECENT_APPS 64

typedef struct {
    char id[128]; /* the .desktop file's name: what the recent list remembers */
    char name[64];
    char exec_cmd[256];
    char generic[64];
    char keywords[192];
    long last_launched; /* seconds since the epoch; 0: not started from here */
} DesktopApp;

typedef struct {
    long when;
    char id[128];
} RecentApp;

typedef struct {
    RecentApp items[MAX_RECENT_APPS]; /* most recent first */
    int count;
} RecentApps;

/* ---- Finding the apps ------------------------------------------------------------------- */

static void app_trim_line(char *s) {
    s[strcspn(s, "\r\n")] = '\0';
}

/* Exec= without its field codes (%U, %f…), keeping what follows them; %% is a literal %. */
static void app_clean_exec(char *exec_cmd) {
    char *out = exec_cmd;
    for (const char *p = exec_cmd; *p; p++) {
        if (p[0] == '%' && p[1] == '%') {
            *out++ = '%';
            p++;
        } else if (p[0] == '%' && p[1] != '\0') {
            p++;
            while (out > exec_cmd && out[-1] == ' ') out--;
        } else {
            *out++ = *p;
        }
    }
    *out = '\0';
}

/* One applications directory. A file name seen in an earlier directory is skipped, also when that
 * file hides the app: a NoDisplay copy in ~/.local/share/applications hides the system's. */
static void app_load_dir(DesktopApp *apps, int *count, int cap, char (*seen)[128], int *seen_count,
                         int seen_cap, const char *dir_path) {
    DIR *d = opendir(dir_path);
    if (!d) return;
    struct dirent *de;
    while ((de = readdir(d)) != NULL && *count < cap) {
        size_t nlen = strlen(de->d_name);
        if (nlen < 9 || nlen >= sizeof(apps[0].id) || strcmp(de->d_name + nlen - 8, ".desktop") != 0) continue;

        bool already = false;
        for (int i = 0; i < *seen_count; i++) {
            if (strcmp(seen[i], de->d_name) == 0) {
                already = true;
                break;
            }
        }
        if (already) continue;
        if (*seen_count < seen_cap) g_strlcpy(seen[(*seen_count)++], de->d_name, 128);

        char path[768];
        snprintf(path, sizeof(path), "%s/%s", dir_path, de->d_name);
        FILE *f = fopen(path, "r");
        if (!f) continue;

        DesktopApp a;
        memset(&a, 0, sizeof a);
        g_strlcpy(a.id, de->d_name, sizeof a.id);
        char line[1024];
        bool in_entry = false, hidden = false, is_app = true;
        while (fgets(line, sizeof(line), f)) {
            if (line[0] == '[') {
                in_entry = strncmp(line, "[Desktop Entry]", 15) == 0;
                continue;
            }
            if (!in_entry) continue;
            app_trim_line(line);
            if (strcmp(line, "NoDisplay=true") == 0 || strcmp(line, "Hidden=true") == 0) hidden = true;
            else if (strncmp(line, "Type=", 5) == 0) is_app = strcmp(line + 5, "Application") == 0;
            else if (!a.name[0] && strncmp(line, "Name=", 5) == 0) g_strlcpy(a.name, line + 5, sizeof a.name);
            else if (!a.exec_cmd[0] && strncmp(line, "Exec=", 5) == 0) g_strlcpy(a.exec_cmd, line + 5, sizeof a.exec_cmd);
            else if (!a.generic[0] && strncmp(line, "GenericName=", 12) == 0) g_strlcpy(a.generic, line + 12, sizeof a.generic);
            else if (!a.keywords[0] && strncmp(line, "Keywords=", 9) == 0) g_strlcpy(a.keywords, line + 9, sizeof a.keywords);
        }
        fclose(f);
        if (hidden || !is_app || !a.name[0] || !a.exec_cmd[0]) continue;
        app_clean_exec(a.exec_cmd);

        bool dup = false;
        for (int i = 0; i < *count; i++) {
            if (strcasecmp(apps[i].name, a.name) == 0) {
                dup = true;
                break;
            }
        }
        if (!dup) apps[(*count)++] = a;
    }
    closedir(d);
}

/* Every applications directory, your own first (XDG base directories, and Flatpak's exports). */
static int app_load_all(DesktopApp *apps, int cap) {
    int count = 0, seen_count = 0;
    enum { SEEN_CAP = 2048 };
    char (*seen)[128] = malloc(SEEN_CAP * 128);
    if (!seen) return 0;
    char dir[640];
    const char *home = getenv("HOME");
    const char *data_home = getenv("XDG_DATA_HOME");
    if (data_home && *data_home) {
        snprintf(dir, sizeof dir, "%s/applications", data_home);
        app_load_dir(apps, &count, cap, seen, &seen_count, SEEN_CAP, dir);
    } else if (home) {
        snprintf(dir, sizeof dir, "%s/.local/share/applications", home);
        app_load_dir(apps, &count, cap, seen, &seen_count, SEEN_CAP, dir);
    }
    if (home) {
        snprintf(dir, sizeof dir, "%s/.local/share/flatpak/exports/share/applications", home);
        app_load_dir(apps, &count, cap, seen, &seen_count, SEEN_CAP, dir);
    }
    const char *data_dirs = getenv("XDG_DATA_DIRS");
    gchar **parts = g_strsplit(data_dirs && *data_dirs ? data_dirs : "/usr/local/share:/usr/share", ":", -1);
    for (gchar **p = parts; *p; p++) {
        if (!**p) continue;
        snprintf(dir, sizeof dir, "%s/applications", *p);
        app_load_dir(apps, &count, cap, seen, &seen_count, SEEN_CAP, dir);
    }
    g_strfreev(parts);
    app_load_dir(apps, &count, cap, seen, &seen_count, SEEN_CAP, "/var/lib/flatpak/exports/share/applications");
    free(seen);
    return count;
}

/* ---- What was started recently ---------------------------------------------------------- */

/* $XDG_STATE_HOME/infynitae/recent-apps: one "<seconds>\t<desktop file>" per line, newest first. */
static bool recent_apps_path(char *out, size_t size) {
    const char *state = getenv("XDG_STATE_HOME");
    const char *home = getenv("HOME");
    if (state && *state) snprintf(out, size, "%s/infynitae/recent-apps", state);
    else if (home) snprintf(out, size, "%s/.local/state/infynitae/recent-apps", home);
    else return false;
    return true;
}

static void recent_apps_load(RecentApps *recent) {
    recent->count = 0;
    char path[512], line[256];
    if (!recent_apps_path(path, sizeof path)) return;
    FILE *f = fopen(path, "r");
    if (!f) return;
    while (recent->count < MAX_RECENT_APPS && fgets(line, sizeof line, f)) {
        app_trim_line(line);
        char *tab = strchr(line, '\t');
        if (!tab || !tab[1]) continue;
        *tab = '\0';
        RecentApp *r = &recent->items[recent->count];
        r->when = atol(line);
        if (r->when <= 0) continue;
        snprintf(r->id, sizeof r->id, "%s", tab + 1);
        recent->count++;
    }
    fclose(f);
}

/* Written beside the file and renamed over it: a crash leaves the old list, never half of one. */
static void recent_apps_save(const RecentApps *recent) {
    char path[512], tmp[544];
    if (!recent_apps_path(path, sizeof path)) return;
    gchar *dir = g_path_get_dirname(path);
    g_mkdir_with_parents(dir, 0700);
    g_free(dir);
    snprintf(tmp, sizeof tmp, "%s.%d", path, (int)getpid());
    FILE *f = fopen(tmp, "w");
    if (!f) return;
    for (int i = 0; i < recent->count; i++) fprintf(f, "%ld\t%s\n", recent->items[i].when, recent->items[i].id);
    if (fclose(f) != 0 || rename(tmp, path) != 0) unlink(tmp);
}

/* The app goes to the top of the list, once. */
static void recent_apps_note(RecentApps *recent, const char *id, long when) {
    int found = -1;
    for (int i = 0; i < recent->count; i++) {
        if (strcmp(recent->items[i].id, id) == 0) {
            found = i;
            break;
        }
    }
    int last = found >= 0 ? found : (recent->count < MAX_RECENT_APPS ? recent->count++ : MAX_RECENT_APPS - 1);
    memmove(&recent->items[1], &recent->items[0], (size_t)last * sizeof recent->items[0]);
    recent->items[0].when = when;
    snprintf(recent->items[0].id, sizeof recent->items[0].id, "%s", id);
}

static void recent_apps_apply(const RecentApps *recent, DesktopApp *apps, int count) {
    for (int a = 0; a < count; a++) {
        apps[a].last_launched = 0;
        for (int i = 0; i < recent->count; i++) {
            if (strcmp(recent->items[i].id, apps[a].id) == 0) {
                apps[a].last_launched = recent->items[i].when;
                break;
            }
        }
    }
}

/* ---- Searching -------------------------------------------------------------------------- */

enum { MATCH_NONE, MATCH_FUZZY, MATCH_OTHER, MATCH_INSIDE, MATCH_WORD_START };

static bool app_is_word_gap(char c) {
    return c == ' ' || c == '-' || c == '_' || c == '.' || c == ':' || c == '/' || c == '(' || c == ';';
}

/* Letters of the word in order, gaps allowed ("frfx" finds firefox); two letters at least. */
static bool app_is_subsequence(const char *hay, const char *word) {
    if (strlen(word) < 2) return false;
    for (; *hay && *word; hay++) {
        if (*hay == *word) word++;
    }
    return *word == '\0';
}

/* A name cut at its length limit can end inside a letter: folding needs valid text. */
static gchar *app_fold(const char *text) {
    gchar *valid = g_utf8_make_valid(text, -1);
    gchar *folded = g_utf8_casefold(valid, -1);
    g_free(valid);
    return folded;
}

static int app_match_word(const char *name, const char *other, const char *word) {
    for (const char *p = name; (p = strstr(p, word)) != NULL; p++) {
        if (p == name || app_is_word_gap(p[-1])) return MATCH_WORD_START;
    }
    if (strstr(name, word)) return MATCH_INSIDE;
    if (strstr(other, word)) return MATCH_OTHER;
    return app_is_subsequence(name, word) ? MATCH_FUZZY : MATCH_NONE;
}

/* The command's own name: /usr/bin/foo --bar is foo, env A=1 foo is foo. */
static void app_command_name(const char *exec_cmd, char *out, size_t size) {
    gchar **tokens = g_strsplit_set(exec_cmd, " \t", -1);
    out[0] = '\0';
    for (gchar **t = tokens; *t; t++) {
        if (!**t || strcmp(*t, "env") == 0 || strchr(*t, '=')) continue;
        const char *slash = strrchr(*t, '/');
        snprintf(out, size, "%s", slash ? slash + 1 : *t);
        break;
    }
    g_strfreev(tokens);
}

typedef struct {
    int index;
    int group;
    long last_launched;
    gchar *name_fold;
} AppRank;

static int app_rank_compare(const void *pa, const void *pb) {
    const AppRank *a = pa, *b = pb;
    if (a->group != b->group) return a->group - b->group;
    if (a->last_launched != b->last_launched) return a->last_launched > b->last_launched ? -1 : 1;
    int by_name = strcmp(a->name_fold, b->name_fold);
    return by_name ? by_name : a->index - b->index;
}

/* The apps matching the query, best first:
 *   an exact name; apps started recently whose name matches, latest first; other names matching
 *   at the start of a word; other names matching inside a word; a match in the description,
 *   keywords or command; letters in order with gaps. Apps started recently go first within each.
 * With no query: the recent apps, then all the others by name. Every word of the query must match.
 * Returns the number written to out. */
static int app_search(const DesktopApp *apps, int count, const char *query, int *out, int cap) {
    AppRank *ranks = malloc(sizeof(AppRank) * (size_t)(count > 0 ? count : 1));
    if (!ranks) return 0;
    gchar *query_fold = app_fold(query ? query : "");
    gchar *stripped = g_strstrip(g_strdup(query_fold));
    gchar **words = g_strsplit_set(stripped, " \t", -1);
    int found = 0;
    for (int i = 0; i < count; i++) {
        const DesktopApp *a = &apps[i];
        gchar *name = app_fold(a->name);
        char command[128], other[768];
        app_command_name(a->exec_cmd, command, sizeof command);
        snprintf(other, sizeof other, "%s;%s;%s;%s", a->generic, a->keywords, command, a->id);
        gchar *other_fold = app_fold(other);

        int worst = MATCH_WORD_START;
        for (gchar **w = words; *w; w++) {
            if (!**w) continue;
            int m = app_match_word(name, other_fold, *w);
            if (m < worst) worst = m;
        }
        g_free(other_fold);
        if (worst == MATCH_NONE) {
            g_free(name);
            continue;
        }
        bool recent = a->last_launched > 0;
        int group;
        if (*stripped && strcmp(name, stripped) == 0) group = 0;
        else if (recent && worst >= MATCH_INSIDE) group = 1;
        else if (worst == MATCH_WORD_START) group = 2;
        else if (worst == MATCH_INSIDE) group = 3;
        else if (worst == MATCH_OTHER) group = 4;
        else group = 5;
        ranks[found++] = (AppRank){ i, group, a->last_launched, name };
    }
    qsort(ranks, (size_t)found, sizeof(AppRank), app_rank_compare);
    int written = 0;
    for (int i = 0; i < found; i++) {
        if (written < cap) out[written++] = ranks[i].index;
        g_free(ranks[i].name_fold);
    }
    g_strfreev(words);
    g_free(stripped);
    g_free(query_fold);
    free(ranks);
    return written;
}

#endif
