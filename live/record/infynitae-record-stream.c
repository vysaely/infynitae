/*
 * Infynitae Screen Capture & Rolling Replay Engine
 * Pure C + Wayland screencopy: raw frames to stdout for the recorder's ffmpeg encoder.
 */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>
#include <stdint.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <signal.h>
#include <time.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <sys/time.h>

#include <wayland-client.h>
#include "wlr-screencopy-protocol.h"

#define DEFAULT_FPS 60
#define DEFAULT_REPLAY_SECONDS 1200 // 20 minutes
#define SEGMENT_DURATION 10         // 10s per segment

typedef struct {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_shm *shm;
    struct wl_output *output;
    struct zwlr_screencopy_manager_v1 *manager;

    int width;
    int height;
    int stride;
    uint32_t format;

    int initial_width;
    int initial_height;

    int target_output_idx;
    int current_output_idx;

    int shm_fd[2];
    void *shm_data[2];
    size_t shm_size[2];
    struct wl_buffer *wl_buffer[2];
    int current_buf;

    bool ready;
    bool running;
    /* the bound zwlr_screencopy_manager_v1 version (buffer_done is v3) */
    uint32_t manager_version;
    /* this frame's buffer is chosen: the compositor sends one `buffer` event per format it
       offers, and a frame may be copied only once (the protocol's already_used error) */
    bool have_buffer;
    int target_fps;
    struct timespec last_frame_time;

    int pipe_fd;
    pid_t ffmpeg_pid;
} RecorderState;

static RecorderState state;
static bool info_only = false;

static int create_shm_buffer(RecorderState *s, int idx, size_t size) {
    if (s->wl_buffer[idx]) {
        wl_buffer_destroy(s->wl_buffer[idx]);
        s->wl_buffer[idx] = NULL;
    }
    if (s->shm_data[idx]) {
        munmap(s->shm_data[idx], s->shm_size[idx]);
        s->shm_data[idx] = NULL;
    }
    if (s->shm_fd[idx] >= 0) {
        close(s->shm_fd[idx]);
        s->shm_fd[idx] = -1;
    }

    int fd = memfd_create("infynitae-screencopy", MFD_CLOEXEC);
    if (fd < 0) {
        perror("memfd_create");
        return -1;
    }
    if (ftruncate(fd, size) < 0) {
        perror("ftruncate");
        close(fd);
        return -1;
    }

    void *data = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (data == MAP_FAILED) {
        perror("mmap");
        close(fd);
        return -1;
    }

    struct wl_shm_pool *pool = wl_shm_create_pool(s->shm, fd, size);
    struct wl_buffer *buf = wl_shm_pool_create_buffer(
        pool, 0, s->width, s->height, s->stride, WL_SHM_FORMAT_ARGB8888
    );
    wl_shm_pool_destroy(pool);

    s->shm_fd[idx] = fd;
    s->shm_data[idx] = data;
    s->shm_size[idx] = size;
    s->wl_buffer[idx] = buf;
    return 0;
}

static void request_frame(RecorderState *s);

static void frame_buffer(void *data, struct zwlr_screencopy_frame_v1 *frame,
                         uint32_t format, uint32_t width, uint32_t height, uint32_t stride) {
    RecorderState *s = (RecorderState *)data;

    if (info_only) {
        printf("WIDTH=%u\nHEIGHT=%u\nSTRIDE=%u\nFORMAT=%u\n", width, height, stride, format);
        fflush(stdout);
        exit(0);
    }

    /* the first format offered is used; the others of the same frame are ignored */
    if (s->have_buffer) {
        return;
    }
    s->have_buffer = true;

    // Dynamic resolution change detection (e.g. handheld docking, game resolution switch)
    if (s->initial_width == 0) {
        s->initial_width = (int)width;
        s->initial_height = (int)height;
    } else if ((int)width != s->initial_width || (int)height != s->initial_height) {
        fprintf(stderr, "[infynitae-record] Output geometry changed (%dx%d -> %ux%u). Exiting to allow supervisor restart.\n",
                s->initial_width, s->initial_height, width, height);
        s->running = false;
        zwlr_screencopy_frame_v1_destroy(frame);
        return;
    }

    s->format = format;
    s->width = (int)width;
    s->height = (int)height;
    s->stride = (int)stride;

    size_t size = (size_t)stride * height;
    if (s->shm_size[s->current_buf] != size) {
        create_shm_buffer(s, 0, size);
        create_shm_buffer(s, 1, size);
    }

    /* from v3 the copy waits for buffer_done, the end of the list of formats */
    if (s->manager_version < 3) {
        zwlr_screencopy_frame_v1_copy(frame, s->wl_buffer[s->current_buf]);
    }
}

static void frame_flags(void *data, struct zwlr_screencopy_frame_v1 *frame, uint32_t flags) {
    (void)data; (void)frame; (void)flags;
}

static void frame_ready(void *data, struct zwlr_screencopy_frame_v1 *frame,
                        uint32_t tv_sec_hi, uint32_t tv_sec_lo, uint32_t tv_nsec) {
    (void)tv_sec_hi; (void)tv_sec_lo; (void)tv_nsec;
    RecorderState *s = (RecorderState *)data;

    // 1. Output the current buffer to stdout/pipe
    if (s->pipe_fd >= 0 && s->shm_data[s->current_buf]) {
        size_t row_bytes = (size_t)s->width * 4;
        if ((size_t)s->stride == row_bytes) {
            // Fast-path: stride matches row width exactly (no padding bytes)
            size_t bytes_to_write = row_bytes * (size_t)s->height;
            ssize_t written = 0;
            char *buf = (char *)s->shm_data[s->current_buf];
            while (written < (ssize_t)bytes_to_write) {
                ssize_t n = write(s->pipe_fd, buf + written, bytes_to_write - written);
                if (n <= 0) {
                    if (errno == EINTR) continue;
                    s->running = false;
                    break;
                }
                written += n;
            }
        } else {
            // Stride has driver padding (common on laptops 1366x768, Steam Deck, custom ultrawide):
            // Write row-by-row stripping padding so FFmpeg receives clean unskewed rawvideo
            for (int y = 0; y < s->height && s->running; y++) {
                char *row = (char *)s->shm_data[s->current_buf] + (size_t)y * s->stride;
                ssize_t written = 0;
                while (written < (ssize_t)row_bytes) {
                    ssize_t n = write(s->pipe_fd, row + written, row_bytes - written);
                    if (n <= 0) {
                        if (errno == EINTR) continue;
                        s->running = false;
                        break;
                    }
                    written += n;
                }
            }
        }
    }

    zwlr_screencopy_frame_v1_destroy(frame);
    s->current_buf = 1 - s->current_buf;

    if (!s->running) return;

    // 2. Rate pacing: Calculate time until next frame deadline
    struct timespec now;
    clock_gettime(CLOCK_MONOTONIC, &now);
    long min_interval_ns = 1000000000L / s->target_fps;

    if (s->last_frame_time.tv_sec != 0) {
        long elapsed_ns = (now.tv_sec - s->last_frame_time.tv_sec) * 1000000000L + (now.tv_nsec - s->last_frame_time.tv_nsec);
        // Leave 2ms headroom so the compositor has the capture request queued before vsync
        long sleep_ns = min_interval_ns - elapsed_ns - 2000000L;
        if (sleep_ns > 0) {
            struct timespec ts = {
                .tv_sec = sleep_ns / 1000000000L,
                .tv_nsec = sleep_ns % 1000000000L
            };
            nanosleep(&ts, NULL);
        }
    }

    clock_gettime(CLOCK_MONOTONIC, &s->last_frame_time);

    if (s->running) {
        request_frame(s);
    }
}

static void frame_failed(void *data, struct zwlr_screencopy_frame_v1 *frame) {
    RecorderState *s = (RecorderState *)data;
    zwlr_screencopy_frame_v1_destroy(frame);
    if (s->running) {
        usleep(16000);
        request_frame(s);
    }
}

static void frame_damage(void *data, struct zwlr_screencopy_frame_v1 *frame,
                         uint32_t x, uint32_t y, uint32_t width, uint32_t height) {
    (void)data; (void)frame; (void)x; (void)y; (void)width; (void)height;
}

static void frame_linux_dmabuf(void *data, struct zwlr_screencopy_frame_v1 *frame,
                               uint32_t format, uint32_t width, uint32_t height) {
    (void)data; (void)frame; (void)format; (void)width; (void)height;
}

static void frame_buffer_done(void *data, struct zwlr_screencopy_frame_v1 *frame) {
    RecorderState *s = (RecorderState *)data;
    /* every format is offered: copy once into the buffer made for the first */
    if (s->have_buffer && s->running && s->wl_buffer[s->current_buf]) {
        zwlr_screencopy_frame_v1_copy(frame, s->wl_buffer[s->current_buf]);
    }
}

static const struct zwlr_screencopy_frame_v1_listener frame_listener = {
    .buffer = frame_buffer,
    .flags = frame_flags,
    .ready = frame_ready,
    .failed = frame_failed,
    .damage = frame_damage,
    .linux_dmabuf = frame_linux_dmabuf,
    .buffer_done = frame_buffer_done,
};

static void request_frame(RecorderState *s) {
    if (!s->manager || !s->output) return;
    s->have_buffer = false;
    struct zwlr_screencopy_frame_v1 *frame = zwlr_screencopy_manager_v1_capture_output(
        s->manager, 1 /* cursor */, s->output
    );
    zwlr_screencopy_frame_v1_add_listener(frame, &frame_listener, s);
    wl_display_flush(s->display);
}

static void registry_global(void *data, struct wl_registry *registry,
                            uint32_t name, const char *interface, uint32_t version) {
    RecorderState *s = (RecorderState *)data;
    if (strcmp(interface, wl_shm_interface.name) == 0) {
        s->shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    } else if (strcmp(interface, wl_output_interface.name) == 0) {
        if (s->current_output_idx == s->target_output_idx && !s->output) {
            s->output = wl_registry_bind(registry, name, &wl_output_interface, 1);
        }
        s->current_output_idx++;
    } else if (strcmp(interface, zwlr_screencopy_manager_v1_interface.name) == 0) {
        s->manager_version = version < 3 ? version : 3;
        s->manager = wl_registry_bind(registry, name, &zwlr_screencopy_manager_v1_interface, s->manager_version);
    }
}

static void registry_global_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data; (void)registry; (void)name;
}

static const struct wl_registry_listener registry_listener = {
    .global = registry_global,
    .global_remove = registry_global_remove,
};

static void sigint_handler(int sig) {
    (void)sig;
    state.running = false;
}

int main(int argc, char **argv) {
    signal(SIGINT, sigint_handler);
    signal(SIGTERM, sigint_handler);
    signal(SIGPIPE, SIG_IGN);

    memset(&state, 0, sizeof(state));
    state.shm_fd[0] = -1;
    state.shm_fd[1] = -1;
    state.pipe_fd = STDOUT_FILENO;
    state.target_fps = DEFAULT_FPS;
    state.running = true;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--info") == 0) {
            info_only = true;
        } else if (strcmp(argv[i], "--fps") == 0 && i + 1 < argc) {
            int fps = atoi(argv[++i]);
            if (fps > 0 && fps <= 360) {
                state.target_fps = fps;
            }
        } else if (strcmp(argv[i], "--output") == 0 && i + 1 < argc) {
            state.target_output_idx = atoi(argv[++i]);
            if (state.target_output_idx < 0) state.target_output_idx = 0;
        }
    }

    state.display = wl_display_connect(NULL);
    if (!state.display) {
        fprintf(stderr, "[infynitae-record] Failed to connect to Wayland display\n");
        return 1;
    }

    state.registry = wl_display_get_registry(state.display);
    wl_registry_add_listener(state.registry, &registry_listener, &state);
    wl_display_roundtrip(state.display);

    if (!state.manager || !state.shm || !state.output) {
        fprintf(stderr, "[infynitae-record] Missing required Wayland interfaces\n");
        return 1;
    }

    // Start first frame
    request_frame(&state);

    while (state.running && wl_display_dispatch(state.display) != -1) {
        // Event loop continues until state.running is false
    }

    // Cleanup
    for (int i = 0; i < 2; i++) {
        if (state.wl_buffer[i]) wl_buffer_destroy(state.wl_buffer[i]);
        if (state.shm_data[i]) munmap(state.shm_data[i], state.shm_size[i]);
        if (state.shm_fd[i] >= 0) close(state.shm_fd[i]);
    }
    if (state.output) wl_output_destroy(state.output);
    if (state.manager) zwlr_screencopy_manager_v1_destroy(state.manager);
    if (state.shm) wl_shm_destroy(state.shm);
    if (state.registry) wl_registry_destroy(state.registry);
    if (state.display) wl_display_disconnect(state.display);

    return 0;
}
