/* The lock screen's pictures, prepared by the wallpaper helper (it already decodes images, the
 * lock screen does not and should not): in $XDG_CACHE_HOME/infynitae (else ~/.cache/infynitae),
 *   lock-background.png  the wallpaper, at most 640 px wide, blurred (the lock screen scales it to
 *                        cover the screen and darkens it); absent when there is no image wallpaper
 *   avatar.png           the user's picture, 192 x 192, centre-cropped, from AccountsService
 *                        (/var/lib/AccountsService/icons/$USER), ~/.face or ~/.face.icon; absent
 *                        when there is none (the lock screen draws initials)
 * Each is written to a temporary file and renamed, so the lock screen never reads half a file.
 * owner, 2026-11-06: "the lock screen is mid, like the wallpaper on it and the user pfp" */
#ifndef INFYNITAE_LOCK_CACHE_H
#define INFYNITAE_LOCK_CACHE_H

#include <errno.h>
#include <gdk-pixbuf/gdk-pixbuf.h>
#include <pwd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define LOCK_BG_WIDTH 640
#define LOCK_AVATAR_SIZE 192

static int lock_cache_dir(char *out, size_t size) {
    const char *xdg = getenv("XDG_CACHE_HOME");
    const char *home = getenv("HOME");
    int n;
    if (xdg && xdg[0] == '/') n = snprintf(out, size, "%s/infynitae", xdg);
    else if (home && home[0]) n = snprintf(out, size, "%s/.cache/infynitae", home);
    else return -1;
    if (n < 0 || (size_t)n >= size) return -1;
    char parent[4096];
    snprintf(parent, sizeof(parent), "%s", out);
    char *slash = strrchr(parent, '/');
    if (slash && slash != parent) {
        *slash = '\0';
        if (mkdir(parent, 0700) != 0 && errno != EEXIST) return -1;
    }
    if (mkdir(out, 0700) != 0 && errno != EEXIST) return -1;
    return 0;
}

static int lock_cache_path(char *out, size_t size, const char *name) {
    char dir[4096];
    if (lock_cache_dir(dir, sizeof(dir)) != 0) return -1;
    int n = snprintf(out, size, "%s/%s", dir, name);
    return (n < 0 || (size_t)n >= size) ? -1 : 0;
}

/* Saves `pb` as PNG to the cache file `name` through a temporary file. */
static int lock_cache_save(GdkPixbuf *pb, const char *name) {
    char path[4096], tmp[4200];
    if (lock_cache_path(path, sizeof(path), name) != 0) return -1;
    snprintf(tmp, sizeof(tmp), "%s.tmp.%d", path, (int)getpid());
    if (!gdk_pixbuf_save(pb, tmp, "png", NULL, NULL)) {
        unlink(tmp);
        return -1;
    }
    if (rename(tmp, path) != 0) {
        unlink(tmp);
        return -1;
    }
    return 0;
}

static void lock_cache_remove(const char *name) {
    char path[4096];
    if (lock_cache_path(path, sizeof(path), name) == 0) unlink(path);
}

/* One horizontal and one vertical box-blur pass of radius r over an RGB(A) buffer, in place. */
static void lock_cache_box_blur(guchar *px, int w, int h, int stride, int nch, int r) {
    int maxdim = w > h ? w : h;
    unsigned *acc = calloc((size_t)maxdim * 3, sizeof(unsigned));
    guchar *line = malloc((size_t)maxdim * 3);
    if (!acc || !line) {
        free(acc);
        free(line);
        return;
    }
    for (int pass = 0; pass < 2; pass++) {
        int lines = pass == 0 ? h : w, len = pass == 0 ? w : h;
        for (int l = 0; l < lines; l++) {
            for (int i = 0; i < len; i++) {
                guchar *p = pass == 0 ? px + (size_t)l * stride + (size_t)i * nch : px + (size_t)i * stride + (size_t)l * nch;
                for (int c = 0; c < 3; c++) line[i * 3 + c] = p[c];
            }
            for (int i = 0; i < len; i++) {
                unsigned sum[3] = {0, 0, 0};
                int count = 0;
                for (int k = i - r; k <= i + r; k++) {
                    int j = k < 0 ? 0 : (k >= len ? len - 1 : k);
                    for (int c = 0; c < 3; c++) sum[c] += line[j * 3 + c];
                    count++;
                }
                for (int c = 0; c < 3; c++) acc[i * 3 + c] = sum[c] / (unsigned)count;
            }
            for (int i = 0; i < len; i++) {
                guchar *p = pass == 0 ? px + (size_t)l * stride + (size_t)i * nch : px + (size_t)i * stride + (size_t)l * nch;
                for (int c = 0; c < 3; c++) p[c] = (guchar)acc[i * 3 + c];
                if (nch == 4) p[3] = 255;
            }
        }
    }
    free(acc);
    free(line);
}

/* The blurred lock background from the decoded wallpaper. Returns 0 when written. */
static int lock_cache_write_background(GdkPixbuf *wallpaper) {
    if (!wallpaper) return -1;
    int w = gdk_pixbuf_get_width(wallpaper), h = gdk_pixbuf_get_height(wallpaper);
    if (w <= 0 || h <= 0) return -1;
    int nw = w > LOCK_BG_WIDTH ? LOCK_BG_WIDTH : w;
    int nh = (int)((double)h * nw / w + 0.5);
    if (nh < 1) nh = 1;
    GdkPixbuf *small = gdk_pixbuf_scale_simple(wallpaper, nw, nh, GDK_INTERP_BILINEAR);
    if (!small) return -1;
    GdkPixbuf *rgb = small;
    if (gdk_pixbuf_get_has_alpha(small)) {
        /* transparency is not wanted behind a lock screen: over black */
        rgb = gdk_pixbuf_composite_color_simple(small, nw, nh, GDK_INTERP_NEAREST, 255, 8, 0, 0);
        g_object_unref(small);
        if (!rgb) return -1;
    }
    int radius = nw / 80 > 2 ? nw / 80 : 2;
    for (int i = 0; i < 3; i++)
        lock_cache_box_blur(gdk_pixbuf_get_pixels(rgb), nw, nh, gdk_pixbuf_get_rowstride(rgb), gdk_pixbuf_get_n_channels(rgb), radius);
    int rc = lock_cache_save(rgb, "lock-background.png");
    g_object_unref(rgb);
    return rc;
}

/* The user's picture, centre-cropped to a square. Returns 0 when written, 1 when the user has
 * none (and any old one is removed), -1 on an error. */
static int lock_cache_write_avatar(void) {
    const char *home = getenv("HOME");
    struct passwd *pw = getpwuid(getuid());
    char candidates[3][4096];
    int n = 0;
    if (pw && pw->pw_name) snprintf(candidates[n++], sizeof(candidates[0]), "/var/lib/AccountsService/icons/%s", pw->pw_name);
    if (home && home[0]) {
        snprintf(candidates[n++], sizeof(candidates[0]), "%s/.face", home);
        snprintf(candidates[n++], sizeof(candidates[0]), "%s/.face.icon", home);
    }
    for (int i = 0; i < n; i++) {
        struct stat st;
        /* a picture, not a huge or odd file */
        if (stat(candidates[i], &st) != 0 || !S_ISREG(st.st_mode) || st.st_size <= 0 || st.st_size > 32 * 1024 * 1024) continue;
        GdkPixbuf *full = gdk_pixbuf_new_from_file(candidates[i], NULL);
        if (!full) continue;
        int w = gdk_pixbuf_get_width(full), h = gdk_pixbuf_get_height(full);
        int side = w < h ? w : h;
        GdkPixbuf *square = gdk_pixbuf_new_subpixbuf(full, (w - side) / 2, (h - side) / 2, side, side);
        GdkPixbuf *out = square ? gdk_pixbuf_scale_simple(square, LOCK_AVATAR_SIZE, LOCK_AVATAR_SIZE, GDK_INTERP_HYPER) : NULL;
        if (square) g_object_unref(square);
        g_object_unref(full);
        if (!out) continue;
        int rc = lock_cache_save(out, "avatar.png");
        g_object_unref(out);
        return rc;
    }
    lock_cache_remove("avatar.png");
    return 1;
}

#endif
