#!/usr/bin/env python3
"""
pzoom.py — measure zoom sharpness (> 1x) and downscale accuracy (< 1x).

Drives an Infynitae session across zooms 0.03, 0.06, 0.125, 0.25, 0.5, 0.75,
1.5, 2, 2.7, and 4 using compositor IPC `navigate`, or analyzes pre-captured
screenshots (for KDE / GNOME / Windows comparisons). Does not modify user
config or persistent session state.

Metrics:
  zoom > 1  : letter edge rise width in screen pixels (10% to 90% contrast).
              Target sharp text is < 1.5 px (median across letter edges).
  zoom < 1  : luminance difference (RMSE and max error) vs a CPU box-averaged
              1x reference crop, plus line-pattern std deviation (even grey vs
              aliasing/moire bands).

Run in an Infynitae Wayland session (requires `grim`, `gcc`, `pangocairo`):
  python3 tools/vm-edge/pzoom.py

Optional flags:
  python3 tools/vm-edge/pzoom.py --save-dir /tmp/pzoom --settle-ms 500
  python3 tools/vm-edge/pzoom.py --zooms 0.125,0.5,1.5,2,4

Manual comparison workflow (KDE / GNOME / Windows):
  1. Launch the test window only:
       python3 tools/vm-edge/pzoom.py --window-only
  2. Screenshot the window at 1x and at each zoomed view.
  3. Analyze saved PNGs:
       python3 tools/vm-edge/pzoom.py --analyze --ref-1x 1x.png \
         --zoom 0.25:z025.png --zoom 2:z2.png --zoom 4:z4.png

Offline self-test (no compositor required):
  python3 tools/vm-edge/pzoom.py --self-test
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time
import zlib
from pathlib import Path
from typing import Dict, List, Optional, Sequence, Tuple

DEFAULT_ZOOMS = [0.03, 0.06, 0.125, 0.25, 0.5, 0.75, 1.5, 2.0, 2.7, 4.0]
DEFAULT_WIN_W = 960
DEFAULT_WIN_H = 600
STRIPE_FRAC = 0.28
TEXT_ROI_FRAC = (0.20, 0.32, 0.80, 0.68)  # center region holding 10/12/16 px text

# Small Wayland+Cairo+Pango test window supporting wl_compositor v6
# (wl_surface.preferred_buffer_scale) so Infynitae's settle scale hints trigger
# sharp 2x/3x redraws while still working unchanged on KDE, GNOME, and Winit.
TEST_WINDOW_C = r"""
#define _GNU_SOURCE
#include "xdg-shell-protocol.h"

#include <cairo/cairo.h>
#include <errno.h>
#include <pango/pangocairo.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>

struct app_state {
	struct wl_display *display;
	struct wl_registry *registry;
	struct wl_compositor *compositor;
	uint32_t compositor_ver;
	struct wl_shm *shm;
	struct xdg_wm_base *wm_base;
	struct wl_surface *surface;
	struct xdg_surface *xdg_surface;
	struct xdg_toplevel *xdg_toplevel;

	int logical_w;
	int logical_h;
	int buffer_scale;
	bool configured;
	bool dirty;
	bool running;
	const char *app_id;

	struct wl_buffer *cur_buf;
};

static volatile sig_atomic_t g_stop = 0;

static void on_sig(int sig)
{
	(void)sig;
	g_stop = 1;
}

static void on_buf_release(void *data, struct wl_buffer *buf)
{
	struct app_state *st = (struct app_state *)data;
	if (st && st->cur_buf == buf)
		st->cur_buf = NULL;
	wl_buffer_destroy(buf);
}

static const struct wl_buffer_listener buf_listener = {
	.release = on_buf_release,
};

static void draw_and_commit(struct app_state *st)
{
	if (!st->configured || !st->dirty || !st->running)
		return;

	int scale = st->buffer_scale < 1 ? 1 : (st->buffer_scale > 8 ? 8 : st->buffer_scale);
	int w_px = st->logical_w * scale;
	int h_px = st->logical_h * scale;
	int stride = cairo_format_stride_for_width(CAIRO_FORMAT_RGB24, w_px);
	size_t size = (size_t)stride * (size_t)h_px;

	int fd = memfd_create("pzoom-win-shm", MFD_CLOEXEC);
	if (fd < 0 || ftruncate(fd, (off_t)size) < 0) {
		if (fd >= 0)
			close(fd);
		return;
	}

	uint8_t *mem = (uint8_t *)mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
	if (mem == MAP_FAILED) {
		close(fd);
		return;
	}

	struct wl_shm_pool *pool = wl_shm_create_pool(st->shm, fd, (int32_t)size);
	close(fd);
	if (!pool) {
		munmap(mem, size);
		return;
	}

	struct wl_buffer *buf = wl_shm_pool_create_buffer(pool, 0, w_px, h_px, stride, WL_SHM_FORMAT_XRGB8888);
	wl_shm_pool_destroy(pool);
	if (!buf) {
		munmap(mem, size);
		return;
	}
	wl_buffer_add_listener(buf, &buf_listener, st);

	cairo_surface_t *cs = cairo_image_surface_create_for_data(mem, CAIRO_FORMAT_RGB24, w_px, h_px, stride);
	cairo_t *cr = cairo_create(cs);

	/* Solid white background */
	cairo_set_source_rgb(cr, 1.0, 1.0, 1.0);
	cairo_paint(cr);

	cairo_scale(cr, (double)scale, (double)scale);

	int strip_h = (int)(st->logical_h * 0.28);
	cairo_save(cr);
	cairo_set_antialias(cr, CAIRO_ANTIALIAS_NONE);
	cairo_set_source_rgb(cr, 0.0, 0.0, 0.0);

	/* Top horizontal 1 logical px alternating stripes */
	for (int y = 0; y < strip_h; y += 2)
		cairo_rectangle(cr, 0, y, st->logical_w, 1);
	/* Bottom vertical 1 logical px alternating stripes */
	int bot_y = st->logical_h - strip_h;
	for (int x = 0; x < st->logical_w; x += 2)
		cairo_rectangle(cr, x, bot_y, 1, strip_h);
	cairo_fill(cr);
	cairo_restore(cr);

	/* Center white band with black text at 16, 12, and 10 px */
	const int sizes[3] = {16, 12, 10};
	const char *labels[3] = {
		"16px Hamburge 01234",
		"12px Hamburge 01234",
		"10px Hamburge 01234",
	};
	PangoLayout *layouts[3];
	int heights[3];
	int widths[3];
	int total_h = 0;
	const int gap = 12;

	for (int i = 0; i < 3; i++) {
		layouts[i] = pango_cairo_create_layout(cr);
		PangoFontDescription *desc = pango_font_description_new();
		pango_font_description_set_family(desc, "Sans");
		pango_font_description_set_absolute_size(desc, sizes[i] * PANGO_SCALE);
		pango_layout_set_font_description(layouts[i], desc);
		pango_font_description_free(desc);
		pango_layout_set_text(layouts[i], labels[i], -1);
		pango_layout_get_pixel_size(layouts[i], &widths[i], &heights[i]);
		total_h += heights[i];
	}
	total_h += 2 * gap;

	int cur_y = (st->logical_h - total_h) / 2;
	cairo_set_source_rgb(cr, 0.0, 0.0, 0.0);
	for (int i = 0; i < 3; i++) {
		int x = (st->logical_w - widths[i]) / 2;
		cairo_move_to(cr, x, cur_y);
		pango_cairo_show_layout(cr, layouts[i]);
		g_object_unref(layouts[i]);
		cur_y += heights[i] + gap;
	}

	cairo_surface_flush(cs);
	cairo_destroy(cr);
	cairo_surface_destroy(cs);
	munmap(mem, size);

	if (st->compositor_ver >= 3)
		wl_surface_set_buffer_scale(st->surface, scale);
	wl_surface_attach(st->surface, buf, 0, 0);
	if (st->compositor_ver >= 4)
		wl_surface_damage_buffer(st->surface, 0, 0, w_px, h_px);
	else
		wl_surface_damage(st->surface, 0, 0, st->logical_w, st->logical_h);
	wl_surface_commit(st->surface);

	st->cur_buf = buf;
	st->dirty = false;
}

static void on_surface_enter(void *data, struct wl_surface *s, struct wl_output *out)
{
	(void)data; (void)s; (void)out;
}

static void on_surface_leave(void *data, struct wl_surface *s, struct wl_output *out)
{
	(void)data; (void)s; (void)out;
}

static void on_surface_pref_scale(void *data, struct wl_surface *s, int32_t factor)
{
	(void)s;
	struct app_state *st = (struct app_state *)data;
	int clamped = factor < 1 ? 1 : (factor > 8 ? 8 : factor);
	if (clamped != st->buffer_scale) {
		st->buffer_scale = clamped;
		st->dirty = true;
		draw_and_commit(st);
	}
}

static void on_surface_pref_transform(void *data, struct wl_surface *s, uint32_t t)
{
	(void)data; (void)s; (void)t;
}

static const struct wl_surface_listener surface_listener = {
	.enter = on_surface_enter,
	.leave = on_surface_leave,
	.preferred_buffer_scale = on_surface_pref_scale,
	.preferred_buffer_transform = on_surface_pref_transform,
};

static void on_ping(void *data, struct xdg_wm_base *wm, uint32_t serial)
{
	(void)data;
	xdg_wm_base_pong(wm, serial);
}

static const struct xdg_wm_base_listener wm_listener = {
	.ping = on_ping,
};

static void on_xdg_surface_configure(void *data, struct xdg_surface *xs, uint32_t serial)
{
	struct app_state *st = (struct app_state *)data;
	xdg_surface_ack_configure(xs, serial);
	st->configured = true;
	st->dirty = true;
	draw_and_commit(st);
}

static const struct xdg_surface_listener xs_listener = {
	.configure = on_xdg_surface_configure,
};

static void on_toplevel_configure(void *data, struct xdg_toplevel *xt,
				  int32_t w, int32_t h, struct wl_array *states)
{
	(void)xt; (void)states;
	struct app_state *st = (struct app_state *)data;
	if (w > 0 && w != st->logical_w) {
		st->logical_w = w;
		st->dirty = true;
	}
	if (h > 0 && h != st->logical_h) {
		st->logical_h = h;
		st->dirty = true;
	}
}

static void on_toplevel_close(void *data, struct xdg_toplevel *xt)
{
	(void)xt;
	struct app_state *st = (struct app_state *)data;
	st->running = false;
}

static const struct xdg_toplevel_listener xt_listener = {
	.configure = on_toplevel_configure,
	.close = on_toplevel_close,
};

static void on_global(void *data, struct wl_registry *reg, uint32_t name,
		      const char *iface, uint32_t ver)
{
	struct app_state *st = (struct app_state *)data;
	if (strcmp(iface, wl_compositor_interface.name) == 0 && !st->compositor) {
		uint32_t v = ver < 6 ? ver : 6;
		st->compositor = (struct wl_compositor *)wl_registry_bind(reg, name, &wl_compositor_interface, v);
		st->compositor_ver = v;
	} else if (strcmp(iface, wl_shm_interface.name) == 0 && !st->shm) {
		st->shm = (struct wl_shm *)wl_registry_bind(reg, name, &wl_shm_interface, 1);
	} else if (strcmp(iface, xdg_wm_base_interface.name) == 0 && !st->wm_base) {
		st->wm_base = (struct xdg_wm_base *)wl_registry_bind(reg, name, &xdg_wm_base_interface, 1);
		xdg_wm_base_add_listener(st->wm_base, &wm_listener, st);
	}
}

static void on_global_remove(void *data, struct wl_registry *reg, uint32_t name)
{
	(void)data; (void)reg; (void)name;
}

static const struct wl_registry_listener reg_listener = {
	.global = on_global,
	.global_remove = on_global_remove,
};

int main(int argc, char **argv)
{
	struct app_state st;
	memset(&st, 0, sizeof(st));
	st.logical_w = 960;
	st.logical_h = 600;
	st.buffer_scale = 1;
	st.running = true;
	st.app_id = "infynitae.pzoom-test";

	for (int i = 1; i < argc; i++) {
		if (strcmp(argv[i], "--width") == 0 && i + 1 < argc)
			st.logical_w = atoi(argv[++i]);
		else if (strcmp(argv[i], "--height") == 0 && i + 1 < argc)
			st.logical_h = atoi(argv[++i]);
		else if (strcmp(argv[i], "--app-id") == 0 && i + 1 < argc)
			st.app_id = argv[++i];
	}
	if (st.logical_w < 64) st.logical_w = 64;
	if (st.logical_h < 64) st.logical_h = 64;

	struct sigaction sa;
	memset(&sa, 0, sizeof(sa));
	sa.sa_handler = on_sig;
	sigaction(SIGINT, &sa, NULL);
	sigaction(SIGTERM, &sa, NULL);

	st.display = wl_display_connect(NULL);
	if (!st.display) {
		fprintf(stderr, "pzoom-win: cannot connect to Wayland display\n");
		return 1;
	}
	st.registry = wl_display_get_registry(st.display);
	wl_registry_add_listener(st.registry, &reg_listener, &st);
	wl_display_roundtrip(st.display);

	if (!st.compositor || !st.shm || !st.wm_base) {
		fprintf(stderr, "pzoom-win: missing Wayland globals\n");
		return 1;
	}

	st.surface = wl_compositor_create_surface(st.compositor);
	wl_surface_add_listener(st.surface, &surface_listener, &st);
	st.xdg_surface = xdg_wm_base_get_xdg_surface(st.wm_base, st.surface);
	xdg_surface_add_listener(st.xdg_surface, &xs_listener, &st);
	st.xdg_toplevel = xdg_surface_get_toplevel(st.xdg_surface);
	xdg_toplevel_add_listener(st.xdg_toplevel, &xt_listener, &st);
	xdg_toplevel_set_title(st.xdg_toplevel, "pzoom-test");
	xdg_toplevel_set_app_id(st.xdg_toplevel, st.app_id);
	wl_surface_commit(st.surface);

	int wl_fd = wl_display_get_fd(st.display);
	while (st.running && !g_stop) {
		while (wl_display_prepare_read(st.display) != 0)
			wl_display_dispatch_pending(st.display);
		wl_display_flush(st.display);
		struct pollfd pfd = { .fd = wl_fd, .events = POLLIN };
		if (poll(&pfd, 1, 200) > 0) {
			wl_display_read_events(st.display);
			wl_display_dispatch_pending(st.display);
		} else {
			wl_display_cancel_read(st.display);
		}
		if (st.dirty && st.configured)
			draw_and_commit(&st);
	}

	if (st.xdg_toplevel) xdg_toplevel_destroy(st.xdg_toplevel);
	if (st.xdg_surface) xdg_surface_destroy(st.xdg_surface);
	if (st.surface) wl_surface_destroy(st.surface);
	if (st.wm_base) xdg_wm_base_destroy(st.wm_base);
	if (st.shm) wl_shm_destroy(st.shm);
	if (st.compositor) wl_compositor_destroy(st.compositor);
	if (st.registry) wl_registry_destroy(st.registry);
	wl_display_disconnect(st.display);
	return 0;
}
"""


class PngImage:
    """Minimal 8-bit grayscale/RGB/RGBA PNG container represented as luma floats [0..255]."""

    def __init__(self, width: int, height: int, luma: List[List[float]]) -> None:
        self.width = width
        self.height = height
        self.luma = luma

    def crop(self, x0: int, y0: int, x1: int, y1: int) -> "PngImage":
        cx0 = max(0, min(self.width, x0))
        cy0 = max(0, min(self.height, y0))
        cx1 = max(cx0, min(self.width, x1))
        cy1 = max(cy0, min(self.height, y1))
        rows = [self.luma[y][cx0:cx1] for y in range(cy0, cy1)]
        return PngImage(cx1 - cx0, cy1 - cy0, rows)


def _paeth(a: int, b: int, c: int) -> int:
    p = a + b - c
    pa = abs(p - a)
    pb = abs(p - b)
    pc = abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    if pb <= pc:
        return b
    return c


def _row_to_luma(row: bytearray, width: int, color_type: int) -> List[float]:
    if color_type == 0:
        return [float(row[x]) for x in range(width)]
    if color_type == 2:
        out = [0.0] * width
        idx = 0
        for x in range(width):
            out[x] = 0.299 * row[idx] + 0.587 * row[idx + 1] + 0.114 * row[idx + 2]
            idx += 3
        return out
    if color_type == 4:
        out = [0.0] * width
        idx = 0
        for x in range(width):
            g = row[idx]
            a = row[idx + 1] / 255.0
            out[x] = g * a + 255.0 * (1.0 - a)
            idx += 2
        return out
    if color_type == 6:
        out = [0.0] * width
        idx = 0
        for x in range(width):
            r = row[idx]
            g = row[idx + 1]
            b = row[idx + 2]
            a = row[idx + 3]
            if a == 255:
                out[x] = 0.299 * r + 0.587 * g + 0.114 * b
            else:
                af = a / 255.0
                lum = 0.299 * r + 0.587 * g + 0.114 * b
                out[x] = lum * af + 255.0 * (1.0 - af)
            idx += 4
        return out
    raise ValueError(f"unsupported PNG color_type={color_type}")


def read_png_luma(
    path: Path, roi: Optional[Tuple[int, int, int, int]] = None
) -> PngImage:
    """Decode an 8-bit non-interlaced PNG to luminance rows, optionally cropping while decoding."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError(f"{path}: invalid PNG signature")

    pos = 8
    width = height = bit_depth = color_type = interlace = 0
    idat_parts: List[bytes] = []

    while pos + 8 <= len(data):
        length, ctype = struct.unpack(">I4s", data[pos : pos + 8])
        pos += 8
        chunk = data[pos : pos + length]
        pos += length + 4  # skip CRC
        if ctype == b"IHDR":
            width, height, bit_depth, color_type, comp, filt, interlace = struct.unpack(
                ">IIBBBBB", chunk
            )
            if comp != 0 or filt != 0 or interlace != 0:
                raise ValueError(f"{path}: interlaced or non-standard PNG unsupported")
            if bit_depth != 8 or color_type not in (0, 2, 4, 6):
                raise ValueError(
                    f"{path}: requires 8-bit gray/RGB/RGBA PNG (got depth={bit_depth}, type={color_type})"
                )
        elif ctype == b"IDAT":
            idat_parts.append(chunk)
        elif ctype == b"IEND":
            break

    if width <= 0 or height <= 0:
        raise ValueError(f"{path}: missing IHDR")

    bpp = {0: 1, 2: 3, 4: 2, 6: 4}[color_type]
    stride = width * bpp
    raw = zlib.decompress(b"".join(idat_parts))

    if roi is not None:
        rx0 = max(0, min(width, roi[0]))
        ry0 = max(0, min(height, roi[1]))
        rx1 = max(rx0, min(width, roi[2]))
        ry1 = max(ry0, min(height, roi[3]))
    else:
        rx0, ry0, rx1, ry1 = 0, 0, width, height

    luma_rows: List[List[float]] = []
    prev = bytearray(stride)
    in_off = 0
    slice_w = rx1 - rx0
    b_start = rx0 * bpp
    b_end = rx1 * bpp

    for y in range(ry1):
        ftype = raw[in_off]
        in_off += 1
        cur = bytearray(raw[in_off : in_off + stride])
        in_off += stride

        if ftype == 0:
            pass
        elif ftype == 1:
            for i in range(bpp, stride):
                cur[i] = (cur[i] + cur[i - bpp]) & 0xFF
        elif ftype == 2:
            for i in range(stride):
                cur[i] = (cur[i] + prev[i]) & 0xFF
        elif ftype == 3:
            for i in range(bpp):
                cur[i] = (cur[i] + (prev[i] >> 1)) & 0xFF
            for i in range(bpp, stride):
                cur[i] = (cur[i] + ((cur[i - bpp] + prev[i]) >> 1)) & 0xFF
        elif ftype == 4:
            for i in range(bpp):
                cur[i] = (cur[i] + prev[i]) & 0xFF
            for i in range(bpp, stride):
                cur[i] = (cur[i] + _paeth(cur[i - bpp], prev[i], prev[i - bpp])) & 0xFF
        else:
            raise ValueError(f"{path}: invalid PNG filter type {ftype} at row {y}")

        if y >= ry0:
            sub = cur[b_start:b_end]
            luma_rows.append(_row_to_luma(sub, slice_w, color_type))
        prev = cur

    return PngImage(slice_w, ry1 - ry0, luma_rows)


def write_png_rgba(
    path: Path, width: int, height: int, pixels_rgb: Sequence[bytes], filter_mode: int = 0
) -> None:
    """Write a 24-bit RGB PNG (used by --self-test; filter_mode in 0..4)."""
    stride = width * 3
    prev = bytearray(stride)
    encoded = bytearray()

    for y in range(height):
        cur = bytearray(pixels_rgb[y])
        encoded.append(filter_mode)
        if filter_mode == 0:
            encoded.extend(cur)
        elif filter_mode == 1:
            for i in range(stride):
                a = cur[i - 3] if i >= 3 else 0
                encoded.append((cur[i] - a) & 0xFF)
        elif filter_mode == 2:
            for i in range(stride):
                encoded.append((cur[i] - prev[i]) & 0xFF)
        elif filter_mode == 3:
            for i in range(stride):
                a = cur[i - 3] if i >= 3 else 0
                encoded.append((cur[i] - ((a + prev[i]) >> 1)) & 0xFF)
        elif filter_mode == 4:
            for i in range(stride):
                a = cur[i - 3] if i >= 3 else 0
                b = prev[i]
                c = prev[i - 3] if i >= 3 else 0
                encoded.append((cur[i] - _paeth(a, b, c)) & 0xFF)
        else:
            raise ValueError(f"bad filter_mode {filter_mode}")
        prev = cur

    def chunk(ctype: bytes, payload: bytes) -> bytes:
        crc = zlib.crc32(ctype + payload) & 0xFFFFFFFF
        return struct.pack(">I", len(payload)) + ctype + payload + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    idat = zlib.compress(bytes(encoded), 6)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b""))


def _percentile(sorted_vals: Sequence[float], q: float) -> float:
    if not sorted_vals:
        return 0.0
    idx = int(round(q * (len(sorted_vals) - 1)))
    idx = max(0, min(len(sorted_vals) - 1, idx))
    return sorted_vals[idx]


def _scan_line_edges(
    line: Sequence[float], bg: float, fg: float, min_swing: float
) -> List[float]:
    """Return 10%-90% contrast transition widths (in pixels) along a 1D scanline."""
    n = len(line)
    if n < 3:
        return []
    widths: List[float] = []
    # Scan both falling (white -> black) and rising (black -> white) non-monotonic runs.
    for falling in (True, False):
        i = 0
        while i < n - 1:
            j = i
            if falling:
                while j + 1 < n and line[j + 1] <= line[j] + 1.0:
                    j += 1
            else:
                while j + 1 < n and line[j + 1] >= line[j] - 1.0:
                    j += 1

            a, b = i, j
            while a < b and abs(line[a + 1] - line[a]) <= 0.5:
                a += 1
            while b > a and abs(line[b] - line[b - 1]) <= 0.5:
                b -= 1

            if b > a:
                hi = line[a] if falling else line[b]
                lo = line[b] if falling else line[a]
                swing = hi - lo
                if swing >= min_swing and hi >= bg - 0.20 * (bg - fg) and lo <= fg + 0.35 * (bg - fg):
                    t90 = lo + 0.90 * swing
                    t10 = lo + 0.10 * swing
                    x90 = None
                    x10 = None
                    if falling:
                        for k in range(a, b):
                            if x90 is None and line[k] >= t90 > line[k + 1]:
                                x90 = k + (t90 - line[k]) / (line[k + 1] - line[k])
                            if line[k] > t10 >= line[k + 1]:
                                x10 = k + (t10 - line[k]) / (line[k + 1] - line[k])
                    else:
                        for k in range(a, b):
                            if x10 is None and line[k] <= t10 < line[k + 1]:
                                x10 = k + (t10 - line[k]) / (line[k + 1] - line[k])
                            if line[k] < t90 <= line[k + 1]:
                                x90 = k + (t90 - line[k]) / (line[k + 1] - line[k])
                    if x10 is not None and x90 is not None:
                        widths.append(abs(x90 - x10))
            i = j + 1
    return widths


def measure_edge_rise_width(img: PngImage) -> Dict[str, float]:
    """Measure 10%-90% letter edge transition width across rows and columns."""
    if img.width < 3 or img.height < 3:
        return {"count": 0, "median": float("nan"), "mean": float("nan"), "p90": float("nan")}

    all_vals = sorted(v for row in img.luma for v in row)
    bg = _percentile(all_vals, 0.95)
    fg = _percentile(all_vals, 0.01)
    total_range = bg - fg
    if total_range < 50.0:
        return {"count": 0, "median": float("nan"), "mean": float("nan"), "p90": float("nan")}

    min_swing = 0.65 * total_range
    widths: List[float] = []
    for y in range(img.height):
        widths.extend(_scan_line_edges(img.luma[y], bg, fg, min_swing))
    for x in range(img.width):
        col = [img.luma[y][x] for y in range(img.height)]
        widths.extend(_scan_line_edges(col, bg, fg, min_swing))

    if not widths:
        return {"count": 0, "median": float("nan"), "mean": float("nan"), "p90": float("nan")}

    widths.sort()
    return {
        "count": len(widths),
        "median": _percentile(widths, 0.50),
        "mean": sum(widths) / len(widths),
        "p90": _percentile(widths, 0.90),
    }


def _box_weights_1d(src_len: int, dst_len: int) -> List[List[Tuple[int, float]]]:
    """Exact fractional pixel coverage mapping src_len -> dst_len."""
    scale = src_len / float(dst_len)
    mapping: List[List[Tuple[int, float]]] = []
    for d in range(dst_len):
        s0 = d * scale
        s1 = (d + 1) * scale
        span = s1 - s0
        i0 = max(0, int(math.floor(s0)))
        i1 = min(src_len - 1, int(math.ceil(s1)) - 1)
        items: List[Tuple[int, float]] = []
        for s in range(i0, i1 + 1):
            ov = min(s + 1.0, s1) - max(float(s), s0)
            if ov > 0.0:
                items.append((s, ov / span))
        mapping.append(items)
    return mapping


def area_downsample(img: PngImage, dst_w: int, dst_h: int) -> PngImage:
    """Separable exact continuous box-filter downsample from 1x reference onto (dst_w, dst_h)."""
    if dst_w <= 0 or dst_h <= 0:
        raise ValueError(f"invalid target size {dst_w}x{dst_h}")
    wx = _box_weights_1d(img.width, dst_w)
    wy = _box_weights_1d(img.height, dst_h)

    horiz: List[List[float]] = []
    for y in range(img.height):
        src_row = img.luma[y]
        h_row = [sum(src_row[sx] * w for sx, w in col_map) for col_map in wx]
        horiz.append(h_row)

    out_rows: List[List[float]] = []
    for row_map in wy:
        out_row = [0.0] * dst_w
        for sy, w in row_map:
            h_row = horiz[sy]
            for dx in range(dst_w):
                out_row[dx] += h_row[dx] * w
        out_rows.append(out_row)

    return PngImage(dst_w, dst_h, out_rows)


def compare_downscale(actual: PngImage, ref_1x: PngImage) -> Dict[str, float]:
    """Compare a zoomed-out screenshot against CPU area-averaged 1x reference."""
    if actual.width <= 0 or actual.height <= 0:
        return {"rmse": float("nan"), "max_diff": float("nan"), "pattern_std": float("nan")}

    cpu_ref = area_downsample(ref_1x, actual.width, actual.height)

    # Trim 1 px border if large enough to avoid outer compositor background blend.
    mx = 1 if actual.width >= 6 else 0
    my = 1 if actual.height >= 6 else 0
    sq_err = 0.0
    max_diff = 0.0
    count = 0
    for y in range(my, actual.height - my):
        a_row = actual.luma[y]
        r_row = cpu_ref.luma[y]
        for x in range(mx, actual.width - mx):
            diff = abs(a_row[x] - r_row[x])
            sq_err += diff * diff
            if diff > max_diff:
                max_diff = diff
            count += 1

    rmse = math.sqrt(sq_err / count) if count > 0 else float("nan")

    # Measure band unevenness inside top & bottom 1 px stripe strips (well away from text boundary).
    strip_px_h = int(math.floor(actual.height * STRIPE_FRAC))
    pat_vals: List[float] = []
    top_y0 = 1 if strip_px_h >= 4 else 0
    top_y1 = max(top_y0, strip_px_h - 1)
    bot_y0 = min(actual.height, actual.height - strip_px_h + 1)
    bot_y1 = actual.height - (1 if actual.height >= 6 else 0)
    for y_start, y_end in ((top_y0, top_y1), (bot_y0, bot_y1)):
        for y in range(y_start, y_end):
            pat_vals.extend(actual.luma[y][mx : actual.width - mx])

    if pat_vals:
        mean_p = sum(pat_vals) / len(pat_vals)
        pat_std = math.sqrt(sum((v - mean_p) ** 2 for v in pat_vals) / len(pat_vals))
    else:
        pat_std = float("nan")

    return {"rmse": rmse, "max_diff": max_diff, "pattern_std": pat_std}


def map_logical_roi_to_screen(
    win_rect: Tuple[float, float, float, float],
    win_w: int,
    win_h: int,
    roi_frac: Tuple[float, float, float, float],
    scr_w: int,
    scr_h: int,
) -> Tuple[int, int, int, int]:
    """Map normalized [fx0, fy0, fx1, fy1] within a window's projected rect to screen pixels."""
    wx0, wy0, wx1, wy1 = win_rect
    span_x = wx1 - wx0
    span_y = wy1 - wy0
    fx0, fy0, fx1, fy1 = roi_frac
    sx0 = int(max(0, math.floor(wx0 + fx0 * span_x)))
    sy0 = int(max(0, math.floor(wy0 + fy0 * span_y)))
    sx1 = int(min(scr_w, math.ceil(wx0 + fx1 * span_x)))
    sy1 = int(min(scr_h, math.ceil(wy0 + fy1 * span_y)))
    return (sx0, sy0, max(sx0, sx1), max(sy0, sy1))


class ShellClient:
    """Line-delimited JSON IPC client for Infynitae shell.sock."""

    def __init__(self, sock_path: Path) -> None:
        self.sock_path = sock_path
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(5.0)
        self.sock.connect(str(sock_path))
        self.f = self.sock.makefile("r", encoding="utf-8")
        self.next_id = 1

    def call(self, payload: Dict[str, object]) -> Dict[str, object]:
        rid = self.next_id
        self.next_id += 1
        req = dict(payload)
        req["id"] = rid
        self.sock.sendall((json.dumps(req) + "\n").encode("utf-8"))
        while True:
            line = self.f.readline()
            if not line:
                raise RuntimeError(f"EOF waiting for reply on {self.sock_path}")
            msg = json.loads(line)
            if msg.get("id") == rid:
                if not msg.get("ok", False):
                    raise RuntimeError(f"IPC {req.get('cmd')} failed: {msg.get('error')}")
                return msg

    def snapshot(self) -> Dict[str, object]:
        return self.call({"cmd": "snapshot"})["snapshot"]  # type: ignore[return-value]

    def layers(self) -> Dict[str, object]:
        return self.call({"cmd": "layers"})["snapshot"]  # type: ignore[return-value]

    def navigate(
        self,
        output_id: int,
        zoom: float,
        element: Optional[List[int]] = None,
        canvas: Optional[List[float]] = None,
    ) -> None:
        to: Dict[str, object]
        if element is not None:
            to = {"element": element}
        elif canvas is not None:
            to = {"canvas": canvas}
        else:
            raise ValueError("navigate requires element or canvas")
        self.call({"cmd": "navigate", "output": output_id, "to": to, "zoom": zoom})

    def close(self) -> None:
        try:
            self.f.close()
        finally:
            self.sock.close()


def _find_xdg_shell_proto(build_dir: Path) -> Path:
    repo_proto = Path(__file__).resolve().parents[1] / "frame-probe" / "xdg-shell-protocol.c"
    if repo_proto.exists():
        # Header sits alongside frame-probe/xdg-shell-protocol.c.
        return repo_proto
    xml = Path("/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml")
    if xml.exists() and shutil.which("wayland-scanner"):
        hdr = build_dir / "xdg-shell-protocol.h"
        src = build_dir / "xdg-shell-protocol.c"
        subprocess.run(["wayland-scanner", "client-header", str(xml), str(hdr)], check=True)
        subprocess.run(["wayland-scanner", "private-code", str(xml), str(src)], check=True)
        return src
    raise RuntimeError("xdg-shell-protocol.c not found in tools/frame-probe and wayland-protocols unavailable")


def compile_test_window() -> Path:
    """Compile (cached under /tmp) the Wayland+Cairo+Pango test window binary."""
    digest = hashlib.sha256(TEST_WINDOW_C.encode("utf-8")).hexdigest()[:12]
    build_dir = Path(tempfile.gettempdir()) / f"pzoom-win-{os.getuid()}-{digest}"
    bin_path = build_dir / "pzoom-win"
    if bin_path.exists():
        return bin_path

    build_dir.mkdir(parents=True, exist_ok=True)
    src_path = build_dir / "pzoom-win.c"
    src_path.write_text(TEST_WINDOW_C, encoding="utf-8")
    xdg_c = _find_xdg_shell_proto(build_dir)

    cflags = subprocess.check_output(
        ["pkg-config", "--cflags", "wayland-client", "pangocairo"], text=True
    ).split()
    libs = subprocess.check_output(
        ["pkg-config", "--libs", "wayland-client", "pangocairo"], text=True
    ).split()

    tmp_bin = build_dir / f"pzoom-win.{os.getpid()}"
    cmd = [
        "gcc",
        "-O2",
        "-Wall",
        "-Wextra",
        "-I",
        str(xdg_c.parent),
        *cflags,
        str(src_path),
        str(xdg_c),
        "-o",
        str(tmp_bin),
        *libs,
    ]
    subprocess.run(cmd, check=True)
    tmp_bin.replace(bin_path)
    return bin_path


def _default_socket_path() -> Path:
    rt = os.environ.get("XDG_RUNTIME_DIR") or f"/run/user/{os.getuid()}"
    return Path(rt) / "infynitae" / "shell.sock"


def _resolve_drawn_rect(
    client: ShellClient,
    snap_out: Dict[str, object],
    elem_id: List[int],
    win_anchor: List[float],
    win_size: List[float],
) -> Tuple[float, float, float, float]:
    out_id = snap_out["id"]
    try:
        lay = client.layers()
        for w in lay.get("windows", []):
            if w.get("id") == elem_id:
                for d in w.get("drawn", []):
                    if d.get("output") == out_id and "rect" in d:
                        r = d["rect"]
                        return (float(r[0]), float(r[1]), float(r[2]), float(r[3]))
    except Exception:
        pass

    cam = snap_out["camera"]
    cx, cy = cam["center"]  # type: ignore[misc]
    zoom = float(cam["zoom"])  # type: ignore[arg-type]
    scale = float(snap_out["scale"])  # type: ignore[arg-type]
    mw, mh = snap_out["mode"]  # type: ignore[misc]
    k = zoom * scale
    x0 = (win_anchor[0] - cx) * k + mw * 0.5
    y0 = (win_anchor[1] - cy) * k + mh * 0.5
    x1 = (win_anchor[0] + win_size[0] - cx) * k + mw * 0.5
    y1 = (win_anchor[1] + win_size[1] - cy) * k + mh * 0.5
    return (x0, y0, x1, y1)


def _wait_for_zoom(client: ShellClient, out_id: int, target_zoom: float, timeout_s: float = 3.0) -> Dict[str, object]:
    deadline = time.monotonic() + timeout_s
    last_out: Dict[str, object] = {}
    while True:
        snap = client.snapshot()
        for o in snap["outputs"]:  # type: ignore[attr-defined]
            if o["id"] == out_id:
                last_out = o
                cam = o.get("camera")
                if cam and abs(float(cam["zoom"]) - target_zoom) <= max(1e-4, target_zoom * 1e-3):
                    return o
        if time.monotonic() >= deadline:
            return last_out
        time.sleep(0.03)


def run_live(args: argparse.Namespace) -> int:
    if not shutil.which("grim"):
        print("pzoom: missing 'grim' in PATH", file=sys.stderr)
        return 1
    sock_path = Path(args.socket) if args.socket else _default_socket_path()
    if not sock_path.exists():
        print(f"pzoom: shell socket not found: {sock_path}", file=sys.stderr)
        return 1

    win_bin = compile_test_window()
    client = ShellClient(sock_path)
    app_id = f"infynitae.pzoom-test-{os.getpid()}"
    tmp_dir_obj = None
    if args.save_dir:
        shot_dir = Path(args.save_dir)
        shot_dir.mkdir(parents=True, exist_ok=True)
    else:
        tmp_dir_obj = tempfile.TemporaryDirectory(prefix="pzoom-shots-")
        shot_dir = Path(tmp_dir_obj.name)

    proc: Optional[subprocess.Popen[bytes]] = None
    orig_cam: Optional[Tuple[int, List[float], float]] = None

    try:
        snap0 = client.snapshot()
        outputs = snap0["outputs"]  # type: ignore[index]
        if not outputs:
            raise RuntimeError("no outputs reported by snapshot")
        target_out = outputs[0]
        if args.output:
            matches = [o for o in outputs if o["name"] == args.output or str(o["id"]) == args.output]
            if not matches:
                raise RuntimeError(f"output '{args.output}' not found")
            target_out = matches[0]
        out_id = int(target_out["id"])
        out_name = str(target_out["name"])
        if target_out.get("camera"):
            cam0 = target_out["camera"]
            orig_cam = (out_id, list(cam0["center"]), float(cam0["zoom"]))

        win_w = min(args.width, int(target_out["mode"][0]) - 64) if args.width == DEFAULT_WIN_W else args.width
        win_h = min(args.height, int(target_out["mode"][1]) - 64) if args.height == DEFAULT_WIN_H else args.height
        win_w = max(256, win_w)
        win_h = max(256, win_h)

        proc = subprocess.Popen(
            [str(win_bin), "--width", str(win_w), "--height", str(win_h), "--app-id", app_id]
        )

        elem: Optional[Dict[str, object]] = None
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline:
            snap = client.snapshot()
            for el in snap["elements"]:  # type: ignore[index]
                if el.get("app_id") == app_id and el.get("state") == "mapped" and el.get("size"):
                    elem = el
                    break
            if elem is not None:
                break
            if proc.poll() is not None:
                raise RuntimeError(f"test window exited early (code={proc.returncode})")
            time.sleep(0.05)

        if elem is None:
            raise RuntimeError("timed out waiting for pzoom test window to map")

        elem_id: List[int] = elem["id"]  # type: ignore[assignment]
        elem_anchor: List[float] = elem["anchor"]  # type: ignore[assignment]
        elem_size: List[float] = elem["size"]  # type: ignore[assignment]

        # 1.0x baseline capture for CPU-averaged zoom < 1 comparisons.
        client.navigate(out_id, zoom=1.0, element=elem_id)
        out_1x = _wait_for_zoom(client, out_id, 1.0)
        time.sleep(args.settle_ms / 1000.0)
        png_1x = shot_dir / "z_1.000.png"
        subprocess.run(["grim", "-o", out_name, str(png_1x)], check=True)
        rect_1x = _resolve_drawn_rect(client, out_1x, elem_id, elem_anchor, elem_size)
        scr_w, scr_h = int(out_1x["mode"][0]), int(out_1x["mode"][1])
        ref_1x = read_png_luma(
            png_1x,
            roi=(
                max(0, int(round(rect_1x[0]))),
                max(0, int(round(rect_1x[1]))),
                min(scr_w, int(round(rect_1x[2]))),
                min(scr_h, int(round(rect_1x[3]))),
            ),
        )
        ref_text = read_png_luma(
            png_1x,
            roi=map_logical_roi_to_screen(
                rect_1x, int(elem_size[0]), int(elem_size[1]), TEXT_ROI_FRAC, scr_w, scr_h
            ),
        )
        base_edge = measure_edge_rise_width(ref_text)
        print(
            f"baseline zoom=1.000  window={ref_1x.width}x{ref_1x.height}px  "
            f"edge_10_90(median={base_edge['median']:.2f}px p90={base_edge['p90']:.2f}px n={int(base_edge['count'])})"
        )

        for z in args.zooms:
            client.navigate(out_id, zoom=z, element=elem_id)
            out_z = _wait_for_zoom(client, out_id, z)
            time.sleep(args.settle_ms / 1000.0)

            png_z = shot_dir / f"z_{z:.3f}.png"
            subprocess.run(["grim", "-o", out_name, str(png_z)], check=True)
            rect_z = _resolve_drawn_rect(client, out_z, elem_id, elem_anchor, elem_size)
            scr_w, scr_h = int(out_z["mode"][0]), int(out_z["mode"][1])

            if z > 1.0:
                roi = map_logical_roi_to_screen(
                    rect_z, int(elem_size[0]), int(elem_size[1]), TEXT_ROI_FRAC, scr_w, scr_h
                )
                crop = read_png_luma(png_z, roi=roi)
                m = measure_edge_rise_width(crop)
                flag = "OK" if (not math.isnan(m["median"]) and m["median"] < 1.5) else "SOFT/WIDE"
                print(
                    f"zoom={z:6.3f}  edge_10_90: median={m['median']:5.2f}px  "
                    f"mean={m['mean']:5.2f}px  p90={m['p90']:5.2f}px  edges={int(m['count']):4d}  [{flag}]"
                )
            elif z < 1.0:
                roi = (
                    max(0, int(round(rect_z[0]))),
                    max(0, int(round(rect_z[1]))),
                    min(scr_w, int(round(rect_z[2]))),
                    min(scr_h, int(round(rect_z[3]))),
                )
                crop = read_png_luma(png_z, roi=roi)
                m = compare_downscale(crop, ref_1x)
                print(
                    f"zoom={z:6.3f}  downscale({crop.width}x{crop.height}): "
                    f"cpu_rmse={m['rmse']:5.2f} ({m['rmse']/2.55:4.1f}%)  "
                    f"max_diff={m['max_diff']:5.1f}  pattern_std={m['pattern_std']:5.2f}"
                )
    finally:
        if proc is not None and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=2.0)
            except subprocess.TimeoutExpired:
                proc.kill()
        if orig_cam is not None:
            try:
                client.navigate(orig_cam[0], zoom=orig_cam[2], canvas=orig_cam[1])
            except Exception:
                pass
        client.close()
        if tmp_dir_obj is not None:
            tmp_dir_obj.cleanup()

    return 0


def run_analyze(args: argparse.Namespace) -> int:
    roi = None
    if args.roi:
        parts = [int(x) for x in args.roi.split(",")]
        if len(parts) != 4:
            print("pzoom: --roi expects x,y,w,h", file=sys.stderr)
            return 2
        roi = (parts[0], parts[1], parts[0] + parts[2], parts[1] + parts[3])

    ref_1x = read_png_luma(Path(args.ref_1x), roi=roi) if args.ref_1x else None
    if ref_1x is not None:
        base_edge = measure_edge_rise_width(ref_1x)
        print(
            f"ref-1x ({ref_1x.width}x{ref_1x.height}): "
            f"edge_10_90 median={base_edge['median']:.2f}px p90={base_edge['p90']:.2f}px"
        )

    for spec in args.zoom:
        if ":" not in spec:
            print(f"pzoom: --zoom expects ZOOM:PATH, got '{spec}'", file=sys.stderr)
            return 2
        z_str, p_str = spec.split(":", 1)
        z = float(z_str)
        img = read_png_luma(Path(p_str), roi=roi)
        if z > 1.0:
            m = measure_edge_rise_width(img)
            print(
                f"zoom={z:6.3f}  edge_10_90: median={m['median']:5.2f}px  "
                f"mean={m['mean']:5.2f}px  p90={m['p90']:5.2f}px  edges={int(m['count']):4d}"
            )
        elif z < 1.0:
            if ref_1x is None:
                print(f"pzoom: --ref-1x is required to evaluate zoom={z}", file=sys.stderr)
                return 2
            m = compare_downscale(img, ref_1x)
            print(
                f"zoom={z:6.3f}  downscale({img.width}x{img.height}): "
                f"cpu_rmse={m['rmse']:5.2f}  max_diff={m['max_diff']:5.1f}  pattern_std={m['pattern_std']:5.2f}"
            )
        else:
            m = measure_edge_rise_width(img)
            print(f"zoom={z:6.3f}  edge_10_90: median={m['median']:5.2f}px")
    return 0


def run_self_test() -> int:
    with tempfile.TemporaryDirectory(prefix="pzoom-test-") as td:
        tdir = Path(td)

        # 1. PNG round-trip across all 5 PNG filter modes (0..4).
        w, h = 64, 48
        rows_rgb = [
            bytes(v for x in range(w) for v in ((x * 7 + y * 13) & 0xFF, (x * 3 + y * 5) & 0xFF, (x * 11 + y) & 0xFF))
            for y in range(h)
        ]
        for fm in range(5):
            p = tdir / f"f{fm}.png"
            write_png_rgba(p, w, h, rows_rgb, filter_mode=fm)
            dec = read_png_luma(p)
            assert dec.width == w and dec.height == h
            for y in range(h):
                for x in range(w):
                    r, g, b = rows_rgb[y][3 * x : 3 * x + 3]
                    exp = 0.299 * r + 0.587 * g + 0.114 * b
                    assert abs(dec.luma[y][x] - exp) < 1e-6

        # 2. Edge sharpness metric (> 1x): sharp step vs blurred 4x vs nearest-upscaled AA.
        sharp = PngImage(
            40,
            20,
            [[0.0 if (15 <= x < 22 and 4 <= y < 16) else 255.0 for x in range(40)] for y in range(20)],
        )
        m_sharp = measure_edge_rise_width(sharp)
        assert m_sharp["count"] > 0 and m_sharp["median"] < 1.0, m_sharp

        # Bilinear 4x blurred ramp over 5 pixels: [255, 204, 153, 102, 51, 0, ...]
        ramp_row = [255.0] * 8 + [204.0, 153.0, 102.0, 51.0] + [0.0] * 10 + [51.0, 102.0, 153.0, 204.0] + [255.0] * 8
        blur = PngImage(len(ramp_row), 20, [ramp_row[:] if 4 <= y < 16 else [255.0] * len(ramp_row) for y in range(20)])
        m_blur = measure_edge_rise_width(blur)
        assert m_blur["count"] > 0 and m_blur["median"] > 3.5, m_blur

        # Nearest-neighbor upscaled 1x antialiased stem has a flat mid-grey step plateau.
        stair_row = [255.0] * 6 + [128.0] * 3 + [0.0] * 6 + [128.0] * 3 + [255.0] * 6
        stair = PngImage(len(stair_row), 20, [stair_row[:] if 4 <= y < 16 else [255.0] * len(stair_row) for y in range(20)])
        m_stair = measure_edge_rise_width(stair)
        assert m_stair["count"] > 0 and m_stair["median"] > 3.0, m_stair

        # 3. Downscale accuracy metric (< 1x): CPU average turns 1 px stripes to even grey; aliasing bands fail.
        rw, rh = 320, 200
        strip = int(rh * STRIPE_FRAC)
        pat_1x: List[List[float]] = []
        for y in range(rh):
            if y < strip:
                pat_1x.append([0.0 if (y % 2 == 0) else 255.0] * rw)
            elif y >= rh - strip:
                pat_1x.append([0.0 if (x % 2 == 0) else 255.0 for x in range(rw)])
            else:
                pat_1x.append([255.0] * rw)
        ref = PngImage(rw, rh, pat_1x)
        even_down = area_downsample(ref, 40, 25)
        m_even = compare_downscale(even_down, ref)
        assert m_even["rmse"] < 1e-6 and m_even["pattern_std"] < 1.0, m_even

        # Simulate nearest-neighbor moire/banding at 1/8x (sampling every 8th pixel hits even rows -> all black).
        aliased = PngImage(40, 25, [[pat_1x[y * 8][x * 8] for x in range(40)] for y in range(25)])
        m_alias = compare_downscale(aliased, ref)
        assert m_alias["rmse"] > 50.0, m_alias

        # 4. Screen ROI mapping clipped safely when window extends past output bounds at 4x.
        roi = map_logical_roi_to_screen((-1440.0, -900.0, 2400.0, 1500.0), 960, 600, TEXT_ROI_FRAC, 1280, 800)
        assert roi[0] >= 0 and roi[1] >= 0 and roi[2] <= 1280 and roi[3] <= 800 and roi[2] > roi[0]

        # 5. Offline analyze mode end-to-end on generated PNGs.
        ref_path = tdir / "ref.png"
        sharp_path = tdir / "z2.png"
        down_path = tdir / "z0125.png"
        to_rgb_rows = lambda lum: [
            bytes(v for x in range(lum.width) for v in (int(round(lum.luma[y][x])),) * 3)
            for y in range(lum.height)
        ]
        write_png_rgba(ref_path, ref.width, ref.height, to_rgb_rows(ref), filter_mode=1)
        write_png_rgba(sharp_path, sharp.width, sharp.height, to_rgb_rows(sharp), filter_mode=0)
        write_png_rgba(down_path, even_down.width, even_down.height, to_rgb_rows(even_down), filter_mode=4)
        rc = run_analyze(
            argparse.Namespace(
                ref_1x=str(ref_path),
                zoom=[f"0.125:{down_path}", f"2:{sharp_path}"],
                roi=None,
            )
        )
        assert rc == 0

        # 6. ShellClient Unix socket JSON request/reply wire contract.
        import threading

        sock_file = tdir / "shell.sock"
        srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        srv.bind(str(sock_file))
        srv.listen(1)
        seen_cmds: List[Dict[str, object]] = []

        def serve() -> None:
            conn, _ = srv.accept()
            with conn:
                f = conn.makefile("r", encoding="utf-8")
                for line in f:
                    req = json.loads(line)
                    seen_cmds.append(req)
                    reply: Dict[str, object] = {"id": req["id"], "ok": True, "generation": len(seen_cmds)}
                    if req["cmd"] == "snapshot":
                        reply["snapshot"] = {"outputs": [], "elements": []}
                    elif req["cmd"] == "layers":
                        reply["snapshot"] = {"windows": []}
                    conn.sendall((json.dumps(reply) + "\n").encode("utf-8"))

        th = threading.Thread(target=serve, daemon=True)
        th.start()
        cli = ShellClient(sock_file)
        assert cli.snapshot() == {"outputs": [], "elements": []}
        assert cli.layers() == {"windows": []}
        cli.navigate(output_id=1, zoom=2.7, element=[42, 7])
        cli.close()
        th.join(timeout=2.0)
        srv.close()
        assert seen_cmds[2]["cmd"] == "navigate" and seen_cmds[2]["zoom"] == 2.7
        assert seen_cmds[2]["to"] == {"element": [42, 7]}

    print("pzoom.py self-test passed (PNG filters 0..4, edge width, downscale diff, analyze, IPC)")
    return 0


def parse_zooms(raw: str) -> List[float]:
    return [float(part.strip()) for part in raw.split(",") if part.strip()]


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Measure zoom text sharpness (>1x) and downscale fidelity (<1x)."
    )
    parser.add_argument("--socket", help="Path to Infynitae shell.sock (default: $XDG_RUNTIME_DIR/infynitae/shell.sock)")
    parser.add_argument("--output", help="Target output name or id (default: first output)")
    parser.add_argument("--zooms", type=parse_zooms, default=DEFAULT_ZOOMS, help="Comma-separated zooms to measure")
    parser.add_argument("--settle-ms", type=int, default=500, help="Wait time after camera reaches target zoom (ms)")
    parser.add_argument("--width", type=int, default=DEFAULT_WIN_W, help="Test window logical width")
    parser.add_argument("--height", type=int, default=DEFAULT_WIN_H, help="Test window logical height")
    parser.add_argument("--save-dir", help="Keep captured screenshots in DIR instead of deleting them")
    parser.add_argument("--window-only", action="store_true", help="Compile and show only the test window (for manual KDE/Windows capture)")
    parser.add_argument("--analyze", action="store_true", help="Analyze pre-captured screenshots instead of running live IPC")
    parser.add_argument("--ref-1x", help="Path to 1x reference PNG (used with --analyze)")
    parser.add_argument("--zoom", action="append", default=[], help="ZOOM:PATH entry (repeatable, used with --analyze)")
    parser.add_argument("--roi", help="Crop x,y,w,h before analyzing PNGs (used with --analyze)")
    parser.add_argument("--self-test", action="store_true", help="Run built-in offline unit tests")
    args = parser.parse_args()

    if args.self_test:
        return run_self_test()
    if args.window_only:
        win_bin = compile_test_window()
        return subprocess.call([str(win_bin), "--width", str(args.width), "--height", str(args.height)])
    if args.analyze:
        return run_analyze(args)
    return run_live(args)


if __name__ == "__main__":
    raise SystemExit(main())
