/*
 * Infynitae Native Wallpaper & Live Background Renderer
 * Part of Infynitae (FOUNDATION_ARCHITECTURE C1.1, zwlr_layer_shell_v1)
 *
 * Lightweight, zero-overhead standalone wallpaper manager written in pure C.
 * Features:
 *   - Direct Wayland layer-shell protocol (ZWLR_LAYER_SHELL_V1_LAYER_BACKGROUND)
 *   - Auto-scales across all connected monitors (fill, fit, stretch, center, tile)
 *   - Supports all standard image formats (PNG, JPG, WebP, SVG, BMP, GIF) via GdkPixbuf
 *   - Seamless video background passthrough to hardware mpvpaper
 *   - Zero extra idle memory: sleeps in event loop after surface commit
 *   - Hot swappable: clean SIGTERM instance handover for live config reloading
 */

#define _GNU_SOURCE
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <dirent.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <errno.h>
#include <spawn.h>

extern char **environ;

#include <wayland-client.h>
#include <cairo/cairo.h>
#include <gdk-pixbuf/gdk-pixbuf.h>

#include "wlr-layer-shell-protocol.h"
#include "viewporter-protocol.h"
#include <math.h>
#include <poll.h>
#include <time.h>
#include <sys/socket.h>
#include <sys/un.h>
#include "lock-cache.h"
#include "theme-colors.h"

static void sig_handler(int sig) {
    (void)sig;
    _exit(0);
}

struct wallpaper_output {
    struct wl_output *output;
    uint32_t name;
    char connector[64];
    int32_t width;
    int32_t height;
    int32_t scale;
    struct wl_surface *surface;
    struct zwlr_layer_surface_v1 *layer_surface;
    struct wl_buffer *buffer;
    void *shm_data;
    size_t shm_size;
    /* parallax: the image drawn `margin` px larger on every side, a screen-sized window of it shown
     * (wp_viewport); the view's centre as the compositor last said, and the point the window
     * eases back to after a move */
    struct wp_viewport *viewport;
    int margin;
    bool have_cam;
    double cam_x, cam_y, rest_x, rest_y, zoom;
    struct wl_callback *frame_cb;
    int64_t last_ns;
    struct wallpaper_app *app;
    struct wallpaper_output *next;
};

struct wallpaper_app {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct zwlr_layer_shell_v1 *layer_shell;
    struct wallpaper_output *outputs;
    /* the decoded image, only while a screen still needs drawing: a 4961x3508 wallpaper held 70 MB
     * all session after the screens were drawn (2026-11-06); it is decoded again from `path` when a
     * screen is added or changes size */
    GdkPixbuf *pixbuf;
    char path[4096];
    char mode[32];
    /* --overlay: above every window (a frozen screenshot shown while a region is chosen); only on
     * the output named by --output when one is */
    int overlay;
    char only_output[64];
    /* WALLPAPER_PARALLAX: 0 off, 1 subtle, 2 strong; and the compositor's events, when on */
    int parallax;
    struct wp_viewporter *viewporter;
    int ipc_fd;
    char ipc_buf[65536];
    size_t ipc_len;
};

static int create_shm_file(off_t size) {
    char template[] = "/tmp/infynitae-wall-XXXXXX";
    int fd = mkstemp(template);
    if (fd < 0) return -1;
    unlink(template);
    if (ftruncate(fd, size) < 0) {
        close(fd);
        return -1;
    }
    return fd;
}

static void draw_pixbuf_to_cairo(cairo_t *cr, GdkPixbuf *pb, int dx, int dy) {
    int w = gdk_pixbuf_get_width(pb);
    int h = gdk_pixbuf_get_height(pb);
    if (w <= 0 || h <= 0) return;

    if (w > 32767 || h > 32767) {
        double factor = 32767.0 / (w > h ? w : h);
        int nw = (int)(w * factor);
        int nh = (int)(h * factor);
        if (nw < 1) nw = 1;
        if (nh < 1) nh = 1;
        GdkPixbuf *downscaled = gdk_pixbuf_scale_simple(pb, nw, nh, GDK_INTERP_BILINEAR);
        if (downscaled) {
            draw_pixbuf_to_cairo(cr, downscaled, dx, dy);
            g_object_unref(downscaled);
        }
        return;
    }

    int rowstride = gdk_pixbuf_get_rowstride(pb);
    int n_channels = gdk_pixbuf_get_n_channels(pb);
    const guchar *src_pixels = gdk_pixbuf_read_pixels(pb);

    cairo_surface_t *surf = cairo_image_surface_create(
        n_channels == 4 ? CAIRO_FORMAT_ARGB32 : CAIRO_FORMAT_RGB24, w, h);
    if (!surf || cairo_surface_status(surf) != CAIRO_STATUS_SUCCESS) {
        if (surf) cairo_surface_destroy(surf);
        return;
    }
    int cairo_stride = cairo_image_surface_get_stride(surf);
    uint32_t *dst = (uint32_t *)cairo_image_surface_get_data(surf);
    if (!dst) {
        cairo_surface_destroy(surf);
        return;
    }

    cairo_surface_flush(surf);
    for (int y = 0; y < h; y++) {
        const guchar *s = src_pixels + y * rowstride;
        uint32_t *d = dst + y * (cairo_stride / 4);
        for (int x = 0; x < w; x++) {
            if (n_channels == 4) {
                uint8_t r = s[0];
                uint8_t g = s[1];
                uint8_t b = s[2];
                uint8_t a = s[3];
                uint32_t pr = (r * a + 127) / 255;
                uint32_t pg = (g * a + 127) / 255;
                uint32_t pb_val = (b * a + 127) / 255;
                d[x] = ((uint32_t)a << 24) | (pr << 16) | (pg << 8) | pb_val;
                s += 4;
            } else {
                uint8_t r = s[0];
                uint8_t g = s[1];
                uint8_t b = s[2];
                d[x] = (0xFF << 24) | ((uint32_t)r << 16) | ((uint32_t)g << 8) | b;
                s += 3;
            }
        }
    }
    cairo_surface_mark_dirty(surf);
    cairo_set_source_surface(cr, surf, dx, dy);
    cairo_paint(cr);
    cairo_surface_destroy(surf);
}

/* The image at `path`, scaled down when a side passes cairo's limit; NULL when it cannot be read. */
static GdkPixbuf *load_image(const char *path) {
    GError *err = NULL;
    GdkPixbuf *pixbuf = gdk_pixbuf_new_from_file(path, &err);
    if (!pixbuf) {
        fprintf(stderr, "[infynitae-wallpaper] Failed to load image '%s': %s\n", path, err ? err->message : "unknown error");
        if (err) g_error_free(err);
        return NULL;
    }
    int raw_w = gdk_pixbuf_get_width(pixbuf);
    int raw_h = gdk_pixbuf_get_height(pixbuf);
    if (raw_w > 32767 || raw_h > 32767) {
        double factor = 32767.0 / (raw_w > raw_h ? raw_w : raw_h);
        int nw = (int)(raw_w * factor);
        int nh = (int)(raw_h * factor);
        if (nw < 1) nw = 1;
        if (nh < 1) nh = 1;
        GdkPixbuf *downscaled = gdk_pixbuf_scale_simple(pixbuf, nw, nh, GDK_INTERP_BILINEAR);
        if (downscaled) {
            g_object_unref(pixbuf);
            pixbuf = downscaled;
        }
    }
    return pixbuf;
}

/* Once every screen shows its picture, the decoded image goes. */
static void release_image_when_drawn(struct wallpaper_app *app) {
    if (!app->pixbuf || !app->path[0]) return;
    for (struct wallpaper_output *o = app->outputs; o; o = o->next) {
        if (o->layer_surface && !o->buffer) return;
    }
    g_object_unref(app->pixbuf);
    app->pixbuf = NULL;
}

/* ---- parallax (owner, 2026-11-08): the wallpaper behind the canvas, moving less than it ----
 * The canvas has no end and the image has, so the image does not follow the view: it shifts while
 * the view moves (as far things seem to) and eases back to the middle in about a second after.
 * The shift: the view's distance from where the image rests, scaled by the depth (8%, strong 15%
 * of the movement) and kept inside the margin smoothly (tanh). Nothing is redrawn: only the window
 * into the larger image moves (wp_viewport), once a frame while it moves. */
static int64_t mono_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * 1000000000 + ts.tv_nsec;
}

static void parallax_place(struct wallpaper_output *out) {
    if (!out->viewport || out->margin <= 0) return;
    double depth = out->app->parallax == 2 ? 0.15 : 0.08, m = out->margin;
    double z = out->zoom > 0.0 ? out->zoom : 1.0;
    double dx = out->have_cam ? (out->cam_x - out->rest_x) * z : 0.0;
    double dy = out->have_cam ? (out->cam_y - out->rest_y) * z : 0.0;
    double sx = m + m * tanh(dx * depth / m), sy = m + m * tanh(dy * depth / m);
    wp_viewport_set_source(out->viewport, wl_fixed_from_double(sx), wl_fixed_from_double(sy),
                           wl_fixed_from_int(out->width), wl_fixed_from_int(out->height));
}

static void parallax_frame(void *data, struct wl_callback *cb, uint32_t t);
static const struct wl_callback_listener parallax_frame_listener = {.done = parallax_frame};

/* One step of the easing back, then the next frame's while the image is still off its rest. */
static void parallax_step(struct wallpaper_output *out) {
    if (!out->viewport || !out->surface || out->margin <= 0 || out->frame_cb) return;
    int64_t now = mono_ns();
    double dt = out->last_ns ? (now - out->last_ns) / 1e9 : 0.0;
    if (dt > 0.1) dt = 0.1;
    out->last_ns = now;
    double k = 1.0 - exp(-dt / 0.5);
    out->rest_x += (out->cam_x - out->rest_x) * k;
    out->rest_y += (out->cam_y - out->rest_y) * k;
    parallax_place(out);
    double off = fabs(out->cam_x - out->rest_x) + fabs(out->cam_y - out->rest_y);
    if (off > 0.25) {
        out->frame_cb = wl_surface_frame(out->surface);
        wl_callback_add_listener(out->frame_cb, &parallax_frame_listener, out);
    } else {
        out->rest_x = out->cam_x;
        out->rest_y = out->cam_y;
        out->last_ns = 0;
    }
    wl_surface_commit(out->surface);
}

static void parallax_frame(void *data, struct wl_callback *cb, uint32_t t) {
    (void)t;
    struct wallpaper_output *out = data;
    wl_callback_destroy(cb);
    out->frame_cb = NULL;
    parallax_step(out);
}

/* The compositor's shell socket: subscribed, its output events give each screen's view. */
static void parallax_connect(struct wallpaper_app *app) {
    app->ipc_fd = -1;
    const char *rt = getenv("XDG_RUNTIME_DIR");
    if (!app->parallax || !app->viewporter || !rt) return;
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return;
    struct sockaddr_un sa = {.sun_family = AF_UNIX};
    snprintf(sa.sun_path, sizeof(sa.sun_path), "%s/infynitae/shell.sock", rt);
    const char *req = "{\"id\":1,\"cmd\":\"subscribe\"}\n";
    if (connect(fd, (struct sockaddr *)&sa, sizeof(sa)) < 0 || write(fd, req, strlen(req)) < 0) {
        close(fd);
        return;
    }
    fcntl(fd, F_SETFL, fcntl(fd, F_GETFL) | O_NONBLOCK);
    app->ipc_fd = fd;
    app->ipc_len = 0;
}

/* A line of the socket: every "name":"<connector>" with the "center":[x,y] and "zoom" after it. */
static void parallax_line(struct wallpaper_app *app, char *line) {
    for (char *p = strstr(line, "\"name\":\""); p; p = strstr(p + 1, "\"name\":\"")) {
        char conn[64] = "";
        const char *n = p + 8;
        size_t k = 0;
        while (n[k] && n[k] != '"' && k + 1 < sizeof(conn)) { conn[k] = n[k]; k++; }
        conn[k] = '\0';
        char *next = strstr(p + 1, "\"name\":\"");
        char *c = strstr(p, "\"center\":[");
        if (!c || (next && c > next)) continue;
        double x, y, z = 1.0;
        if (sscanf(c + 10, "%lf,%lf", &x, &y) != 2) continue;
        char *zp = strstr(c, "\"zoom\":");
        if (zp && (!next || zp < next)) z = atof(zp + 7);
        for (struct wallpaper_output *o = app->outputs; o; o = o->next) {
            if (strcmp(o->connector, conn) != 0) continue;
            if (!o->have_cam) {
                o->rest_x = x;
                o->rest_y = y;
                o->have_cam = true;
            }
            o->cam_x = x;
            o->cam_y = y;
            o->zoom = z;
            parallax_step(o);
        }
    }
}

static void parallax_read(struct wallpaper_app *app) {
    for (;;) {
        if (app->ipc_len >= sizeof(app->ipc_buf) - 1) app->ipc_len = 0;  /* a line too long: dropped */
        size_t room = sizeof(app->ipc_buf) - 1 - app->ipc_len;
        ssize_t got = read(app->ipc_fd, app->ipc_buf + app->ipc_len, room);
        if (got == 0 || (got < 0 && errno != EAGAIN && errno != EINTR)) {
            close(app->ipc_fd);  /* the compositor went: the image rests where it is */
            app->ipc_fd = -1;
            return;
        }
        if (got < 0) return;
        app->ipc_len += (size_t)got;
        app->ipc_buf[app->ipc_len] = '\0';
        char *start = app->ipc_buf, *nl;
        while ((nl = strchr(start, '\n'))) {
            *nl = '\0';
            parallax_line(app, start);
            start = nl + 1;
        }
        size_t rest = app->ipc_len - (size_t)(start - app->ipc_buf);
        memmove(app->ipc_buf, start, rest);
        app->ipc_len = rest;
    }
}

static void render_output(struct wallpaper_output *out) {
    if (out->width <= 0 || out->height <= 0) return;
    if (!out->app->pixbuf && out->app->path[0]) out->app->pixbuf = load_image(out->app->path);
    if (!out->app->pixbuf) return;

    /* parallax: drawn larger by a margin on every side (3% of the width, 6% when strong) */
    out->margin = (out->app->parallax && out->app->viewporter && !out->app->overlay)
                      ? (int)lround(out->width * (out->app->parallax == 2 ? 0.06 : 0.03)) : 0;
    int32_t w = out->width + 2 * out->margin;
    int32_t h = out->height + 2 * out->margin;
    int stride = cairo_format_stride_for_width(CAIRO_FORMAT_ARGB32, w);
    size_t size = (size_t)stride * h;

    if (out->buffer) {
        wl_buffer_destroy(out->buffer);
        out->buffer = NULL;
    }
    if (out->shm_data) {
        munmap(out->shm_data, out->shm_size);
        out->shm_data = NULL;
    }

    int fd = create_shm_file(size);
    if (fd < 0) return;

    void *data = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (data == MAP_FAILED) {
        close(fd);
        return;
    }

    struct wl_shm_pool *pool = wl_shm_create_pool(out->app->shm, fd, size);
    out->buffer = wl_shm_pool_create_buffer(pool, 0, w, h, stride, WL_SHM_FORMAT_ARGB8888);
    wl_shm_pool_destroy(pool);
    close(fd);

    out->shm_data = data;
    out->shm_size = size;

    cairo_surface_t *cairo_surf = cairo_image_surface_create_for_data(
        (unsigned char *)data, CAIRO_FORMAT_ARGB32, w, h, stride);
    if (!cairo_surf || cairo_surface_status(cairo_surf) != CAIRO_STATUS_SUCCESS) {
        if (cairo_surf) cairo_surface_destroy(cairo_surf);
        return;
    }
    cairo_t *cr = cairo_create(cairo_surf);
    if (!cr || cairo_status(cr) != CAIRO_STATUS_SUCCESS) {
        if (cr) cairo_destroy(cr);
        cairo_surface_destroy(cairo_surf);
        return;
    }

    // Deep black base canvas
    cairo_set_source_rgb(cr, 0.0, 0.0, 0.0);
    cairo_paint(cr);

    int img_w = gdk_pixbuf_get_width(out->app->pixbuf);
    int img_h = gdk_pixbuf_get_height(out->app->pixbuf);

    if (strcmp(out->app->mode, "stretch") == 0) {
        GdkPixbuf *scaled = gdk_pixbuf_scale_simple(out->app->pixbuf, w, h, GDK_INTERP_BILINEAR);
        if (scaled) {
            draw_pixbuf_to_cairo(cr, scaled, 0, 0);
            g_object_unref(scaled);
        }
    } else if (strcmp(out->app->mode, "fit") == 0) {
        double scale_w = (double)w / img_w;
        double scale_h = (double)h / img_h;
        double s = (scale_w < scale_h) ? scale_w : scale_h;
        int target_w = (int)(img_w * s);
        int target_h = (int)(img_h * s);
        int dx = (w - target_w) / 2;
        int dy = (h - target_h) / 2;
        GdkPixbuf *scaled = gdk_pixbuf_scale_simple(out->app->pixbuf, target_w > 0 ? target_w : 1,
                                                   target_h > 0 ? target_h : 1, GDK_INTERP_BILINEAR);
        if (scaled) {
            draw_pixbuf_to_cairo(cr, scaled, dx, dy);
            g_object_unref(scaled);
        }
    } else if (strcmp(out->app->mode, "center") == 0) {
        int dx = (w - img_w) / 2;
        int dy = (h - img_h) / 2;
        draw_pixbuf_to_cairo(cr, out->app->pixbuf, dx, dy);
    } else if (strcmp(out->app->mode, "tile") == 0) {
        for (int y = 0; y < h; y += img_h) {
            for (int x = 0; x < w; x += img_w) {
                draw_pixbuf_to_cairo(cr, out->app->pixbuf, x, y);
            }
        }
    } else { // default: "fill"
        double scale_w = (double)w / img_w;
        double scale_h = (double)h / img_h;
        double s = (scale_w > scale_h) ? scale_w : scale_h;
        int target_w = (int)(img_w * s);
        int target_h = (int)(img_h * s);
        if (target_w > 32767 || target_h > 32767) {
            double factor = 32767.0 / (target_w > target_h ? target_w : target_h);
            target_w = (int)(target_w * factor);
            target_h = (int)(target_h * factor);
        }
        int dx = (w - target_w) / 2;
        int dy = (h - target_h) / 2;
        GdkPixbuf *scaled = gdk_pixbuf_scale_simple(out->app->pixbuf, target_w > 0 ? target_w : 1,
                                                   target_h > 0 ? target_h : 1, GDK_INTERP_BILINEAR);
        if (scaled) {
            draw_pixbuf_to_cairo(cr, scaled, dx, dy);
            g_object_unref(scaled);
        }
    }

    cairo_destroy(cr);
    cairo_surface_destroy(cairo_surf);

    wl_surface_attach(out->surface, out->buffer, 0, 0);
    wl_surface_damage_buffer(out->surface, 0, 0, w, h);
    if (out->margin > 0) {
        if (!out->viewport) out->viewport = wp_viewporter_get_viewport(out->app->viewporter, out->surface);
        wp_viewport_set_destination(out->viewport, out->width, out->height);
        parallax_place(out);
    }
    wl_surface_commit(out->surface);
}

static void layer_surface_configure(void *data, struct zwlr_layer_surface_v1 *layer_surface,
                                    uint32_t serial, uint32_t width, uint32_t height) {
    struct wallpaper_output *out = data;
    zwlr_layer_surface_v1_ack_configure(layer_surface, serial);

    if (width > 0) out->width = width;
    if (height > 0) out->height = height;

    render_output(out);
    release_image_when_drawn(out->app);
}

static void layer_surface_closed(void *data, struct zwlr_layer_surface_v1 *layer_surface) {
    (void)data;
    (void)layer_surface;
}

static const struct zwlr_layer_surface_v1_listener layer_surface_listener = {
    .configure = layer_surface_configure,
    .closed = layer_surface_closed,
};

static void setup_layer_surface(struct wallpaper_output *out) {
    if (!out->app->layer_shell || !out->app->compositor) return;
    if (out->app->only_output[0] && strcmp(out->connector, out->app->only_output) != 0) return;

    out->surface = wl_compositor_create_surface(out->app->compositor);
    out->layer_surface = zwlr_layer_shell_v1_get_layer_surface(
        out->app->layer_shell,
        out->surface,
        out->output,
        out->app->overlay ? ZWLR_LAYER_SHELL_V1_LAYER_OVERLAY : ZWLR_LAYER_SHELL_V1_LAYER_BACKGROUND,
        out->app->overlay ? "infynitae-freeze" : "wallpaper"
    );

    zwlr_layer_surface_v1_set_anchor(
        out->layer_surface,
        ZWLR_LAYER_SURFACE_V1_ANCHOR_TOP |
        ZWLR_LAYER_SURFACE_V1_ANCHOR_BOTTOM |
        ZWLR_LAYER_SURFACE_V1_ANCHOR_LEFT |
        ZWLR_LAYER_SURFACE_V1_ANCHOR_RIGHT
    );
    zwlr_layer_surface_v1_set_exclusive_zone(out->layer_surface, -1);
    zwlr_layer_surface_v1_add_listener(out->layer_surface, &layer_surface_listener, out);

    /* Empty input region ensures wallpaper never intercepts mouse input or pointer events */
    struct wl_region *empty_region = wl_compositor_create_region(out->app->compositor);
    wl_surface_set_input_region(out->surface, empty_region);
    wl_region_destroy(empty_region);

    wl_surface_commit(out->surface);
}

static void output_geometry(void *data, struct wl_output *wl_output,
                            int32_t x, int32_t y, int32_t physical_width, int32_t physical_height,
                            int32_t subpixel, const char *make, const char *model, int32_t transform) {
    (void)data; (void)wl_output; (void)x; (void)y; (void)physical_width; (void)physical_height;
    (void)subpixel; (void)make; (void)model; (void)transform;
}

static void output_mode(void *data, struct wl_output *wl_output,
                        uint32_t flags, int32_t width, int32_t height, int32_t refresh) {
    (void)wl_output; (void)flags; (void)refresh;
    struct wallpaper_output *out = data;
    if (out->width == 0 && width > 0) out->width = width;
    if (out->height == 0 && height > 0) out->height = height;
}

static void output_done(void *data, struct wl_output *wl_output) {
    (void)wl_output;
    struct wallpaper_output *out = data;
    if (!out->layer_surface) {
        setup_layer_surface(out);
    }
}

static void output_scale(void *data, struct wl_output *wl_output, int32_t factor) {
    (void)wl_output;
    struct wallpaper_output *out = data;
    out->scale = factor > 0 ? factor : 1;
}

static void output_name(void *data, struct wl_output *wl_output, const char *name) {
    (void)wl_output;
    struct wallpaper_output *out = data;
    snprintf(out->connector, sizeof(out->connector), "%s", name);
}

static void output_description(void *data, struct wl_output *wl_output, const char *description) {
    (void)data; (void)wl_output; (void)description;
}

static const struct wl_output_listener output_listener = {
    .geometry = output_geometry,
    .mode = output_mode,
    .done = output_done,
    .scale = output_scale,
    .name = output_name,
    .description = output_description,
};

static void registry_global(void *data, struct wl_registry *registry,
                            uint32_t name, const char *interface, uint32_t version) {
    (void)version;
    struct wallpaper_app *app = data;

    if (strcmp(interface, wl_compositor_interface.name) == 0) {
        app->compositor = wl_registry_bind(registry, name, &wl_compositor_interface, 4);
    } else if (strcmp(interface, wl_shm_interface.name) == 0) {
        app->shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    } else if (strcmp(interface, zwlr_layer_shell_v1_interface.name) == 0) {
        app->layer_shell = wl_registry_bind(registry, name, &zwlr_layer_shell_v1_interface, 1);
    } else if (strcmp(interface, wp_viewporter_interface.name) == 0) {
        app->viewporter = wl_registry_bind(registry, name, &wp_viewporter_interface, 1);
    } else if (strcmp(interface, wl_output_interface.name) == 0) {
        struct wallpaper_output *out = calloc(1, sizeof(struct wallpaper_output));
        out->name = name;
        out->output = wl_registry_bind(registry, name, &wl_output_interface, 4);
        out->scale = 1;
        out->app = app;
        wl_output_add_listener(out->output, &output_listener, out);
        out->next = app->outputs;
        app->outputs = out;
    }
}

static void registry_global_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)registry;
    struct wallpaper_app *app = data;
    struct wallpaper_output **curr = &app->outputs;
    while (*curr) {
        if ((*curr)->name == name) {
            struct wallpaper_output *to_free = *curr;
            *curr = (*curr)->next;
            if (to_free->layer_surface) zwlr_layer_surface_v1_destroy(to_free->layer_surface);
            if (to_free->surface) wl_surface_destroy(to_free->surface);
            if (to_free->buffer) wl_buffer_destroy(to_free->buffer);
            if (to_free->shm_data) munmap(to_free->shm_data, to_free->shm_size);
            if (to_free->output) wl_output_destroy(to_free->output);
            free(to_free);
            break;
        }
        curr = &(*curr)->next;
    }
}

static const struct wl_registry_listener registry_listener = {
    .global = registry_global,
    .global_remove = registry_global_remove,
};

/* The pid file lives only in the user's own runtime directory: never a shared one like /tmp,
 * where another user could plant it. Without XDG_RUNTIME_DIR there is none (NULL). */
static char *get_pid_file_path(void) {
    const char *runtime_dir = getenv("XDG_RUNTIME_DIR");
    if (!runtime_dir || runtime_dir[0] != '/') return NULL;
    static char path[512];
    snprintf(path, sizeof(path), "%s/infynitae-wallpaper.pid", runtime_dir);
    return path;
}

static int proc_matches_wayland_display(int pid, const char *target_display) {
    if (!target_display || !*target_display) return 0;
    char env_path[256];
    snprintf(env_path, sizeof(env_path), "/proc/%d/environ", pid);
    int fd = open(env_path, O_RDONLY);
    if (fd < 0) return 0;

    char buf[8192];
    ssize_t n = read(fd, buf, sizeof(buf) - 1);
    close(fd);
    if (n <= 0) return 0;
    buf[n] = '\0';

    const char *p = buf;
    const char *end = buf + n;
    const size_t prefix_len = strlen("WAYLAND_DISPLAY=");

    while (p < end) {
        if (strncmp(p, "WAYLAND_DISPLAY=", prefix_len) == 0) {
            return (strcmp(p + prefix_len, target_display) == 0);
        }
        p += strlen(p) + 1;
    }
    return 0;
}

static void kill_other_wallpapers(void) {
    pid_t my_pid = getpid();
    const char *my_display = getenv("WAYLAND_DISPLAY");
    if (!my_display || !*my_display) {
        my_display = "wayland-0";
    }
    DIR *dir = opendir("/proc");
    if (!dir) return;
    struct dirent *ent;
    while ((ent = readdir(dir)) != NULL) {
        int pid = atoi(ent->d_name);
        if (pid > 0 && pid != my_pid) {
            char comm_path[256];
            snprintf(comm_path, sizeof(comm_path), "/proc/%d/comm", pid);
            FILE *f = fopen(comm_path, "r");
            if (f) {
                char comm[64];
                if (fgets(comm, sizeof(comm), f)) {
                    char *nl = strchr(comm, '\n');
                    if (nl) *nl = '\0';
                    if (strcmp(comm, "infynitae-wallp") == 0 ||
                        strcmp(comm, "infynitae-wallpaper") == 0 ||
                        strcmp(comm, "swaybg") == 0 ||
                        strcmp(comm, "mpvpaper") == 0) {
                        if (proc_matches_wayland_display(pid, my_display)) {
                            kill(pid, SIGTERM);
                        }
                    }
                }
                fclose(f);
            }
        }
    }
    closedir(dir);
    usleep(25000);
}

static void write_pid_file(void) {
    const char *pid_path = get_pid_file_path();
    if (!pid_path) return;
    FILE *f = fopen(pid_path, "w");
    if (f) {
        fprintf(f, "%d\n", getpid());
        fclose(f);
    }
}

static void expand_tilde(char *path, size_t max_len) {
    if (path[0] == '~' && (path[1] == '/' || path[1] == '\0')) {
        const char *home = getenv("HOME");
        if (home) {
            char temp[1024];
            snprintf(temp, sizeof(temp), "%s%s", home, path + 1);
            snprintf(path, max_len, "%s", temp);
        }
    }
}

static int g_parallax;  /* WALLPAPER_PARALLAX, read with the config */

static void load_config_defaults(char *target, size_t target_sz, char *mode, size_t mode_sz) {
    const char *home = getenv("HOME");
    if (!home) return;
    char path[512];
    snprintf(path, sizeof(path), "%s/.config/infynitae/config", home);
    FILE *f = fopen(path, "r");
    if (!f) return;

    char line[1024];
    while (fgets(line, sizeof(line), f)) {
        char *p = line;
        while (*p == ' ' || *p == '\t') p++;
        if (*p == '#' || *p == '\n' || *p == '\0') continue;

        char *eq = strchr(p, '=');
        if (!eq) continue;
        *eq = '\0';
        char *key = p;
        char *val = eq + 1;

        // trim key
        char *k_end = key + strlen(key) - 1;
        while (k_end > key && (*k_end == ' ' || *k_end == '\t')) *k_end-- = '\0';

        // the value as the shell reads it: a quoted string, or a word that ends at a space; a
        // comment after it is not part of it (owner, 2026-10-25: `WALLPAPER="…" # …`)
        while (*val == ' ' || *val == '\t') val++;
        if (*val == '"' || *val == '\'') {
            char quote = *val++;
            char *close = strchr(val, quote);
            if (close) *close = '\0';
        } else {
            val[strcspn(val, " \t\r\n#")] = '\0';
        }
        val[strcspn(val, "\r\n")] = '\0';

        if (strcmp(key, "WALLPAPER") == 0 && target[0] == '\0') {
            // the config is a shell file: "$HOME/..." and "~/..." mean the home directory
            if (strncmp(val, "$HOME/", 6) == 0) {
                snprintf(target, target_sz, "%s/%s", home, val + 6);
            } else if (strncmp(val, "${HOME}/", 8) == 0) {
                snprintf(target, target_sz, "%s/%s", home, val + 8);
            } else if (strncmp(val, "~/", 2) == 0) {
                snprintf(target, target_sz, "%s/%s", home, val + 2);
            } else {
                snprintf(target, target_sz, "%s", val);
            }
        } else if (strcmp(key, "WALLPAPER_MODE") == 0 && mode[0] == '\0') {
            snprintf(mode, mode_sz, "%s", val);
        } else if (strcmp(key, "WALLPAPER_PARALLAX") == 0) {
            g_parallax = !strcasecmp(val, "strong") ? 2 : (!strcasecmp(val, "subtle") || !strcasecmp(val, "on") ||
                                                          !strcasecmp(val, "true")) ? 1 : 0;
        }
    }
    fclose(f);
}

static void autodetect_wallpaper(char *target, size_t target_sz) {
    const char *home = getenv("HOME");
    if (!home) return;

    DIR *d;

    // Only folders meant for wallpapers: never a random picture from ~/Pictures
    // Standard wallpaper directories
    const char *subdirs[] = {"Pictures/Wallpapers", ".local/share/wallpapers", NULL};
    for (int i = 0; subdirs[i]; i++) {
        char dir_path[512];
        snprintf(dir_path, sizeof(dir_path), "%s/%s", home, subdirs[i]);
        d = opendir(dir_path);
        if (d) {
            struct dirent *ent;
            while ((ent = readdir(d)) != NULL) {
                const char *dot = strrchr(ent->d_name, '.');
                if (dot && (strcasecmp(dot, ".png") == 0 || strcasecmp(dot, ".jpg") == 0 ||
                            strcasecmp(dot, ".jpeg") == 0 || strcasecmp(dot, ".webp") == 0)) {
                    snprintf(target, target_sz, "%s/%s", dir_path, ent->d_name);
                    closedir(d);
                    return;
                }
            }
            closedir(d);
        }
    }

    // 4. System wallpapers
    const char *sys_dirs[] = {"/usr/share/backgrounds", "/usr/share/wallpapers", NULL};
    for (int i = 0; sys_dirs[i]; i++) {
        d = opendir(sys_dirs[i]);
        if (d) {
            struct dirent *ent;
            while ((ent = readdir(d)) != NULL) {
                const char *dot = strrchr(ent->d_name, '.');
                if (dot && (strcasecmp(dot, ".png") == 0 || strcasecmp(dot, ".jpg") == 0 ||
                            strcasecmp(dot, ".jpeg") == 0 || strcasecmp(dot, ".webp") == 0)) {
                    snprintf(target, target_sz, "%s/%s", sys_dirs[i], ent->d_name);
                    closedir(d);
                    return;
                }
            }
            closedir(d);
        }
    }
}

static int is_video_file(const char *path) {
    const char *dot = strrchr(path, '.');
    if (!dot) return 0;
    dot++;
    return (strcasecmp(dot, "mp4") == 0 ||
            strcasecmp(dot, "webm") == 0 ||
            strcasecmp(dot, "mkv") == 0 ||
            strcasecmp(dot, "avi") == 0 ||
            strcasecmp(dot, "mov") == 0);
}

int main(int argc, char *argv[]) {
    char target[1024] = "";
    char mode[32] = "";
    int foreground = 0;
    int overlay = 0;
    /* --spawned: the background copy started below; the starting command already checked the
     * image, killed the old wallpaper and wrote the lock pictures */
    int spawned = 0;
    char only_output[64] = "";

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--kill") == 0 || strcmp(argv[i], "-k") == 0) {
            kill_other_wallpapers();
            return 0;
        } else if (strcmp(argv[i], "--foreground") == 0 || strcmp(argv[i], "-f") == 0) {
            foreground = 1;
        } else if (strcmp(argv[i], "--spawned") == 0) {
            spawned = 1;
            foreground = 1;
        } else if (strcmp(argv[i], "--overlay") == 0) {
            /* a frozen screenshot over everything: the wallpaper itself is left running */
            overlay = 1;
            foreground = 1;
        } else if (strcmp(argv[i], "--output") == 0 && i + 1 < argc) {
            snprintf(only_output, sizeof(only_output), "%s", argv[++i]);
        } else if (strcmp(argv[i], "--colors") == 0 || strcmp(argv[i], "--accents") == 0) {
            if (i + 1 >= argc) {
                fprintf(stderr, "[infynitae-wallpaper] %s needs an image\n", argv[i]);
                return 1;
            }
            /* extract theme data from IMAGE and print it; nothing is drawn */
            GdkPixbuf *img = gdk_pixbuf_new_from_file(argv[i + 1], NULL);
            if (strcmp(argv[i], "--colors") == 0) {
                ThemeColors tc;
                int rc = theme_colors_from(img, &tc);
                if (img) g_object_unref(img);
                if (rc != 0) {
                    fprintf(stderr, "[infynitae-wallpaper] --colors: cannot read %s\n", argv[i + 1]);
                    return 1;
                }
                theme_colors_print(&tc, stdout);
            } else {
                double accents[6][3], pos[6][2];
                int n = theme_accents_from(img, accents, pos, 6);
                if (img) g_object_unref(img);
                if (n <= 0) {
                    fprintf(stderr, "[infynitae-wallpaper] --accents: cannot read %s\n", argv[i + 1]);
                    return 1;
                }
                theme_accents_print(accents, pos, n, stdout);
            }
            return 0;
        } else if (strcmp(argv[i], "--help") == 0 || strcmp(argv[i], "-h") == 0) {
            printf("Usage: infynitae-wallpaper [IMAGE_PATH] [MODE] [OPTIONS]\n");
            printf("Modes: fill, fit, stretch, center, tile\n");
            printf("Options:\n");
            printf("  -k, --kill         Terminate current wallpaper\n");
            printf("  -f, --foreground   Stay in foreground (do not daemonize)\n");
            printf("  --overlay          Show IMAGE above every window (a frozen screen), in the\n");
            printf("                     foreground, leaving the wallpaper running\n");
            printf("  --output NAME      Only on the output NAME (e.g. DP-1)\n");
            printf("  --colors IMAGE     Print theme colours taken from IMAGE (config lines)\n");
            printf("  --accents IMAGE    Print candidate accents and centres from IMAGE (#rrggbb x y)\n");
            printf("  -h, --help         Show this help message\n");
            return 0;
        } else if (target[0] == '\0') {
            snprintf(target, sizeof(target), "%s", argv[i]);
        } else if (mode[0] == '\0') {
            snprintf(mode, sizeof(mode), "%s", argv[i]);
        }
    }

    if (!overlay) {
        /* the image and its mode from the config when not given; WALLPAPER_PARALLAX always */
        load_config_defaults(target, sizeof(target), mode, sizeof(mode));
    }

    if (target[0] == '\0' && overlay) {
        fprintf(stderr, "[infynitae-wallpaper] --overlay needs an image\n");
        return 1;
    }

    /* the lock screen's picture of the user (lock-cache.h); the session starts this helper
     * whether or not there is a wallpaper */
    if (!overlay && !spawned) lock_cache_write_avatar();

    if (target[0] == '\0') {
        autodetect_wallpaper(target, sizeof(target));
    }

    if (target[0] == '\0') {
        kill_other_wallpapers();
        lock_cache_remove("lock-background.png");
        return 0;
    }

    expand_tilde(target, sizeof(target));

    if (access(target, R_OK) != 0) {
        fprintf(stderr, "[infynitae-wallpaper] Error: file not found or inaccessible: %s\n", target);
        return 1;
    }

    if (mode[0] == '\0') {
        snprintf(mode, sizeof(mode), "fill");
    }

    // Handle live animated video wallpaper via mpvpaper
    if (is_video_file(target)) {
        kill_other_wallpapers();
        lock_cache_remove("lock-background.png");
        if (system("command -v mpvpaper >/dev/null 2>&1") == 0) {
            // the path goes to mpvpaper as one argument: no shell, so any file name works
            execlp("mpvpaper", "mpvpaper", "-o", "--loop-file=inf --hwdec=auto --no-audio --vo=gpu",
                   "*", target, (char *)NULL);
            perror("[infynitae-wallpaper] mpvpaper");
            return 1;
        } else {
            fprintf(stderr, "[infynitae-wallpaper] For animated video wallpapers, please install mpvpaper: sudo pacman -S mpvpaper\n");
            return 1;
        }
    }

    /* read before forking, so a bad image fails the command that set it */
    GdkPixbuf *pixbuf = load_image(target);
    if (!pixbuf) return 1;

    if (!overlay && !spawned) {
        kill_other_wallpapers();
        /* the lock screen's background, from the image just decoded. Before the fork: gdk-pixbuf
         * 2.44 decodes and encodes through glycin's loader threads, which a forked child does not
         * have (found in the VM 2026-11-06: the save failed after the fork) */
        if (lock_cache_write_background(pixbuf) != 0)
            fprintf(stderr, "[infynitae-wallpaper] could not write the lock screen background\n");
    }

    /* Into the background unless foreground was asked: a fresh copy of this program, in its own
     * session, takes over and this command returns. Not a fork: gdk-pixbuf 2.44 decodes through
     * glycin's loader threads, which a forked child lacks, so the child could not decode the
     * image again for a new screen (found in the VM 2026-11-06). */
    if (!foreground) {
        g_object_unref(pixbuf);
        char exe[4096];
        ssize_t n = readlink("/proc/self/exe", exe, sizeof(exe) - 1);
        if (n <= 0) {
            fprintf(stderr, "[infynitae-wallpaper] cannot find its own program to start in the background\n");
            return 1;
        }
        exe[n] = '\0';
        char *child_argv[8];
        int c = 0;
        child_argv[c++] = exe;
        child_argv[c++] = "--spawned";
        child_argv[c++] = target;
        child_argv[c++] = mode;
        if (only_output[0]) {
            child_argv[c++] = "--output";
            child_argv[c++] = only_output;
        }
        child_argv[c] = NULL;
        posix_spawnattr_t attr;
        posix_spawnattr_init(&attr);
        posix_spawnattr_setflags(&attr, POSIX_SPAWN_SETSID);
        /* the caller's terminal is not held open: input and output from /dev/null, errors still
         * go where the caller's go (the session log) */
        posix_spawn_file_actions_t files;
        posix_spawn_file_actions_init(&files);
        posix_spawn_file_actions_addopen(&files, STDIN_FILENO, "/dev/null", O_RDONLY, 0);
        posix_spawn_file_actions_addopen(&files, STDOUT_FILENO, "/dev/null", O_WRONLY, 0);
        pid_t pid;
        int rc = posix_spawn(&pid, exe, &files, &attr, child_argv, environ);
        posix_spawn_file_actions_destroy(&files);
        posix_spawnattr_destroy(&attr);
        if (rc != 0) {
            fprintf(stderr, "[infynitae-wallpaper] Failed to start in the background: %s\n", strerror(rc));
            return 1;
        }
        return 0;
    }

    struct wallpaper_app app = {0};
    app.pixbuf = pixbuf;
    /* a path that does not fit is kept decoded instead (never released) */
    if (strlen(target) < sizeof(app.path)) snprintf(app.path, sizeof(app.path), "%s", target);
    snprintf(app.mode, sizeof(app.mode), "%s", mode);
    app.overlay = overlay;
    snprintf(app.only_output, sizeof(app.only_output), "%s", only_output);

    app.display = wl_display_connect(NULL);
    if (!app.display) {
        fprintf(stderr, "[infynitae-wallpaper] Error: could not connect to Wayland display\n");
        if (app.pixbuf) g_object_unref(app.pixbuf);
        return 1;
    }

    signal(SIGTERM, sig_handler);
    signal(SIGINT, sig_handler);

    app.registry = wl_display_get_registry(app.display);
    wl_registry_add_listener(app.registry, &registry_listener, &app);
    wl_display_roundtrip(app.display);
    wl_display_roundtrip(app.display);

    if (!overlay) {
        write_pid_file();
    }

    app.parallax = overlay ? 0 : g_parallax;
    parallax_connect(&app);
    if (app.ipc_fd < 0) {
        while (wl_display_dispatch(app.display) != -1) {
            // Sleep on events; consumes 0% CPU
        }
    } else {
        /* the display and the compositor's events: asleep between them */
        struct pollfd pfd[2] = {{.fd = wl_display_get_fd(app.display), .events = POLLIN},
                                {.fd = app.ipc_fd, .events = POLLIN}};
        for (;;) {
            while (wl_display_prepare_read(app.display) != 0)
                if (wl_display_dispatch_pending(app.display) < 0) goto done;
            wl_display_flush(app.display);
            pfd[1].fd = app.ipc_fd;
            if (poll(pfd, app.ipc_fd >= 0 ? 2 : 1, -1) < 0 && errno != EINTR) {
                wl_display_cancel_read(app.display);
                break;
            }
            if (pfd[0].revents & POLLIN) {
                if (wl_display_read_events(app.display) < 0) break;
            } else {
                wl_display_cancel_read(app.display);
            }
            if (wl_display_dispatch_pending(app.display) < 0) break;
            if (app.ipc_fd >= 0 && (pfd[1].revents & (POLLIN | POLLHUP))) parallax_read(&app);
            wl_display_flush(app.display);
        }
    done:;
    }

    // Clean up
    if (!overlay) {
        if (get_pid_file_path()) unlink(get_pid_file_path());
    }

    struct wallpaper_output *curr = app.outputs;
    while (curr) {
        struct wallpaper_output *next = curr->next;
        if (curr->layer_surface) zwlr_layer_surface_v1_destroy(curr->layer_surface);
        if (curr->surface) wl_surface_destroy(curr->surface);
        if (curr->buffer) wl_buffer_destroy(curr->buffer);
        if (curr->shm_data) munmap(curr->shm_data, curr->shm_size);
        if (curr->output) wl_output_destroy(curr->output);
        free(curr);
        curr = next;
    }

    if (app.layer_shell) zwlr_layer_shell_v1_destroy(app.layer_shell);
    if (app.compositor) wl_compositor_destroy(app.compositor);
    if (app.shm) wl_shm_destroy(app.shm);
    if (app.registry) wl_registry_destroy(app.registry);
    wl_display_disconnect(app.display);

    if (app.pixbuf) g_object_unref(app.pixbuf);

    return 0;
}
