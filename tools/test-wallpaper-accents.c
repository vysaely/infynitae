/* Unit tests for theme_accents_from / theme_colors_from in live/wallpaper/theme-colors.h. */
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

#include "../live/wallpaper/theme-colors.h"

static GdkPixbuf *make_pixbuf(int w, int h) {
    GdkPixbuf *pb = gdk_pixbuf_new(GDK_COLORSPACE_RGB, FALSE, 8, w, h);
    assert(pb != NULL);
    return pb;
}

static void put_rgb(GdkPixbuf *pb, int x, int y, guchar r, guchar g, guchar b) {
    int rs = gdk_pixbuf_get_rowstride(pb);
    int nch = gdk_pixbuf_get_n_channels(pb);
    guchar *p = gdk_pixbuf_get_pixels(pb) + y * rs + x * nch;
    p[0] = r; p[1] = g; p[2] = b;
}

static void fill_hsv_cols(GdkPixbuf *pb, int x0, int x1, double h, double s, double v) {
    double rgb[3];
    tc_hsv_to_rgb(h, s, v, rgb);
    guchar r = (guchar)(tc_clamp(rgb[0], 0, 1) * 255 + 0.5);
    guchar g = (guchar)(tc_clamp(rgb[1], 0, 1) * 255 + 0.5);
    guchar b = (guchar)(tc_clamp(rgb[2], 0, 1) * 255 + 0.5);
    int hpx = gdk_pixbuf_get_height(pb);
    for (int y = 0; y < hpx; y++)
        for (int x = x0; x < x1; x++)
            put_rgb(pb, x, y, r, g, b);
}

static void fill_rgb_rect(GdkPixbuf *pb, int x0, int y0, int x1, int y1, guchar r, guchar g, guchar b) {
    for (int y = y0; y < y1; y++)
        for (int x = x0; x < x1; x++)
            put_rgb(pb, x, y, r, g, b);
}

static double hue_diff(double a, double b) {
    double d = fabs(a - b);
    return d > 180.0 ? 360.0 - d : d;
}

static void format_hex(const double c[3], char buf[8]) {
    snprintf(buf, 8, "#%02x%02x%02x",
             (int)(tc_clamp(c[0], 0, 1) * 255 + 0.5),
             (int)(tc_clamp(c[1], 0, 1) * 255 + 0.5),
             (int)(tc_clamp(c[2], 0, 1) * 255 + 0.5));
}

static void check_common(GdkPixbuf *pb, double out[][3], int n, int expect_clamped_sv) {
    ThemeColors tc;
    assert(theme_colors_from(pb, &tc) == 0);
    assert(n >= 1 && n <= 6);

    char hex0[8], hex_tc[8];
    format_hex(out[0], hex0);
    format_hex(tc.accent, hex_tc);
    assert(strcmp(hex0, hex_tc) == 0);
    for (int k = 0; k < 3; k++)
        assert(fabs(out[0][k] - tc.accent[k]) < 1e-9);

    for (int i = 0; i < n; i++) {
        double h, s, v;
        tc_rgb_to_hsv(out[i], &h, &s, &v);
        if (expect_clamped_sv) {
            assert(s >= 0.45 - 1e-6 && s <= 0.85 + 1e-6);
            assert(v >= 0.80 - 1e-6 && v <= 0.95 + 1e-6);
        }
    }
}

static void test_invalid_args(void) {
    GdkPixbuf *pb = make_pixbuf(4, 4);
    double out[6][3];
    assert(theme_accents_from(NULL, out, 6) == -1);
    assert(theme_accents_from(pb, NULL, 6) == -1);
    assert(theme_accents_from(pb, out, 0) == -1);
    assert(theme_accents_from(pb, out, -2) == -1);
    g_object_unref(pb);
}

static void test_one_hue_and_clamps(void) {
    /* dark low-saturation green: must clamp up to S=0.45, V=0.80 */
    GdkPixbuf *dark = make_pixbuf(24, 16);
    fill_hsv_cols(dark, 0, 24, 120.0, 0.30, 0.30);
    double out[6][3];
    int n = theme_accents_from(dark, out, 6);
    assert(n == 1);
    check_common(dark, out, n, 1);
    double h, s, v;
    tc_rgb_to_hsv(out[0], &h, &s, &v);
    assert(hue_diff(h, 120.0) < 2.0);
    assert(fabs(s - 0.45) < 0.02);
    assert(fabs(v - 0.80) < 0.02);
    g_object_unref(dark);

    /* neon pure red: must clamp down to S=0.85, V=0.95 */
    GdkPixbuf *neon = make_pixbuf(24, 16);
    fill_hsv_cols(neon, 0, 24, 0.0, 1.0, 1.0);
    n = theme_accents_from(neon, out, 6);
    assert(n == 1);
    check_common(neon, out, n, 1);
    tc_rgb_to_hsv(out[0], &h, &s, &v);
    assert(hue_diff(h, 0.0) < 2.0);
    assert(fabs(s - 0.85) < 0.02);
    assert(fabs(v - 0.95) < 0.02);
    g_object_unref(neon);
}

static void test_three_hues(void) {
    /* weighted: hue 10 (55%), hue 220 (30%), hue 125 (15%) -> exact best-first order */
    GdkPixbuf *pb = make_pixbuf(60, 20);
    fill_hsv_cols(pb, 0, 33, 10.0, 0.80, 0.90);
    fill_hsv_cols(pb, 33, 51, 220.0, 0.80, 0.90);
    fill_hsv_cols(pb, 51, 60, 125.0, 0.80, 0.90);

    double out[6][3];
    int n = theme_accents_from(pb, out, 6);
    assert(n == 3);
    check_common(pb, out, n, 1);

    double h[3], s, v;
    for (int i = 0; i < 3; i++)
        tc_rgb_to_hsv(out[i], &h[i], &s, &v);
    assert(hue_diff(h[0], 10.0) < 5.0);
    assert(hue_diff(h[1], 220.0) < 5.0);
    assert(hue_diff(h[2], 125.0) < 5.0);

    /* also verify print helpers match candidate 0 */
    ThemeColors tc;
    assert(theme_colors_from(pb, &tc) == 0);
    FILE *fa = tmpfile(), *fc = tmpfile();
    assert(fa != NULL && fc != NULL);
    theme_accents_print(out, n, fa);
    theme_colors_print(&tc, fc);
    rewind(fa);
    rewind(fc);
    char line_a[64] = {0}, line_c[64] = {0}, expected_c[80];
    assert(fgets(line_a, sizeof(line_a), fa) != NULL);
    assert(fgets(line_c, sizeof(line_c), fc) != NULL);
    line_a[strcspn(line_a, "\n")] = '\0';
    line_c[strcspn(line_c, "\n")] = '\0';
    snprintf(expected_c, sizeof(expected_c), "BORDER_ACTIVE=\"%s\"", line_a);
    assert(strcmp(line_c, expected_c) == 0);
    fclose(fa);
    fclose(fc);
    g_object_unref(pb);
}

static void test_grey_image(void) {
    GdkPixbuf *pb = make_pixbuf(24, 16);
    fill_rgb_rect(pb, 0, 0, 12, 16, 128, 128, 128);
    fill_rgb_rect(pb, 12, 0, 24, 16, 30, 30, 30);

    double out[6][3];
    int n = theme_accents_from(pb, out, 6);
    assert(n == 1);
    check_common(pb, out, n, 0);

    char hex[8];
    format_hex(out[0], hex);
    assert(strcmp(hex, "#7aa2f7") == 0);
    g_object_unref(pb);
}

static void test_bin_spacing_and_max(void) {
    /* bins 1 (22 deg, 60%) and 2 (37 deg, 40%) are only 1 bin apart -> collapse to 1 */
    GdkPixbuf *close_pb = make_pixbuf(50, 16);
    fill_hsv_cols(close_pb, 0, 30, 22.0, 0.80, 0.90);
    fill_hsv_cols(close_pb, 30, 50, 37.0, 0.80, 0.90);
    double out[10][3];
    int n = theme_accents_from(close_pb, out, 10);
    assert(n == 1);
    check_common(close_pb, out, n, 1);
    g_object_unref(close_pb);

    /* 8 hues 45 degrees apart (bin distance >= 3) -> capped at 6 even when max=10 */
    GdkPixbuf *many = make_pixbuf(80, 16);
    for (int i = 0; i < 8; i++)
        fill_hsv_cols(many, i * 10, (i + 1) * 10, 7.5 + i * 45.0, 0.80, 0.90);
    n = theme_accents_from(many, out, 10);
    assert(n == 6);
    check_common(many, out, n, 1);

    double hues[6], s, v;
    for (int i = 0; i < 6; i++)
        tc_rgb_to_hsv(out[i], &hues[i], &s, &v);
    for (int i = 0; i < 6; i++)
        for (int j = i + 1; j < 6; j++)
            assert(hue_diff(hues[i], hues[j]) >= 28.0);
    g_object_unref(many);
}

static void test_noisy_image(void) {
    /* 1) Two main hues plus <4% vivid noise speckles and grey noise -> speckles dropped */
    GdkPixbuf *pb = make_pixbuf(96, 32);
    fill_hsv_cols(pb, 0, 54, 185.0, 0.75, 0.90);
    fill_hsv_cols(pb, 54, 92, 35.0, 0.75, 0.90);
    fill_hsv_cols(pb, 92, 94, 300.0, 0.75, 0.90); /* ~2% weight */
    fill_hsv_cols(pb, 94, 96, 260.0, 0.75, 0.90); /* ~2% weight */
    fill_rgb_rect(pb, 0, 0, 96, 2, 110, 110, 110); /* grey noise strip */

    double out[6][3];
    int n = theme_accents_from(pb, out, 6);
    assert(n == 2);
    check_common(pb, out, n, 1);
    double h0, h1, s, v;
    tc_rgb_to_hsv(out[0], &h0, &s, &v);
    tc_rgb_to_hsv(out[1], &h1, &s, &v);
    assert(hue_diff(h0, 185.0) < 5.0);
    assert(hue_diff(h1, 35.0) < 5.0);
    g_object_unref(pb);

    /* 2) Per-pixel pseudo-random texture across 9 distinct hues + grey speckles */
    GdkPixbuf *rand_pb = make_pixbuf(96, 32);
    unsigned rng = 0x12345678u;
    for (int y = 0; y < 32; y++) {
        for (int x = 0; x < 96; x++) {
            rng = rng * 1664525u + 1013904223u;
            unsigned pick = (rng >> 24) % 10u;
            double rgb[3];
            if (pick == 9) {
                rgb[0] = rgb[1] = rgb[2] = 0.5;
            } else {
                /* 9 hues 40 degrees apart, centered in bins so min hue spacing > 30 deg */
                double h = pick * 40.0 + 7.5;
                double s = 0.55 + 0.35 * (((rng >> 16) & 255u) / 255.0);
                double v = 0.65 + 0.30 * (((rng >> 8) & 255u) / 255.0);
                tc_hsv_to_rgb(h, s, v, rgb);
            }
            put_rgb(rand_pb, x, y,
                    (guchar)(rgb[0] * 255 + 0.5),
                    (guchar)(rgb[1] * 255 + 0.5),
                    (guchar)(rgb[2] * 255 + 0.5));
        }
    }
    n = theme_accents_from(rand_pb, out, 6);
    assert(n == 6);
    check_common(rand_pb, out, n, 1);
    g_object_unref(rand_pb);
}

int main(void) {
    test_invalid_args();
    test_one_hue_and_clamps();
    test_three_hues();
    test_grey_image();
    test_bin_spacing_and_max();
    test_noisy_image();
    printf("test-wallpaper-accents.c: all assertions passed\n");
    return 0;
}
