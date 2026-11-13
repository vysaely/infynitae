/* The desktop's colours from the wallpaper (owner, 2026-11-06: "change the theme to the wallpaper
 * accents"): `infynitae-wallpaper --colors IMAGE` prints them, `infynitae-settings
 * wallpaper-colors` writes them into the config, and every part follows the config as before.
 *
 * accent   the most present vivid hue: pixels are binned by hue (24 bins of 15 degrees), each
 *          weighted by saturation x brightness, so a large pale area does not beat a smaller vivid
 *          one; the winning bin's average colour, brought to a readable strength on a dark
 *          surface (saturation 0.45..0.85, brightness 0.80..0.95). A grey image gives the default.
 * surface  the image's average hue, very dark (brightness 0.09): panels, menus, lock box
 * canvas   the same, darker (0.055): behind windows
 * border   the surface lightened toward the accent: outlines and dividers
 * Light themes (COLOR_SCHEME=prefer-light) are not derived: the text colour is left alone. */
#ifndef INFYNITAE_THEME_COLORS_H
#define INFYNITAE_THEME_COLORS_H

#include <gdk-pixbuf/gdk-pixbuf.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    double accent[3], surface[3], canvas[3], border[3]; /* 0..1 */
} ThemeColors;

static void tc_rgb_to_hsv(const double c[3], double *h, double *s, double *v) {
    double mx = fmax(c[0], fmax(c[1], c[2])), mn = fmin(c[0], fmin(c[1], c[2])), d = mx - mn;
    *v = mx;
    *s = mx > 0 ? d / mx : 0;
    if (d <= 0) *h = 0;
    else if (mx == c[0]) *h = fmod((c[1] - c[2]) / d + 6.0, 6.0) * 60.0;
    else if (mx == c[1]) *h = ((c[2] - c[0]) / d + 2.0) * 60.0;
    else *h = ((c[0] - c[1]) / d + 4.0) * 60.0;
}

static void tc_hsv_to_rgb(double h, double s, double v, double out[3]) {
    double c = v * s, x = c * (1 - fabs(fmod(h / 60.0, 2.0) - 1)), m = v - c, r = 0, g = 0, b = 0;
    if (h < 60) { r = c; g = x; } else if (h < 120) { r = x; g = c; } else if (h < 180) { g = c; b = x; }
    else if (h < 240) { g = x; b = c; } else if (h < 300) { r = x; b = c; } else { r = c; b = x; }
    out[0] = r + m; out[1] = g + m; out[2] = b + m;
}

static double tc_clamp(double x, double lo, double hi) { return x < lo ? lo : x > hi ? hi : x; }

static int tc_sample_bins(GdkPixbuf *image, double bin_w[24], double bin_rgb[24][3],
                          double bin_xy[24][2], double *sum_x, double *sum_y, double *sum_w) {
    if (!image) return -1;
    int w = gdk_pixbuf_get_width(image), h = gdk_pixbuf_get_height(image);
    if (w <= 0 || h <= 0) return -1;
    /* 96 px wide is plenty to find colours without scanning every pixel */
    int sw = w > 96 ? 96 : w, sh = (int)((double)h * sw / w + 0.5);
    if (sh < 1) sh = 1;
    GdkPixbuf *small = gdk_pixbuf_scale_simple(image, sw, sh, GDK_INTERP_BILINEAR);
    if (!small) return -1;
    const guchar *px = gdk_pixbuf_get_pixels(small);
    int rs = gdk_pixbuf_get_rowstride(small), nch = gdk_pixbuf_get_n_channels(small);
    memset(bin_w, 0, 24 * sizeof(bin_w[0]));
    memset(bin_rgb, 0, 24 * sizeof(bin_rgb[0]));
    memset(bin_xy, 0, 24 * sizeof(bin_xy[0]));
    *sum_x = *sum_y = *sum_w = 0; /* the average hue, as a vector */
    for (int y = 0; y < sh; y++)
        for (int x = 0; x < sw; x++) {
            const guchar *p = px + y * rs + x * nch;
            if (nch == 4 && p[3] < 128) continue;
            double c[3] = {p[0] / 255.0, p[1] / 255.0, p[2] / 255.0}, hu, s, v;
            tc_rgb_to_hsv(c, &hu, &s, &v);
            double wt = s * v;
            *sum_x += cos(hu * M_PI / 180.0) * wt;
            *sum_y += sin(hu * M_PI / 180.0) * wt;
            *sum_w += wt;
            if (s < 0.25 || v < 0.25) continue; /* greys and shadows say nothing about the accent */
            int b = (int)(hu / 15.0) % 24;
            wt *= s * v; /* vivid pixels count more */
            bin_w[b] += wt;
            for (int k = 0; k < 3; k++) bin_rgb[b][k] += c[k] * wt;
            bin_xy[b][0] += ((x + 0.5) / sw) * wt;
            bin_xy[b][1] += ((y + 0.5) / sh) * wt;
        }
    g_object_unref(small);
    return 0;
}

static int tc_bin_dist(int a, int b) {
    int d = a - b;
    if (d < 0) d = -d;
    return d > 12 ? 24 - d : d;
}

static void tc_bin_accent(double bin_w, const double bin_rgb[3], double out[3]) {
    double c[3], ah, as, av;
    for (int k = 0; k < 3; k++) c[k] = bin_rgb[k] / bin_w;
    tc_rgb_to_hsv(c, &ah, &as, &av);
    as = tc_clamp(as, 0.45, 0.85);
    av = tc_clamp(av, 0.80, 0.95);
    tc_hsv_to_rgb(ah, as, av, out);
}

static int tc_accents_from_bins(const double bin_w[24], double bin_rgb[24][3],
                                const double bin_xy[24][2], double out[][3],
                                double pos[][2], int max) {
    if (!out || max <= 0) return -1;
    int cap = max < 6 ? max : 6; /* up to 6 distinct candidates */
    double total_w = 0, score[24] = {0};
    for (int b = 0; b < 24; b++) {
        total_w += bin_w[b];
        /* a hue split across a bin edge is one colour */
        score[b] = bin_w[b] + 0.5 * (bin_w[(b + 23) % 24] + bin_w[(b + 1) % 24]);
    }

    int picked[6], count = 0;
    for (int n = 0; n < cap; n++) {
        int best = -1;
        double best_w = 0;
        for (int b = 0; b < 24; b++) {
            if (bin_w[b] <= 0) continue;
            if (n > 0) {
                if (bin_w[b] < 0.04 * total_w) continue;
                int close = 0;
                for (int j = 0; j < count; j++)
                    if (tc_bin_dist(b, picked[j]) < 2) { close = 1; break; }
                if (close) continue;
            }
            if (score[b] > best_w) { best_w = score[b]; best = b; }
        }
        if (best < 0) break;
        picked[count] = best;
        tc_bin_accent(bin_w[best], bin_rgb[best], out[count]);
        if (pos) {
            pos[count][0] = tc_clamp(bin_xy[best][0] / bin_w[best], 0.0, 1.0);
            pos[count][1] = tc_clamp(bin_xy[best][1] / bin_w[best], 0.0, 1.0);
        }
        count++;
    }

    if (count == 0) {
        /* a grey image: Infynitae's own blue (#7aa2f7) */
        out[0][0] = 0x7a / 255.0; out[0][1] = 0xa2 / 255.0; out[0][2] = 0xf7 / 255.0;
        if (pos) {
            pos[0][0] = 0.5;
            pos[0][1] = 0.5;
        }
        count = 1;
    }
    return count;
}

/* Up to 6 distinct candidate accents, best first, bins at least 30 degrees apart,
 * plus the weighted pixel centre (x, y in 0..1) of each candidate's hue bin.
 * Returns candidate count (>= 1), or -1 on invalid arguments or unreadable image. */
static int theme_accents_from(GdkPixbuf *image, double out[][3], double pos[][2], int max) {
    if (!out || !pos || max <= 0) return -1;
    double bin_w[24], bin_rgb[24][3], bin_xy[24][2], sum_x, sum_y, sum_w;
    if (tc_sample_bins(image, bin_w, bin_rgb, bin_xy, &sum_x, &sum_y, &sum_w) != 0) return -1;
    return tc_accents_from_bins(bin_w, bin_rgb, bin_xy, out, pos, max);
}

/* Returns 0, or -1 for an image it cannot read. */
static int theme_colors_from(GdkPixbuf *image, ThemeColors *out) {
    if (!out) return -1;
    double bin_w[24], bin_rgb[24][3], bin_xy[24][2], sum_x, sum_y, sum_w;
    if (tc_sample_bins(image, bin_w, bin_rgb, bin_xy, &sum_x, &sum_y, &sum_w) != 0) return -1;
    if (tc_accents_from_bins(bin_w, bin_rgb, bin_xy, &out->accent, NULL, 1) <= 0) return -1;

    double ah = 0;
    if (sum_w <= 0) {
        double as, av;
        tc_rgb_to_hsv(out->accent, &ah, &as, &av);
    }
    double mh = sum_w > 0 ? atan2(sum_y, sum_x) * 180.0 / M_PI : ah;
    if (mh < 0) mh += 360.0;
    double tint = sum_w > 0 ? tc_clamp(hypot(sum_x, sum_y) / sum_w, 0.0, 1.0) : 0.0;
    tc_hsv_to_rgb(mh, 0.15 + 0.20 * tint, 0.09, out->surface);
    tc_hsv_to_rgb(mh, 0.15 + 0.20 * tint, 0.055, out->canvas);
    for (int k = 0; k < 3; k++) out->border[k] = out->surface[k] * 0.55 + out->accent[k] * 0.20 + 0.06;
    return 0;
}

static void tc_print_hex(FILE *f, const double c[3]) {
    fprintf(f, "#%02x%02x%02x",
            (int)(tc_clamp(c[0], 0, 1) * 255 + 0.5),
            (int)(tc_clamp(c[1], 0, 1) * 255 + 0.5),
            (int)(tc_clamp(c[2], 0, 1) * 255 + 0.5));
}

static void theme_colors_print(const ThemeColors *t, FILE *f) {
    const char *names[4] = {"BORDER_ACTIVE", "SURFACE_COLOR", "CANVAS_COLOR", "BORDER_INACTIVE"};
    const double *cols[4] = {t->accent, t->surface, t->canvas, t->border};
    for (int i = 0; i < 4; i++) {
        fprintf(f, "%s=\"", names[i]);
        tc_print_hex(f, cols[i]);
        fputs("\"\n", f);
    }
}

static void theme_accents_print(double accents[][3], double pos[][2], int count, FILE *f) {
    for (int i = 0; i < count; i++) {
        tc_print_hex(f, accents[i]);
        fprintf(f, " %.3f %.3f\n", tc_clamp(pos[i][0], 0.0, 1.0), tc_clamp(pos[i][1], 0.0, 1.0));
    }
}

#endif
