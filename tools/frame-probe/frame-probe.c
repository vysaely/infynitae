/*
 * frame-probe — Wayland frame pacing and motion-to-screen probe.
 *
 * Measures on any Wayland compositor supporting wp_presentation (Infynitae,
 * GNOME, KDE) and compares equivalent metrics (dropped frames, input-to-screen
 * latency) across desktops:
 *   1. Draws a frame counter every frame (driven by wl_surface.frame) and tracks
 *      missed refreshes via wp_presentation_feedback (MSC seq gaps > 1 when the
 *      compositor reports non-zero seq, and inter-present intervals > 1.5x refresh).
 *   2. Draws a square at the pointer (wl_pointer.motion) and pairs each motion
 *      timestamp with the presentation timestamp of the frame that showed it.
 *
 * Build:
 *   make -C tools/frame-probe
 *   (or directly:
 *    gcc -O2 -Wall -Wextra -Werror $(pkg-config --cflags wayland-client) \
 *      frame-probe.c xdg-shell-protocol.c presentation-time-protocol.c \
 *      -o frame-probe $(pkg-config --libs wayland-client))
 *
 * Run:
 *   ./tools/frame-probe/frame-probe [--duration SECS] [--interval SECS] [--width W] [--height H]
 *
 * Examples:
 *   ./tools/frame-probe/frame-probe --duration 60 --interval 1
 *   ./tools/frame-probe/frame-probe            # runs until Ctrl-C or window closed
 */
#define _GNU_SOURCE

#include "presentation-time-protocol.h"
#include "probe_stats.h"
#include "xdg-shell-protocol.h"

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>

#define NUM_BUFFERS 3
#define DEFAULT_WIDTH 480
#define DEFAULT_HEIGHT 320
#define MIN_SIZE 64
#define MAX_SIZE 4096

#define COLOR_BG 0xFF141416U
#define COLOR_FG 0xFFF2F2F2U
#define COLOR_DIM 0xFF52525AU
#define COLOR_BOX 0xFFE8E8EAU

struct shm_slot {
	struct probe_app *app;
	struct wl_buffer *buffer;
	uint32_t *pixels;
	size_t map_size;
	int width;
	int height;
	int stride;
	bool busy;
	bool retired;
	struct shm_slot *next_retired;
};

struct pending_fb {
	struct wp_presentation_feedback *proxy;
	struct probe_app *app;
	uint64_t commit_seq;
	struct wl_output *sync_output;
	struct pending_fb *next;
};

struct output_node {
	uint32_t name;
	struct wl_output *proxy;
	struct output_node *next;
};

struct probe_app {
	struct wl_display *display;
	struct wl_registry *registry;
	struct wl_compositor *compositor;
	uint32_t compositor_ver;
	struct wl_shm *shm;
	struct xdg_wm_base *wm_base;
	struct wp_presentation *presentation;
	struct wl_seat *seat;
	uint32_t seat_ver;
	struct wl_pointer *pointer;

	struct wl_surface *surface;
	struct xdg_surface *xdg_surface;
	struct xdg_toplevel *xdg_toplevel;
	struct wl_callback *frame_cb;

	struct shm_slot *slots[NUM_BUFFERS];
	struct shm_slot *retired_slots;
	struct pending_fb *pending_fbs;
	struct output_node *outputs;

	int cur_w;
	int cur_h;
	int pending_w;
	int pending_h;
	bool configured;
	bool running;
	bool want_frame;
	bool frame_cb_pending;

	clockid_t clock_id;
	bool has_clock_id;

	bool ptr_inside;
	int ptr_x;
	int ptr_y;

	uint64_t frame_counter;
	uint64_t commit_seq;
	struct fp_stats stats;
};

static volatile sig_atomic_t g_stop = 0;

static void on_signal(int sig)
{
	(void)sig;
	g_stop = 1;
}

static int clamp_dim(int v)
{
	if (v < MIN_SIZE)
		return MIN_SIZE;
	if (v > MAX_SIZE)
		return MAX_SIZE;
	return v;
}

static int64_t read_clock_ns(clockid_t clk)
{
	struct timespec ts;
	if (clock_gettime(clk, &ts) != 0)
		clock_gettime(CLOCK_MONOTONIC, &ts);
	return (int64_t)ts.tv_sec * 1000000000LL + (int64_t)ts.tv_nsec;
}

/* 5x7 bitmaps for digits '0'..'9', row-major 5 bits MSB..LSB. */
static const uint8_t FONT_DIGITS[10][7] = {
	{0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E}, /* 0 */
	{0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E}, /* 1 */
	{0x0E, 0x11, 0x01, 0x06, 0x08, 0x10, 0x1F}, /* 2 */
	{0x1F, 0x02, 0x04, 0x06, 0x01, 0x11, 0x0E}, /* 3 */
	{0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02}, /* 4 */
	{0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E}, /* 5 */
	{0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E}, /* 6 */
	{0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08}, /* 7 */
	{0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E}, /* 8 */
	{0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C}, /* 9 */
};

static void fill_rect(uint32_t *px, int w, int h, int x0, int y0, int rw, int rh, uint32_t color)
{
	int x1 = x0 + rw;
	int y1 = y0 + rh;
	if (x0 < 0)
		x0 = 0;
	if (y0 < 0)
		y0 = 0;
	if (x1 > w)
		x1 = w;
	if (y1 > h)
		y1 = h;
	for (int y = y0; y < y1; y++) {
		uint32_t *row = px + (size_t)y * (size_t)w;
		for (int x = x0; x < x1; x++)
			row[x] = color;
	}
}

static void draw_digit(uint32_t *px, int w, int h, int digit, int ox, int oy, int scale, uint32_t color)
{
	for (int r = 0; r < 7; r++) {
		uint8_t bits = FONT_DIGITS[digit][r];
		for (int c = 0; c < 5; c++) {
			if (bits & (1U << (4 - c)))
				fill_rect(px, w, h, ox + c * scale, oy + r * scale, scale, scale, color);
		}
	}
}

static void render_frame(const struct probe_app *app, struct shm_slot *slot)
{
	int w = slot->width;
	int h = slot->height;
	uint32_t *px = slot->pixels;
	size_t n = (size_t)w * (size_t)h;

	for (size_t i = 0; i < n; i++)
		px[i] = COLOR_BG;

	/* 8-digit frame counter scaled cleanly to fit the window. */
	char digits[16];
	snprintf(digits, sizeof(digits), "%08llu", (unsigned long long)(app->frame_counter % 100000000ULL));

	int scale = w / 64;
	if (scale < 2)
		scale = 2;
	if (scale > 8)
		scale = 8;
	int glyph_w = 5 * scale;
	int glyph_gap = scale;
	int total_w = 8 * glyph_w + 7 * glyph_gap;
	int start_x = (w - total_w) / 2;
	int start_y = h / 6;

	for (int i = 0; i < 8; i++)
		draw_digit(px, w, h, digits[i] - '0', start_x + i * (glyph_w + glyph_gap), start_y, scale, COLOR_FG);

	/* 16-cell step strip so adjacent frames look distinct even under high-speed camera inspection. */
	int cells = 16;
	int cell_w = (w * 3 / 4) / cells;
	if (cell_w < 4)
		cell_w = 4;
	int strip_w = cells * cell_w;
	int strip_x = (w - strip_w) / 2;
	int strip_y = start_y + 7 * scale + scale * 3;
	int cell_h = scale * 2;
	if (strip_y + cell_h < h - 8) {
		int active = (int)(app->frame_counter % (uint64_t)cells);
		for (int c = 0; c < cells; c++) {
			uint32_t col = (c == active) ? COLOR_FG : COLOR_DIM;
			fill_rect(px, w, h, strip_x + c * cell_w + 1, strip_y, cell_w - 2, cell_h, col);
		}
	}

	/* Pointer square + thin 1 px crosshair lines when cursor is inside the surface. */
	if (app->ptr_inside) {
		int box = 24;
		fill_rect(px, w, h, app->ptr_x - box / 2, app->ptr_y - box / 2, box, box, COLOR_BOX);
		fill_rect(px, w, h, app->ptr_x - 2, app->ptr_y - 2, 4, 4, COLOR_BG);
		fill_rect(px, w, h, app->ptr_x - 32, app->ptr_y, 64, 1, COLOR_FG);
		fill_rect(px, w, h, app->ptr_x, app->ptr_y - 32, 1, 64, COLOR_FG);
	}
}

static void destroy_slot(struct shm_slot *slot)
{
	if (!slot)
		return;
	if (slot->buffer)
		wl_buffer_destroy(slot->buffer);
	if (slot->pixels && slot->pixels != MAP_FAILED)
		munmap(slot->pixels, slot->map_size);
	free(slot);
}

static void remove_retired_slot(struct probe_app *app, struct shm_slot *target)
{
	struct shm_slot **pp = &app->retired_slots;
	while (*pp) {
		if (*pp == target) {
			*pp = target->next_retired;
			destroy_slot(target);
			return;
		}
		pp = &(*pp)->next_retired;
	}
}

static void on_buffer_release(void *data, struct wl_buffer *wl_buffer)
{
	(void)wl_buffer;
	struct shm_slot *slot = (struct shm_slot *)data;
	slot->busy = false;
	if (slot->retired) {
		remove_retired_slot(slot->app, slot);
		return;
	}
}

static const struct wl_buffer_listener buffer_listener = {
	.release = on_buffer_release,
};

static struct shm_slot *create_slot(struct probe_app *app, int w, int h)
{
	int stride = w * 4;
	size_t size = (size_t)stride * (size_t)h;

	int fd = memfd_create("frame-probe-shm", MFD_CLOEXEC);
	if (fd < 0)
		return NULL;
	if (ftruncate(fd, (off_t)size) < 0) {
		close(fd);
		return NULL;
	}

	uint32_t *pixels = (uint32_t *)mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	if (pixels == MAP_FAILED) {
		close(fd);
		return NULL;
	}

	struct shm_slot *slot = (struct shm_slot *)calloc(1, sizeof(*slot));
	if (!slot) {
		munmap(pixels, size);
		close(fd);
		return NULL;
	}

	struct wl_shm_pool *pool = wl_shm_create_pool(app->shm, fd, (int32_t)size);
	close(fd);
	if (!pool) {
		munmap(pixels, size);
		free(slot);
		return NULL;
	}

	struct wl_buffer *buf = wl_shm_pool_create_buffer(pool, 0, w, h, stride, WL_SHM_FORMAT_XRGB8888);
	wl_shm_pool_destroy(pool);
	if (!buf) {
		munmap(pixels, size);
		free(slot);
		return NULL;
	}

	slot->app = app;
	slot->buffer = buf;
	slot->pixels = pixels;
	slot->map_size = size;
	slot->width = w;
	slot->height = h;
	slot->stride = stride;
	wl_buffer_add_listener(buf, &buffer_listener, slot);
	return slot;
}

static void retire_all_slots(struct probe_app *app)
{
	for (int i = 0; i < NUM_BUFFERS; i++) {
		struct shm_slot *slot = app->slots[i];
		app->slots[i] = NULL;
		if (!slot)
			continue;
		if (!slot->busy) {
			destroy_slot(slot);
		} else {
			slot->retired = true;
			slot->next_retired = app->retired_slots;
			app->retired_slots = slot;
		}
	}
}

static bool ensure_slots(struct probe_app *app, int w, int h)
{
	if (app->cur_w == w && app->cur_h == h && app->slots[0] != NULL)
		return true;

	retire_all_slots(app);
	for (int i = 0; i < NUM_BUFFERS; i++) {
		app->slots[i] = create_slot(app, w, h);
		if (!app->slots[i]) {
			retire_all_slots(app);
			return false;
		}
	}
	app->cur_w = w;
	app->cur_h = h;
	return true;
}

static struct shm_slot *acquire_slot(struct probe_app *app)
{
	for (int i = 0; i < NUM_BUFFERS; i++) {
		if (app->slots[i] && !app->slots[i]->busy)
			return app->slots[i];
	}
	return NULL;
}

static void unlink_pending_fb(struct probe_app *app, struct pending_fb *target)
{
	struct pending_fb **pp = &app->pending_fbs;
	while (*pp) {
		if (*pp == target) {
			*pp = target->next;
			return;
		}
		pp = &(*pp)->next;
	}
}

static void on_fb_sync_output(void *data, struct wp_presentation_feedback *fb, struct wl_output *output)
{
	(void)fb;
	struct pending_fb *pfb = (struct pending_fb *)data;
	pfb->sync_output = output;
}

static void on_fb_presented(void *data, struct wp_presentation_feedback *fb,
			    uint32_t tv_sec_hi, uint32_t tv_sec_lo, uint32_t tv_nsec,
			    uint32_t refresh, uint32_t seq_hi, uint32_t seq_lo, uint32_t flags)
{
	(void)flags;
	struct pending_fb *pfb = (struct pending_fb *)data;
	struct probe_app *app = pfb->app;

	int64_t sec = (int64_t)(((uint64_t)tv_sec_hi << 32) | (uint64_t)tv_sec_lo);
	int64_t present_ns = sec * 1000000000LL + (int64_t)tv_nsec;
	uint64_t seq = ((uint64_t)seq_hi << 32) | (uint64_t)seq_lo;

	fp_stats_on_presented(&app->stats, pfb->commit_seq, present_ns, refresh, seq, pfb->sync_output);

	unlink_pending_fb(app, pfb);
	wp_presentation_feedback_destroy(fb);
	free(pfb);
}

static void on_fb_discarded(void *data, struct wp_presentation_feedback *fb)
{
	struct pending_fb *pfb = (struct pending_fb *)data;
	struct probe_app *app = pfb->app;

	fp_stats_on_discarded(&app->stats);

	unlink_pending_fb(app, pfb);
	wp_presentation_feedback_destroy(fb);
	free(pfb);
}

static const struct wp_presentation_feedback_listener fb_listener = {
	.sync_output = on_fb_sync_output,
	.presented = on_fb_presented,
	.discarded = on_fb_discarded,
};

static void submit_frame(struct probe_app *app);

static void on_frame_done(void *data, struct wl_callback *cb, uint32_t time)
{
	(void)time;
	struct probe_app *app = (struct probe_app *)data;
	wl_callback_destroy(cb);
	app->frame_cb = NULL;
	app->frame_cb_pending = false;
	app->want_frame = true;
	if (app->configured && app->running)
		submit_frame(app);
}

static const struct wl_callback_listener frame_listener = {
	.done = on_frame_done,
};

static void submit_frame(struct probe_app *app)
{
	if (!app->configured || !app->running || app->frame_cb_pending || !app->want_frame)
		return;
	if (!ensure_slots(app, app->pending_w, app->pending_h)) {
		fprintf(stderr, "frame-probe: failed to allocate shm buffers\n");
		app->running = false;
		return;
	}

	struct shm_slot *slot = acquire_slot(app);
	if (!slot)
		return;

	app->frame_counter++;
	render_frame(app, slot);
	slot->busy = true;
	app->want_frame = false;

	app->commit_seq++;
	fp_stats_on_commit(&app->stats, app->commit_seq);

	wl_surface_attach(app->surface, slot->buffer, 0, 0);
	if (app->compositor_ver >= 4)
		wl_surface_damage_buffer(app->surface, 0, 0, slot->width, slot->height);
	else
		wl_surface_damage(app->surface, 0, 0, slot->width, slot->height);

	app->frame_cb = wl_surface_frame(app->surface);
	if (app->frame_cb) {
		wl_callback_add_listener(app->frame_cb, &frame_listener, app);
		app->frame_cb_pending = true;
	}

	struct wp_presentation_feedback *fb = wp_presentation_feedback(app->presentation, app->surface);
	if (fb) {
		struct pending_fb *pfb = (struct pending_fb *)calloc(1, sizeof(*pfb));
		if (pfb) {
			pfb->proxy = fb;
			pfb->app = app;
			pfb->commit_seq = app->commit_seq;
			pfb->next = app->pending_fbs;
			app->pending_fbs = pfb;
			wp_presentation_feedback_add_listener(fb, &fb_listener, pfb);
		} else {
			wp_presentation_feedback_destroy(fb);
		}
	}

	wl_surface_commit(app->surface);
}

static void on_xdg_wm_base_ping(void *data, struct xdg_wm_base *wm_base, uint32_t serial)
{
	(void)data;
	xdg_wm_base_pong(wm_base, serial);
}

static const struct xdg_wm_base_listener wm_base_listener = {
	.ping = on_xdg_wm_base_ping,
};

static void on_xdg_surface_configure(void *data, struct xdg_surface *surface, uint32_t serial)
{
	struct probe_app *app = (struct probe_app *)data;
	xdg_surface_ack_configure(surface, serial);
	app->configured = true;
	if (!app->frame_cb_pending) {
		app->want_frame = true;
		submit_frame(app);
	}
}

static const struct xdg_surface_listener xdg_surface_listener = {
	.configure = on_xdg_surface_configure,
};

static void on_xdg_toplevel_configure(void *data, struct xdg_toplevel *toplevel,
				      int32_t width, int32_t height, struct wl_array *states)
{
	(void)toplevel;
	(void)states;
	struct probe_app *app = (struct probe_app *)data;
	if (width > 0)
		app->pending_w = clamp_dim(width);
	if (height > 0)
		app->pending_h = clamp_dim(height);
}

static void on_xdg_toplevel_close(void *data, struct xdg_toplevel *toplevel)
{
	(void)toplevel;
	struct probe_app *app = (struct probe_app *)data;
	app->running = false;
}

static const struct xdg_toplevel_listener xdg_toplevel_listener = {
	.configure = on_xdg_toplevel_configure,
	.close = on_xdg_toplevel_close,
};

static void on_presentation_clock_id(void *data, struct wp_presentation *pres, uint32_t clk_id)
{
	(void)pres;
	struct probe_app *app = (struct probe_app *)data;
	struct timespec ts;
	if (clock_gettime((clockid_t)clk_id, &ts) == 0) {
		app->clock_id = (clockid_t)clk_id;
		app->has_clock_id = true;
	} else {
		fprintf(stderr, "frame-probe: unsupported presentation clock_id %u, using CLOCK_MONOTONIC\n",
			clk_id);
		app->clock_id = CLOCK_MONOTONIC;
	}
}

static const struct wp_presentation_listener presentation_listener = {
	.clock_id = on_presentation_clock_id,
};

static void on_ptr_enter(void *data, struct wl_pointer *ptr, uint32_t serial,
			 struct wl_surface *surface, wl_fixed_t sx, wl_fixed_t sy)
{
	(void)ptr;
	(void)serial;
	struct probe_app *app = (struct probe_app *)data;
	if (surface != app->surface)
		return;
	app->ptr_inside = true;
	app->ptr_x = wl_fixed_to_int(sx);
	app->ptr_y = wl_fixed_to_int(sy);
}

static void on_ptr_leave(void *data, struct wl_pointer *ptr, uint32_t serial, struct wl_surface *surface)
{
	(void)ptr;
	(void)serial;
	struct probe_app *app = (struct probe_app *)data;
	if (surface != app->surface)
		return;
	app->ptr_inside = false;
}

static void on_ptr_motion(void *data, struct wl_pointer *ptr, uint32_t time, wl_fixed_t sx, wl_fixed_t sy)
{
	(void)ptr;
	struct probe_app *app = (struct probe_app *)data;
	int64_t recv_ns = read_clock_ns(app->clock_id);
	app->ptr_inside = true;
	app->ptr_x = wl_fixed_to_int(sx);
	app->ptr_y = wl_fixed_to_int(sy);
	fp_stats_record_motion(&app->stats, time, recv_ns);
}

static void on_ptr_button(void *data, struct wl_pointer *ptr, uint32_t serial,
			  uint32_t time, uint32_t button, uint32_t state)
{
	(void)data;
	(void)ptr;
	(void)serial;
	(void)time;
	(void)button;
	(void)state;
}

static void on_ptr_axis(void *data, struct wl_pointer *ptr, uint32_t time, uint32_t axis, wl_fixed_t value)
{
	(void)data;
	(void)ptr;
	(void)time;
	(void)axis;
	(void)value;
}

static void on_ptr_frame(void *data, struct wl_pointer *ptr)
{
	(void)data;
	(void)ptr;
}

static void on_ptr_axis_source(void *data, struct wl_pointer *ptr, uint32_t source)
{
	(void)data;
	(void)ptr;
	(void)source;
}

static void on_ptr_axis_stop(void *data, struct wl_pointer *ptr, uint32_t time, uint32_t axis)
{
	(void)data;
	(void)ptr;
	(void)time;
	(void)axis;
}

static void on_ptr_axis_discrete(void *data, struct wl_pointer *ptr, uint32_t axis, int32_t discrete)
{
	(void)data;
	(void)ptr;
	(void)axis;
	(void)discrete;
}

static const struct wl_pointer_listener pointer_listener = {
	.enter = on_ptr_enter,
	.leave = on_ptr_leave,
	.motion = on_ptr_motion,
	.button = on_ptr_button,
	.axis = on_ptr_axis,
	.frame = on_ptr_frame,
	.axis_source = on_ptr_axis_source,
	.axis_stop = on_ptr_axis_stop,
	.axis_discrete = on_ptr_axis_discrete,
};

static void destroy_pointer(struct probe_app *app)
{
	if (!app->pointer)
		return;
	if (app->seat_ver >= 3)
		wl_pointer_release(app->pointer);
	else
		wl_pointer_destroy(app->pointer);
	app->pointer = NULL;
	app->ptr_inside = false;
}

static void on_seat_capabilities(void *data, struct wl_seat *seat, uint32_t caps)
{
	struct probe_app *app = (struct probe_app *)data;
	bool has_ptr = (caps & WL_SEAT_CAPABILITY_POINTER) != 0;
	if (has_ptr && !app->pointer) {
		app->pointer = wl_seat_get_pointer(seat);
		if (app->pointer)
			wl_pointer_add_listener(app->pointer, &pointer_listener, app);
	} else if (!has_ptr && app->pointer) {
		destroy_pointer(app);
	}
}

static void on_seat_name(void *data, struct wl_seat *seat, const char *name)
{
	(void)data;
	(void)seat;
	(void)name;
}

static const struct wl_seat_listener seat_listener = {
	.capabilities = on_seat_capabilities,
	.name = on_seat_name,
};

static void on_registry_global(void *data, struct wl_registry *reg, uint32_t name,
			       const char *interface, uint32_t version)
{
	struct probe_app *app = (struct probe_app *)data;

	if (strcmp(interface, wl_compositor_interface.name) == 0 && !app->compositor) {
		uint32_t v = version < 4 ? version : 4;
		app->compositor = (struct wl_compositor *)wl_registry_bind(reg, name, &wl_compositor_interface, v);
		app->compositor_ver = v;
	} else if (strcmp(interface, wl_shm_interface.name) == 0 && !app->shm) {
		app->shm = (struct wl_shm *)wl_registry_bind(reg, name, &wl_shm_interface, 1);
	} else if (strcmp(interface, xdg_wm_base_interface.name) == 0 && !app->wm_base) {
		app->wm_base = (struct xdg_wm_base *)wl_registry_bind(reg, name, &xdg_wm_base_interface, 1);
		xdg_wm_base_add_listener(app->wm_base, &wm_base_listener, app);
	} else if (strcmp(interface, wp_presentation_interface.name) == 0 && !app->presentation) {
		uint32_t v = version < 2 ? version : 2;
		app->presentation = (struct wp_presentation *)wl_registry_bind(reg, name, &wp_presentation_interface, v);
		wp_presentation_add_listener(app->presentation, &presentation_listener, app);
	} else if (strcmp(interface, wl_seat_interface.name) == 0 && !app->seat) {
		uint32_t v = version < 5 ? version : 5;
		app->seat = (struct wl_seat *)wl_registry_bind(reg, name, &wl_seat_interface, v);
		app->seat_ver = v;
		wl_seat_add_listener(app->seat, &seat_listener, app);
	} else if (strcmp(interface, wl_output_interface.name) == 0) {
		struct wl_output *out = (struct wl_output *)wl_registry_bind(reg, name, &wl_output_interface, 1);
		if (out) {
			struct output_node *node = (struct output_node *)calloc(1, sizeof(*node));
			if (node) {
				node->name = name;
				node->proxy = out;
				node->next = app->outputs;
				app->outputs = node;
			} else {
				wl_output_destroy(out);
			}
		}
	}
}

static void on_registry_remove(void *data, struct wl_registry *reg, uint32_t name)
{
	(void)reg;
	struct probe_app *app = (struct probe_app *)data;
	struct output_node **pp = &app->outputs;
	while (*pp) {
		if ((*pp)->name == name) {
			struct output_node *dead = *pp;
			*pp = dead->next;
			wl_output_destroy(dead->proxy);
			free(dead);
			return;
		}
		pp = &(*pp)->next;
	}
}

static const struct wl_registry_listener registry_listener = {
	.global = on_registry_global,
	.global_remove = on_registry_remove,
};

static void print_interval(struct probe_app *app, double elapsed_s)
{
	struct fp_stats *s = &app->stats;
	printf("t=%6.1fs  presented=%4" PRIu64 "  discarded=%2" PRIu64,
	       elapsed_s, s->interval_frames, s->interval_discarded);

	if (s->interval_seq_pairs > 0)
		printf("  missed_seq=%" PRIu64 "/%" PRIu64, s->interval_seq_missed, s->interval_seq_pairs);
	else
		printf("  missed_seq=n/a(seq=0)");

	printf("  missed_time=%" PRIu64 "/%" PRIu64, s->interval_dt_missed, s->interval_dt_pairs);

	if (s->interval_lat_n > 0) {
		double min_ms = (double)s->interval_lat_min / 1e6;
		double mean_ms = ((double)s->interval_lat_sum / (double)s->interval_lat_n) / 1e6;
		double max_ms = (double)s->interval_lat_max / 1e6;
		printf("  motion(n=%zu min=%.2f mean=%.2f max=%.2f ms)",
		       s->interval_lat_n, min_ms, mean_ms, max_ms);
	} else {
		printf("  motion(n=0)");
	}
	printf("\n");
	fflush(stdout);
	fp_stats_reset_interval(s);
}

static void print_summary(const struct probe_app *app, double elapsed_s)
{
	const struct fp_stats *s = &app->stats;
	struct fp_latency_summary lat = fp_stats_latency_summary(s);

	printf("\nframe-probe summary (elapsed=%.2fs, clock_id=%d)\n", elapsed_s, (int)app->clock_id);
	printf("  frames: committed=%" PRIu64 " presented=%" PRIu64 " discarded=%" PRIu64 " output_switches=%" PRIu64 "\n",
	       s->frames_committed, s->frames_presented, s->frames_discarded, s->output_switches);

	if (s->seq_pairs > 0) {
		printf("  missed_refreshes_seq:  %" PRIu64 " (gaps=%" PRIu64 " over %" PRIu64 " pairs)\n",
		       s->seq_missed, s->seq_gaps, s->seq_pairs);
	} else {
		printf("  missed_refreshes_seq:  n/a (compositor reported seq=0)\n");
	}
	printf("  missed_refreshes_time: %" PRIu64 " (gaps=%" PRIu64 " over %" PRIu64 " pairs)\n",
	       s->dt_missed, s->dt_gaps, s->dt_pairs);

	printf("  motion_to_screen: matched=%" PRIu64 " events=%" PRIu64 " fallbacks=%" PRIu64
	       " invalid=%" PRIu64 " overflow=%" PRIu64 "\n",
	       s->motion_matched, s->motion_events, s->motion_clock_fallbacks,
	       s->motion_negative_discarded, s->motion_dropped_overflow);

	if (lat.count > 0) {
		printf("    min=%.2fms  p50=%.2fms  p95=%.2fms  p99=%.2fms  max=%.2fms  mean=%.2fms\n",
		       lat.min_ms, lat.p50_ms, lat.p95_ms, lat.p99_ms, lat.max_ms, lat.mean_ms);
	} else {
		printf("    no pointer motion events matched to a presented frame\n");
	}
}

static void usage(const char *prog)
{
	printf("Usage: %s [--duration SECS] [--interval SECS] [--width W] [--height H]\n"
	       "\n"
	       "Measures presented frame drops (wp_presentation_feedback) and pointer\n"
	       "motion-to-screen latency on any Wayland desktop.\n"
	       "\n"
	       "Options:\n"
	       "  --duration SECS   Stop automatically after SECS seconds (0 = until closed/Ctrl-C)\n"
	       "  --interval SECS   Print interval stats every SECS seconds (default: 1, 0 = quiet)\n"
	       "  --width W         Initial window width in pixels (default: %d)\n"
	       "  --height H        Initial window height in pixels (default: %d)\n"
	       "  -h, --help        Show this help and exit\n",
	       prog, DEFAULT_WIDTH, DEFAULT_HEIGHT);
}

static void cleanup_app(struct probe_app *app)
{
	while (app->pending_fbs) {
		struct pending_fb *pfb = app->pending_fbs;
		app->pending_fbs = pfb->next;
		wp_presentation_feedback_destroy(pfb->proxy);
		free(pfb);
	}
	if (app->frame_cb)
		wl_callback_destroy(app->frame_cb);

	retire_all_slots(app);
	while (app->retired_slots) {
		struct shm_slot *slot = app->retired_slots;
		app->retired_slots = slot->next_retired;
		destroy_slot(slot);
	}

	destroy_pointer(app);
	if (app->seat)
		wl_seat_destroy(app->seat);

	while (app->outputs) {
		struct output_node *node = app->outputs;
		app->outputs = node->next;
		wl_output_destroy(node->proxy);
		free(node);
	}

	if (app->xdg_toplevel)
		xdg_toplevel_destroy(app->xdg_toplevel);
	if (app->xdg_surface)
		xdg_surface_destroy(app->xdg_surface);
	if (app->surface)
		wl_surface_destroy(app->surface);
	if (app->presentation)
		wp_presentation_destroy(app->presentation);
	if (app->wm_base)
		xdg_wm_base_destroy(app->wm_base);
	if (app->shm)
		wl_shm_destroy(app->shm);
	if (app->compositor)
		wl_compositor_destroy(app->compositor);
	if (app->registry)
		wl_registry_destroy(app->registry);
	if (app->display)
		wl_display_disconnect(app->display);

	fp_stats_free(&app->stats);
}

int main(int argc, char **argv)
{
	double duration_s = 0.0;
	double interval_s = 1.0;
	int width = DEFAULT_WIDTH;
	int height = DEFAULT_HEIGHT;

	for (int i = 1; i < argc; i++) {
		if ((strcmp(argv[i], "-h") == 0) || (strcmp(argv[i], "--help") == 0)) {
			usage(argv[0]);
			return 0;
		} else if (strcmp(argv[i], "--duration") == 0 && i + 1 < argc) {
			duration_s = atof(argv[++i]);
			if (duration_s < 0.0) {
				fprintf(stderr, "frame-probe: --duration must be >= 0\n");
				return 2;
			}
		} else if (strcmp(argv[i], "--interval") == 0 && i + 1 < argc) {
			interval_s = atof(argv[++i]);
			if (interval_s < 0.0) {
				fprintf(stderr, "frame-probe: --interval must be >= 0\n");
				return 2;
			}
		} else if (strcmp(argv[i], "--width") == 0 && i + 1 < argc) {
			width = clamp_dim(atoi(argv[++i]));
		} else if (strcmp(argv[i], "--height") == 0 && i + 1 < argc) {
			height = clamp_dim(atoi(argv[++i]));
		} else {
			fprintf(stderr, "frame-probe: unknown or incomplete option '%s'\n", argv[i]);
			usage(argv[0]);
			return 2;
		}
	}

	struct probe_app app;
	memset(&app, 0, sizeof(app));
	app.pending_w = width;
	app.pending_h = height;
	app.running = true;
	app.clock_id = CLOCK_MONOTONIC;
	fp_stats_init(&app.stats);

	struct sigaction sa;
	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = on_signal;
	sigemptyset(&sa.sa_mask);
	sigaction(SIGINT, &sa, NULL);
	sigaction(SIGTERM, &sa, NULL);

	app.display = wl_display_connect(NULL);
	if (!app.display) {
		fprintf(stderr, "frame-probe: cannot connect to Wayland display\n");
		return 1;
	}

	app.registry = wl_display_get_registry(app.display);
	wl_registry_add_listener(app.registry, &registry_listener, &app);
	wl_display_roundtrip(app.display);
	/* Second roundtrip ensures initial clock_id and seat capabilities arrive. */
	wl_display_roundtrip(app.display);

	if (!app.compositor || !app.shm || !app.wm_base) {
		fprintf(stderr, "frame-probe: missing required Wayland globals (compositor/shm/xdg_wm_base)\n");
		cleanup_app(&app);
		return 1;
	}
	if (!app.presentation) {
		fprintf(stderr, "frame-probe: compositor does not advertise wp_presentation\n");
		cleanup_app(&app);
		return 1;
	}

	app.surface = wl_compositor_create_surface(app.compositor);
	app.xdg_surface = xdg_wm_base_get_xdg_surface(app.wm_base, app.surface);
	xdg_surface_add_listener(app.xdg_surface, &xdg_surface_listener, &app);
	app.xdg_toplevel = xdg_surface_get_toplevel(app.xdg_surface);
	xdg_toplevel_add_listener(app.xdg_toplevel, &xdg_toplevel_listener, &app);
	xdg_toplevel_set_title(app.xdg_toplevel, "frame-probe");
	xdg_toplevel_set_app_id(app.xdg_toplevel, "infynitae.frame-probe");
	wl_surface_commit(app.surface);

	int64_t start_ns = read_clock_ns(CLOCK_MONOTONIC);
	int64_t end_ns = (duration_s > 0.0) ? start_ns + (int64_t)(duration_s * 1e9) : 0;
	int64_t interval_ns = (interval_s > 0.0) ? (int64_t)(interval_s * 1e9) : 0;
	int64_t next_report_ns = (interval_ns > 0) ? start_ns + interval_ns : 0;

	int wl_fd = wl_display_get_fd(app.display);
	while (app.running && !g_stop) {
		while (wl_display_prepare_read(app.display) != 0) {
			if (wl_display_dispatch_pending(app.display) < 0) {
				app.running = false;
				break;
			}
		}
		if (!app.running) {
			wl_display_cancel_read(app.display);
			break;
		}

		bool flush_more = false;
		if (wl_display_flush(app.display) < 0) {
			if (errno == EAGAIN) {
				flush_more = true;
			} else {
				wl_display_cancel_read(app.display);
				break;
			}
		}

		int64_t now = read_clock_ns(CLOCK_MONOTONIC);
		int64_t wake_ns = now + 250000000LL;
		if (end_ns > 0 && end_ns < wake_ns)
			wake_ns = end_ns;
		if (next_report_ns > 0 && next_report_ns < wake_ns)
			wake_ns = next_report_ns;
		int timeout_ms = (wake_ns > now) ? (int)((wake_ns - now + 999999LL) / 1000000LL) : 0;

		struct pollfd pfd = {
			.fd = wl_fd,
			.events = (short)(POLLIN | (flush_more ? POLLOUT : 0)),
		};
		int rc = poll(&pfd, 1, timeout_ms);
		if (rc > 0) {
			if (pfd.revents & POLLIN) {
				if (wl_display_read_events(app.display) < 0) {
					app.running = false;
					break;
				}
			} else {
				wl_display_cancel_read(app.display);
			}
			if (pfd.revents & (POLLERR | POLLHUP)) {
				app.running = false;
				break;
			}
			if (wl_display_dispatch_pending(app.display) < 0) {
				app.running = false;
				break;
			}
		} else {
			wl_display_cancel_read(app.display);
			if (rc < 0 && errno != EINTR) {
				app.running = false;
				break;
			}
		}

		if (app.want_frame && !app.frame_cb_pending && app.configured && app.running)
			submit_frame(&app);

		now = read_clock_ns(CLOCK_MONOTONIC);
		if (next_report_ns > 0 && now >= next_report_ns) {
			print_interval(&app, (double)(now - start_ns) / 1e9);
			while (next_report_ns <= now)
				next_report_ns += interval_ns;
		}
		if (end_ns > 0 && now >= end_ns)
			break;
	}

	/* Flush pending presentation feedback briefly if still connected. */
	if (wl_display_get_error(app.display) == 0)
		wl_display_roundtrip(app.display);

	int64_t stop_ns = read_clock_ns(CLOCK_MONOTONIC);
	print_summary(&app, (double)(stop_ns - start_ns) / 1e9);
	cleanup_app(&app);
	return 0;
}
