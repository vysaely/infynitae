/*
 * Screen sharing for Infynitae's portal: what org.freedesktop.impl.portal.ScreenCast streams
 * (remote-desktop.c answers the calls). Before 2026-11-25 xdg-desktop-portal-wlr did this and ran
 * all session (43 MB on the owner's PC); now the portal does it itself.
 *
 * A source is picked with live/screenshare-chooser ("Monitor: DP-1" or "Window: title (app)"
 * lines). Its frames come from the compositor (ext-image-copy-capture: a screen, or one window of
 * ext-foreign-toplevel-list), straight into the buffers of a PipeWire stream: GPU buffers
 * (dmabuf, linear, the only kind the compositor offers) when the app takes them, so the
 * compositor draws into them on the GPU; shared memory otherwise, which the compositor fills
 * with a copy on its main thread. The compositor keeps private windows out and shares no
 * window while locked; it sends a frame only when something changed, and at most as often as
 * the app asked (maxFramerate).
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <gbm.h>
#include <glib-unix.h>
#include <pipewire/pipewire.h>
#include <spa/buffer/meta.h>
#include <spa/param/video/format-utils.h>
#include <spa/pod/builder.h>
#include <string.h>
#include <sys/mman.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xf86drm.h>

#include "ext-foreign-toplevel-list-v1-protocol.h"
#include "ext-image-capture-source-v1-protocol.h"
#include "ext-image-copy-capture-v1-protocol.h"
#include "linux-dmabuf-v1-protocol.h"
#include "screencast.h"
#include "xdg-output-protocol.h"

#define DRM_FORMAT_XRGB8888 0x34325258
#define MOD_LINEAR UINT64_C(0)
#define DEFAULT_FPS 60

/* ---- the compositor: screens, windows, capture -------------------------------------------- */

typedef struct {
    uint32_t name;
    struct wl_output *output;
    struct zxdg_output_v1 *xdg;
    char *connector;
    int32_t x, y, w, h; /* logical px */
} Screen;

typedef struct {
    struct ext_foreign_toplevel_handle_v1 *handle;
    char *title, *app_id;
} Window;

static struct wl_display *wl;
static struct wl_shm *shm;
static struct zwp_linux_dmabuf_v1 *linux_dmabuf;
static struct zxdg_output_manager_v1 *xdg_outputs;
static struct ext_image_copy_capture_manager_v1 *copy_manager;
static struct ext_output_image_capture_source_manager_v1 *screen_sources;
static struct ext_foreign_toplevel_image_capture_source_manager_v1 *window_sources;
static struct ext_foreign_toplevel_list_v1 *toplevels;
static GList *screens; /* Screen* */
static GList *windows; /* Window* */

static void xdg_position(void *data, struct zxdg_output_v1 *x, int32_t px, int32_t py) {
    ((Screen *)data)->x = px;
    ((Screen *)data)->y = py;
}
static void xdg_size(void *data, struct zxdg_output_v1 *x, int32_t w, int32_t h) {
    ((Screen *)data)->w = w;
    ((Screen *)data)->h = h;
}
static void xdg_name(void *data, struct zxdg_output_v1 *x, const char *name) {
    g_free(((Screen *)data)->connector);
    ((Screen *)data)->connector = g_strdup(name);
}
static void xdg_noop(void *data, struct zxdg_output_v1 *x) {}
static void xdg_str(void *data, struct zxdg_output_v1 *x, const char *s) {}
static const struct zxdg_output_v1_listener xdg_listener = { xdg_position, xdg_size, xdg_noop, xdg_name, xdg_str };

static void screen_watch(Screen *s) {
    if (xdg_outputs && !s->xdg) {
        s->xdg = zxdg_output_manager_v1_get_xdg_output(xdg_outputs, s->output);
        zxdg_output_v1_add_listener(s->xdg, &xdg_listener, s);
    }
}

static void window_closed(void *data, struct ext_foreign_toplevel_handle_v1 *h) {
    Window *w = data;
    windows = g_list_remove(windows, w);
    ext_foreign_toplevel_handle_v1_destroy(h);
    g_free(w->title);
    g_free(w->app_id);
    g_free(w);
}
static void window_done(void *data, struct ext_foreign_toplevel_handle_v1 *h) {}
static void window_title(void *data, struct ext_foreign_toplevel_handle_v1 *h, const char *t) {
    g_free(((Window *)data)->title);
    ((Window *)data)->title = g_strdup(t);
}
static void window_app_id(void *data, struct ext_foreign_toplevel_handle_v1 *h, const char *a) {
    g_free(((Window *)data)->app_id);
    ((Window *)data)->app_id = g_strdup(a);
}
static void window_identifier(void *data, struct ext_foreign_toplevel_handle_v1 *h, const char *i) {}
static const struct ext_foreign_toplevel_handle_v1_listener window_listener = {
    window_closed, window_done, window_title, window_app_id, window_identifier,
};
static void toplevel_new(void *data, struct ext_foreign_toplevel_list_v1 *l,
                         struct ext_foreign_toplevel_handle_v1 *h) {
    Window *w = g_new0(Window, 1);
    w->handle = h;
    ext_foreign_toplevel_handle_v1_add_listener(h, &window_listener, w);
    windows = g_list_append(windows, w);
}
static void toplevels_finished(void *data, struct ext_foreign_toplevel_list_v1 *l) {}
static const struct ext_foreign_toplevel_list_v1_listener toplevels_listener = { toplevel_new, toplevels_finished };

static void registry_global(void *data, struct wl_registry *r, uint32_t name, const char *iface,
                            uint32_t version) {
    if (strcmp(iface, wl_output_interface.name) == 0) {
        Screen *s = g_new0(Screen, 1);
        s->name = name;
        s->output = wl_registry_bind(r, name, &wl_output_interface, 1);
        screens = g_list_append(screens, s);
        screen_watch(s);
    } else if (strcmp(iface, zxdg_output_manager_v1_interface.name) == 0 && version >= 2) {
        xdg_outputs = wl_registry_bind(r, name, &zxdg_output_manager_v1_interface, 2);
        for (GList *l = screens; l; l = l->next)
            screen_watch(l->data);
    } else if (strcmp(iface, wl_shm_interface.name) == 0) {
        shm = wl_registry_bind(r, name, &wl_shm_interface, 1);
    } else if (strcmp(iface, zwp_linux_dmabuf_v1_interface.name) == 0 && version >= 3) {
        linux_dmabuf = wl_registry_bind(r, name, &zwp_linux_dmabuf_v1_interface, 3);
    } else if (strcmp(iface, ext_image_copy_capture_manager_v1_interface.name) == 0) {
        copy_manager = wl_registry_bind(r, name, &ext_image_copy_capture_manager_v1_interface, 1);
    } else if (strcmp(iface, ext_output_image_capture_source_manager_v1_interface.name) == 0) {
        screen_sources = wl_registry_bind(r, name, &ext_output_image_capture_source_manager_v1_interface, 1);
    } else if (strcmp(iface, ext_foreign_toplevel_image_capture_source_manager_v1_interface.name) == 0) {
        window_sources = wl_registry_bind(r, name, &ext_foreign_toplevel_image_capture_source_manager_v1_interface, 1);
    } else if (strcmp(iface, ext_foreign_toplevel_list_v1_interface.name) == 0) {
        toplevels = wl_registry_bind(r, name, &ext_foreign_toplevel_list_v1_interface, 1);
        ext_foreign_toplevel_list_v1_add_listener(toplevels, &toplevels_listener, NULL);
    }
}

static void registry_global_remove(void *data, struct wl_registry *r, uint32_t name) {
    for (GList *l = screens; l; l = l->next) {
        Screen *s = l->data;
        if (s->name == name) {
            if (s->xdg)
                zxdg_output_v1_destroy(s->xdg);
            wl_output_destroy(s->output);
            screens = g_list_delete_link(screens, l);
            g_free(s->connector);
            g_free(s);
            return;
        }
    }
}
static const struct wl_registry_listener registry_listener = { registry_global, registry_global_remove };

static gboolean wl_readable(gint fd, GIOCondition cond, gpointer data) {
    if (wl_display_dispatch(wl) < 0) {
        g_printerr("[infynitae-portal] screen sharing: the compositor went away\n");
        return G_SOURCE_REMOVE;
    }
    return G_SOURCE_CONTINUE;
}

/* ---- PipeWire, run by the portal's GLib loop ----------------------------------------------- */

static struct pw_loop *pw_main;
static struct pw_context *pw_ctx;
static struct pw_core *pw_core;

static gboolean pw_readable(gint fd, GIOCondition cond, gpointer data) {
    pw_loop_iterate(pw_main, 0);
    return G_SOURCE_CONTINUE;
}

/* the GPU that dmabuf frames are made on: the compositor's (dmabuf_device), opened once */
static struct gbm_device *gbm;
static dev_t gbm_dev;

static gboolean gbm_for(dev_t dev) {
    if (gbm)
        return gbm_dev == dev;
    drmDevicePtr device;
    if (drmGetDeviceFromDevId(dev, 0, &device) != 0)
        return FALSE;
    int fd = -1;
    if (device->available_nodes & (1 << DRM_NODE_RENDER))
        fd = open(device->nodes[DRM_NODE_RENDER], O_RDWR | O_CLOEXEC);
    drmFreeDevice(&device);
    if (fd < 0)
        return FALSE;
    gbm = gbm_create_device(fd);
    if (!gbm) {
        close(fd);
        return FALSE;
    }
    gbm_dev = dev;
    return TRUE;
}

/* Connected once, at the first share: FALSE when something a share needs is missing. */
static gboolean ready(void) {
    if (!wl) {
        wl = wl_display_connect(NULL);
        if (!wl) {
            g_printerr("[infynitae-portal] screen sharing: no Wayland display\n");
            return FALSE;
        }
        struct wl_registry *registry = wl_display_get_registry(wl);
        wl_registry_add_listener(registry, &registry_listener, NULL);
        wl_display_roundtrip(wl); /* the globals */
        wl_display_roundtrip(wl); /* the screens' names and places, the windows */
        g_unix_fd_add(wl_display_get_fd(wl), G_IO_IN, wl_readable, NULL);
    }
    if (!copy_manager || !screen_sources || !shm) {
        g_printerr("[infynitae-portal] screen sharing: the compositor does not let this portal "
                   "capture (ALLOW_CAPTURE)\n");
        return FALSE;
    }
    if (!pw_ctx) {
        pw_init(NULL, NULL);
        pw_main = pw_loop_new(NULL);
        pw_loop_enter(pw_main);
        g_unix_fd_add(pw_loop_get_fd(pw_main), G_IO_IN, pw_readable, NULL);
        pw_ctx = pw_context_new(pw_main, NULL, 0);
    }
    if (!pw_core) {
        pw_core = pw_ctx ? pw_context_connect(pw_ctx, NULL, 0) : NULL;
        if (!pw_core) {
            g_printerr("[infynitae-portal] screen sharing: PipeWire is not running: %s\n", strerror(errno));
            return FALSE;
        }
    }
    return TRUE;
}

/* ---- a stream: one source's frames ------------------------------------------------------- */

typedef struct {
    struct wl_buffer *wl;
    struct gbm_bo *bo;
} Buffer;

typedef struct {
    Cast *cast;
    guint32 kind; /* CAST_MONITOR or CAST_WINDOW */
    int32_t x, y, lw, lh; /* a screen's place and logical size */
    struct ext_image_capture_source_v1 *source;
    struct ext_image_copy_capture_session_v1 *session;
    /* the constraints the compositor sent last: size, shm XRGB8888, linear dmabuf */
    uint32_t w, h;
    gboolean shm_ok, dmabuf_ok;
    dev_t dev;
    gboolean known; /* a first `done` came */
    struct pw_stream *stream;
    struct spa_hook listener;
    uint32_t node;
    gboolean dmabuf; /* what the app took */
    uint32_t fps;
    gboolean streaming;
    struct ext_image_copy_capture_frame_v1 *frame;
    struct pw_buffer *buffer;  /* being filled, or kept after a failed frame */
    struct pw_buffer *filled;  /* waiting for the next graph cycle */
    uint64_t pts, seq;
    gint64 last_us;
    guint timer;
} Stream;

struct Cast {
    guint32 types, cursor;
    GPtrArray *streams; /* Stream* */
    CastStarted started;
    CastEnded ended;
    gpointer data;
    GCancellable *choosing;
    guint ending;
};

static void capture_next(Stream *s);

static gboolean capture_later(gpointer data) {
    Stream *s = data;
    s->timer = 0;
    capture_next(s);
    return G_SOURCE_REMOVE;
}

static void capture_in(Stream *s, gint64 us) {
    if (!s->timer)
        s->timer = g_timeout_add(MAX(1, us / 1000), capture_later, s);
}

static void frame_transform(void *d, struct ext_image_copy_capture_frame_v1 *f, uint32_t t) {}
static void frame_damage(void *d, struct ext_image_copy_capture_frame_v1 *f, int32_t x, int32_t y, int32_t w, int32_t h) {}
static void frame_time(void *d, struct ext_image_copy_capture_frame_v1 *f, uint32_t hi, uint32_t lo, uint32_t ns) {
    ((Stream *)d)->pts = (((uint64_t)hi << 32 | lo) * SPA_NSEC_PER_SEC) + ns;
}

static void frame_ready(void *d, struct ext_image_copy_capture_frame_v1 *f) {
    Stream *s = d;
    ext_image_copy_capture_frame_v1_destroy(f);
    s->frame = NULL;
    struct spa_buffer *b = s->buffer->buffer;
    b->datas[0].chunk->size = b->datas[0].maxsize;
    struct spa_meta_header *h = spa_buffer_find_meta_data(b, SPA_META_Header, sizeof(*h));
    if (h) {
        h->pts = s->pts;
        h->flags = 0;
        h->seq = s->seq++;
        h->dts_offset = 0;
    }
    s->last_us = g_get_monotonic_time();
    if (pw_stream_is_driving(s->stream)) {
        /* queued in the cycle this starts (process) */
        s->filled = s->buffer;
        s->buffer = NULL;
        pw_stream_trigger_process(s->stream);
    } else {
        pw_stream_queue_buffer(s->stream, s->buffer);
        s->buffer = NULL;
        capture_next(s);
    }
}

static void frame_failed(void *d, struct ext_image_copy_capture_frame_v1 *f, uint32_t reason) {
    Stream *s = d;
    ext_image_copy_capture_frame_v1_destroy(f);
    s->frame = NULL;
    /* the buffer is kept for the next frame. On new constraints (a resize) the session sends
     * them and `done`, and the next frame follows the app's new format (stream_param); a private
     * window or a locked session fails until that changes: tried again at the stream's rate */
    if (reason == EXT_IMAGE_COPY_CAPTURE_FRAME_V1_FAILURE_REASON_UNKNOWN)
        capture_in(s, G_USEC_PER_SEC / s->fps);
}
static const struct ext_image_copy_capture_frame_v1_listener frame_listener = {
    frame_transform, frame_damage, frame_time, frame_ready, frame_failed,
};

/* The next frame into a free buffer, when the app wants frames and not sooner than its rate. */
static void capture_next(Stream *s) {
    if (!s->streaming || s->frame || s->timer)
        return;
    gint64 wait = s->last_us + G_USEC_PER_SEC / s->fps - g_get_monotonic_time();
    if (wait > 0) {
        capture_in(s, wait);
        return;
    }
    if (!s->buffer)
        s->buffer = pw_stream_dequeue_buffer(s->stream);
    if (!s->buffer) {
        /* all buffers are with the app: one comes back by the next frame's time */
        capture_in(s, G_USEC_PER_SEC / s->fps);
        return;
    }
    Buffer *buf = s->buffer->user_data;
    s->frame = ext_image_copy_capture_session_v1_create_frame(s->session);
    ext_image_copy_capture_frame_v1_add_listener(s->frame, &frame_listener, s);
    ext_image_copy_capture_frame_v1_attach_buffer(s->frame, buf->wl);
    ext_image_copy_capture_frame_v1_damage_buffer(s->frame, 0, 0, s->w, s->h);
    ext_image_copy_capture_frame_v1_capture(s->frame);
    wl_display_flush(wl);
}

/* The formats offered: linear dmabuf first when the compositor and this GPU can, then shm. */
static uint32_t formats(Stream *s, struct spa_pod_builder *b, const struct spa_pod **params) {
    uint32_t n = 0;
    gboolean dmabuf = s->dmabuf_ok && linux_dmabuf && gbm_for(s->dev);
    for (int pass = dmabuf ? 0 : 1; pass < 2; pass++) {
        if (pass == 1 && !s->shm_ok)
            break;
        struct spa_pod_frame f;
        spa_pod_builder_push_object(b, &f, SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat);
        spa_pod_builder_add(b, SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video),
                            SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
                            SPA_FORMAT_VIDEO_format, SPA_POD_Id(SPA_VIDEO_FORMAT_BGRx), 0);
        if (pass == 0) {
            spa_pod_builder_prop(b, SPA_FORMAT_VIDEO_modifier, SPA_POD_PROP_FLAG_MANDATORY);
            spa_pod_builder_long(b, MOD_LINEAR);
        }
        spa_pod_builder_add(b, SPA_FORMAT_VIDEO_size, SPA_POD_Rectangle(&SPA_RECTANGLE(s->w, s->h)),
                            SPA_FORMAT_VIDEO_framerate, SPA_POD_Fraction(&SPA_FRACTION(0, 1)),
                            SPA_FORMAT_VIDEO_maxFramerate,
                            SPA_POD_CHOICE_RANGE_Fraction(&SPA_FRACTION(DEFAULT_FPS, 1), &SPA_FRACTION(1, 1),
                                                          &SPA_FRACTION(1000, 1)),
                            0);
        params[n++] = spa_pod_builder_pop(b, &f);
    }
    return n;
}

static void stream_state(void *data, enum pw_stream_state old, enum pw_stream_state state, const char *error);
static void stream_param(void *data, uint32_t id, const struct spa_pod *param);
static void stream_add_buffer(void *data, struct pw_buffer *b);
static void stream_remove_buffer(void *data, struct pw_buffer *b);
static void stream_process(void *data);
static const struct pw_stream_events stream_events = {
    PW_VERSION_STREAM_EVENTS,
    .state_changed = stream_state,
    .param_changed = stream_param,
    .add_buffer = stream_add_buffer,
    .remove_buffer = stream_remove_buffer,
    .process = stream_process,
};

static void stream_connect(Stream *s) {
    uint8_t pod[1024];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(pod, sizeof(pod));
    const struct spa_pod *params[2];
    uint32_t n = formats(s, &b, params);
    s->stream = pw_stream_new(pw_core, "infynitae-screencast",
                              pw_properties_new(PW_KEY_MEDIA_CLASS, "Video/Source", NULL));
    pw_stream_add_listener(s->stream, &s->listener, &stream_events, s);
    s->node = SPA_ID_INVALID;
    pw_stream_connect(s->stream, PW_DIRECTION_OUTPUT, PW_ID_ANY,
                      PW_STREAM_FLAG_DRIVER | PW_STREAM_FLAG_ALLOC_BUFFERS, params, n);
}

static void cast_report(Cast *c);
static void stream_fail(Stream *s);

static void stream_state(void *data, enum pw_stream_state old, enum pw_stream_state state, const char *error) {
    Stream *s = data;
    if (state == PW_STREAM_STATE_ERROR) {
        g_printerr("[infynitae-portal] screen sharing: stream: %s\n", error ? error : "error");
        stream_fail(s);
        return;
    }
    s->streaming = state == PW_STREAM_STATE_STREAMING;
    if (s->node == SPA_ID_INVALID && pw_stream_get_node_id(s->stream) != SPA_ID_INVALID) {
        s->node = pw_stream_get_node_id(s->stream);
        cast_report(s->cast);
    }
    if (s->streaming)
        capture_next(s);
}

/* The app picked a format: buffers for it (allocated here: add_buffer). */
static void stream_param(void *data, uint32_t id, const struct spa_pod *param) {
    Stream *s = data;
    if (id != SPA_PARAM_Format || !param)
        return;
    struct spa_video_info_raw info;
    if (spa_format_video_raw_parse(param, &info) < 0)
        return;
    s->dmabuf = spa_pod_find_prop(param, NULL, SPA_FORMAT_VIDEO_modifier) != NULL;
    s->fps = info.max_framerate.denom && info.max_framerate.num
                 ? MAX(1, info.max_framerate.num / info.max_framerate.denom)
                 : DEFAULT_FPS;
    uint8_t pod[1024];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(pod, sizeof(pod));
    const struct spa_pod *params[2];
    params[0] = spa_pod_builder_add_object(
        &b, SPA_TYPE_OBJECT_ParamBuffers, SPA_PARAM_Buffers,
        SPA_PARAM_BUFFERS_buffers, SPA_POD_CHOICE_RANGE_Int(4, 2, 8),
        SPA_PARAM_BUFFERS_blocks, SPA_POD_Int(1),
        SPA_PARAM_BUFFERS_dataType, SPA_POD_CHOICE_FLAGS_Int(1 << (s->dmabuf ? SPA_DATA_DmaBuf : SPA_DATA_MemFd)));
    params[1] = spa_pod_builder_add_object(
        &b, SPA_TYPE_OBJECT_ParamMeta, SPA_PARAM_Meta,
        SPA_PARAM_META_type, SPA_POD_Id(SPA_META_Header),
        SPA_PARAM_META_size, SPA_POD_Int(sizeof(struct spa_meta_header)));
    pw_stream_update_params(s->stream, params, 2);
    /* after a resize nothing else asks for the first frame into the new buffers */
    capture_in(s, 0);
}

static void stream_add_buffer(void *data, struct pw_buffer *pb) {
    Stream *s = data;
    struct spa_data *d = &pb->buffer->datas[0];
    Buffer *buf = g_new0(Buffer, 1);
    uint32_t stride, offset = 0;
    int fd;
    if (s->dmabuf) {
        uint64_t mod = MOD_LINEAR;
        buf->bo = gbm_bo_create_with_modifiers2(gbm, s->w, s->h, DRM_FORMAT_XRGB8888, &mod, 1, GBM_BO_USE_RENDERING);
        fd = buf->bo ? gbm_bo_get_fd(buf->bo) : -1;
        if (fd < 0) {
            if (buf->bo)
                gbm_bo_destroy(buf->bo);
            g_free(buf);
            pw_stream_set_error(s->stream, -ENOMEM, "no GPU buffer");
            return;
        }
        stride = gbm_bo_get_stride(buf->bo);
        offset = gbm_bo_get_offset(buf->bo, 0);
        struct zwp_linux_buffer_params_v1 *p = zwp_linux_dmabuf_v1_create_params(linux_dmabuf);
        zwp_linux_buffer_params_v1_add(p, fd, 0, offset, stride, MOD_LINEAR >> 32, MOD_LINEAR & 0xffffffff);
        buf->wl = zwp_linux_buffer_params_v1_create_immed(p, s->w, s->h, DRM_FORMAT_XRGB8888, 0);
        zwp_linux_buffer_params_v1_destroy(p);
        d->type = SPA_DATA_DmaBuf;
    } else {
        stride = s->w * 4;
        fd = memfd_create("infynitae-screencast", MFD_CLOEXEC);
        if (fd < 0 || ftruncate(fd, (off_t)stride * s->h) < 0) {
            if (fd >= 0)
                close(fd);
            g_free(buf);
            pw_stream_set_error(s->stream, -errno, "no memory for a frame");
            return;
        }
        struct wl_shm_pool *pool = wl_shm_create_pool(shm, fd, stride * s->h);
        buf->wl = wl_shm_pool_create_buffer(pool, 0, s->w, s->h, stride, WL_SHM_FORMAT_XRGB8888);
        wl_shm_pool_destroy(pool);
        d->type = SPA_DATA_MemFd;
    }
    d->fd = fd;
    d->flags = SPA_DATA_FLAG_READWRITE;
    d->mapoffset = 0;
    d->maxsize = stride * s->h;
    d->data = NULL;
    d->chunk->offset = offset;
    d->chunk->stride = stride;
    d->chunk->size = 0;
    pb->user_data = buf;
}

static void stream_remove_buffer(void *data, struct pw_buffer *pb) {
    Stream *s = data;
    Buffer *buf = pb->user_data;
    if (!buf)
        return;
    if (s->buffer == pb) {
        if (s->frame) {
            ext_image_copy_capture_frame_v1_destroy(s->frame);
            s->frame = NULL;
        }
        s->buffer = NULL;
    }
    if (s->filled == pb)
        s->filled = NULL;
    wl_buffer_destroy(buf->wl);
    if (buf->bo)
        gbm_bo_destroy(buf->bo);
    close(pb->buffer->datas[0].fd);
    pb->buffer->datas[0].fd = -1;
    pb->user_data = NULL;
    g_free(buf);
}

static void stream_process(void *data) {
    Stream *s = data;
    if (s->filled) {
        pw_stream_queue_buffer(s->stream, s->filled);
        s->filled = NULL;
    }
    capture_next(s);
}

/* ---- the capture session's constraints ------------------------------------------------- */

static void session_size(void *d, struct ext_image_copy_capture_session_v1 *x, uint32_t w, uint32_t h) {
    Stream *s = d;
    s->w = w;
    s->h = h;
    s->shm_ok = s->dmabuf_ok = FALSE;
}
static void session_shm(void *d, struct ext_image_copy_capture_session_v1 *x, uint32_t format) {
    if (format == WL_SHM_FORMAT_XRGB8888)
        ((Stream *)d)->shm_ok = TRUE;
}
static void session_device(void *d, struct ext_image_copy_capture_session_v1 *x, struct wl_array *dev) {
    if (dev->size == sizeof(dev_t))
        memcpy(&((Stream *)d)->dev, dev->data, sizeof(dev_t));
}
static void session_dmabuf(void *d, struct ext_image_copy_capture_session_v1 *x, uint32_t format, struct wl_array *mods) {
    uint64_t *mod;
    if (format == DRM_FORMAT_XRGB8888)
        wl_array_for_each(mod, mods) if (*mod == MOD_LINEAR) ((Stream *)d)->dmabuf_ok = TRUE;
}

/* The constraints are complete: the first time the stream is made, after a resize the app is
 * offered the new size (it picks a format again, and new buffers follow). */
static void session_done(void *d, struct ext_image_copy_capture_session_v1 *x) {
    Stream *s = d;
    if (!s->known) {
        s->known = TRUE;
        stream_connect(s);
        return;
    }
    uint8_t pod[1024];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(pod, sizeof(pod));
    const struct spa_pod *params[2];
    pw_stream_update_params(s->stream, params, formats(s, &b, params));
}

static gboolean cast_end(gpointer data) {
    Cast *c = data;
    c->ending = 0;
    if (c->ended)
        c->ended(c->data);
    return G_SOURCE_REMOVE;
}

/* A stream that cannot start fails its Start (node 0); one that ran ends the cast. */
static void stream_fail(Stream *s) {
    Cast *c = s->cast;
    if (c->started) {
        s->node = 0;
        cast_report(c);
    } else if (!c->ending) {
        /* from the main loop: `ended` frees the cast, this stream with it */
        c->ending = g_idle_add(cast_end, c);
    }
}

static void session_stopped(void *d, struct ext_image_copy_capture_session_v1 *x) {
    stream_fail(d);
}
static const struct ext_image_copy_capture_session_v1_listener session_listener = {
    session_size, session_shm, session_device, session_dmabuf, session_done, session_stopped,
};

/* ---- a cast ---------------------------------------------------------------------------- */

Cast *cast_new(void) {
    if (!ready())
        return NULL;
    Cast *c = g_new0(Cast, 1);
    c->types = CAST_MONITOR;
    c->cursor = CURSOR_HIDDEN;
    c->streams = g_ptr_array_new();
    return c;
}

void cast_select(Cast *c, GVariant *options) {
    guint32 v;
    if (g_variant_lookup(options, "types", "u", &v) && (v & (CAST_MONITOR | CAST_WINDOW)))
        c->types = v & (CAST_MONITOR | CAST_WINDOW);
    if (g_variant_lookup(options, "cursor_mode", "u", &v) && (v & (CURSOR_HIDDEN | CURSOR_EMBEDDED)))
        c->cursor = v;
}

static void stream_free(Stream *s) {
    if (s->timer)
        g_source_remove(s->timer);
    if (s->frame)
        ext_image_copy_capture_frame_v1_destroy(s->frame);
    s->frame = NULL;
    if (s->stream)
        pw_stream_destroy(s->stream); /* its buffers go (remove_buffer) */
    ext_image_copy_capture_session_v1_destroy(s->session);
    ext_image_capture_source_v1_destroy(s->source);
    wl_display_flush(wl);
    g_free(s);
}

/* Start's answer once every stream has its PipeWire node (or failed: node 0). */
static void cast_report(Cast *c) {
    if (!c->started)
        return;
    GVariantBuilder streams;
    g_variant_builder_init(&streams, G_VARIANT_TYPE("a(ua{sv})"));
    for (guint i = 0; i < c->streams->len; i++) {
        Stream *s = g_ptr_array_index(c->streams, i);
        if (s->node == SPA_ID_INVALID) {
            g_variant_builder_clear(&streams);
            return;
        }
        if (s->node == 0) {
            g_variant_builder_clear(&streams);
            c->started(NULL, c->data);
            c->started = NULL;
            return;
        }
        GVariantBuilder props;
        g_variant_builder_init(&props, G_VARIANT_TYPE_VARDICT);
        g_variant_builder_add(&props, "{sv}", "source_type", g_variant_new_uint32(s->kind));
        if (s->kind == CAST_MONITOR) {
            g_variant_builder_add(&props, "{sv}", "position", g_variant_new("(ii)", s->x, s->y));
            g_variant_builder_add(&props, "{sv}", "size", g_variant_new("(ii)", s->lw, s->lh));
        } else {
            g_variant_builder_add(&props, "{sv}", "size", g_variant_new("(ii)", (gint32)s->w, (gint32)s->h));
        }
        g_variant_builder_add(&streams, "(ua{sv})", s->node, &props);
    }
    CastStarted started = c->started;
    c->started = NULL;
    started(g_variant_builder_end(&streams), c->data);
}

static Stream *stream_add(Cast *c, guint32 kind, struct ext_image_capture_source_v1 *source) {
    Stream *s = g_new0(Stream, 1);
    s->cast = c;
    s->kind = kind;
    s->fps = DEFAULT_FPS;
    s->node = SPA_ID_INVALID;
    s->source = source;
    s->session = ext_image_copy_capture_manager_v1_create_session(
        copy_manager, source, c->cursor == CURSOR_EMBEDDED ? EXT_IMAGE_COPY_CAPTURE_MANAGER_V1_OPTIONS_PAINT_CURSORS : 0);
    ext_image_copy_capture_session_v1_add_listener(s->session, &session_listener, s);
    g_ptr_array_add(c->streams, s);
    wl_display_flush(wl);
    return s;
}

/* the chooser's line for a window: one line, as the chooser shows it */
static char *window_line(Window *w) {
    char *line = g_strdup_printf("Window: %s (%s)", w->title ? w->title : "", w->app_id ? w->app_id : "");
    g_strdelimit(line, "\n\r", ' ');
    return line;
}

static void chosen(GObject *source, GAsyncResult *res, gpointer data) {
    char *out = NULL;
    GError *error = NULL;
    gboolean ok = g_subprocess_communicate_utf8_finish(G_SUBPROCESS(source), res, &out, NULL, &error);
    g_object_unref(source);
    if (!ok && g_error_matches(error, G_IO_ERROR, G_IO_ERROR_CANCELLED)) {
        g_error_free(error);
        return; /* the cast is gone */
    }
    Cast *c = data;
    g_clear_object(&c->choosing);
    if (!ok) {
        g_printerr("[infynitae-portal] screen sharing: the chooser failed: %s\n", error->message);
        g_error_free(error);
    }
    char *answer = g_strstrip(out ? out : g_strdup(""));
    if (g_str_has_prefix(answer, "Monitor: ")) {
        for (GList *l = screens; l; l = l->next) {
            Screen *sc = l->data;
            if (g_strcmp0(sc->connector, answer + 9) == 0) {
                Stream *s = stream_add(c, CAST_MONITOR, ext_output_image_capture_source_manager_v1_create_source(
                                                            screen_sources, sc->output));
                s->x = sc->x;
                s->y = sc->y;
                s->lw = sc->w;
                s->lh = sc->h;
                break;
            }
        }
    } else if (g_str_has_prefix(answer, "Window: ") && window_sources) {
        for (GList *l = windows; l; l = l->next) {
            char *line = window_line(l->data);
            gboolean match = g_strcmp0(line, answer) == 0;
            g_free(line);
            if (match) {
                stream_add(c, CAST_WINDOW, ext_foreign_toplevel_image_capture_source_manager_v1_create_source(
                                               window_sources, ((Window *)l->data)->handle));
                break;
            }
        }
    }
    g_free(answer);
    if (c->streams->len == 0) {
        c->started(NULL, c->data);
        c->started = NULL;
    }
}

void cast_start(Cast *c, CastStarted started, CastEnded ended, gpointer data) {
    c->started = started;
    c->ended = ended;
    c->data = data;
    wl_display_roundtrip(wl); /* the screens and windows as they are now */
    GString *lines = g_string_new(NULL);
    if (c->types & CAST_MONITOR)
        for (GList *l = screens; l; l = l->next)
            if (((Screen *)l->data)->connector)
                g_string_append_printf(lines, "Monitor: %s\n", ((Screen *)l->data)->connector);
    if ((c->types & CAST_WINDOW) && window_sources)
        for (GList *l = windows; l; l = l->next) {
            char *line = window_line(l->data);
            g_string_append_printf(lines, "%s\n", line);
            g_free(line);
        }
    char *chooser = g_build_filename(g_get_home_dir(), "infynitae", "live", "screenshare-chooser", NULL);
    GError *error = NULL;
    GSubprocess *p = g_subprocess_new(G_SUBPROCESS_FLAGS_STDIN_PIPE | G_SUBPROCESS_FLAGS_STDOUT_PIPE, &error,
                                      chooser, NULL);
    g_free(chooser);
    if (!p) {
        g_printerr("[infynitae-portal] screen sharing: no chooser: %s\n", error->message);
        g_error_free(error);
        g_string_free(lines, TRUE);
        c->started = NULL;
        started(NULL, data);
        return;
    }
    c->choosing = g_cancellable_new();
    g_subprocess_communicate_utf8_async(p, lines->str, c->choosing, chosen, c);
    g_string_free(lines, TRUE);
}

void cast_free(Cast *c) {
    if (c->ending)
        g_source_remove(c->ending);
    if (c->choosing) {
        g_cancellable_cancel(c->choosing);
        g_object_unref(c->choosing);
    }
    for (guint i = 0; i < c->streams->len; i++)
        stream_free(g_ptr_array_index(c->streams, i));
    g_ptr_array_unref(c->streams);
    g_free(c);
}
