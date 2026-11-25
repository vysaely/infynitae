/* Screen sharing for Infynitae's portal (screencast.c): a screen or a window into a PipeWire stream. */
#pragma once
#include <gio/gio.h>

/* ScreenCast source types and cursor modes (the portal's numbers) */
#define CAST_MONITOR 1
#define CAST_WINDOW 2
#define CURSOR_HIDDEN 1
#define CURSOR_EMBEDDED 2

typedef struct Cast Cast;

/* `streams` is the a(ua{sv}) a Start answers with, or NULL: nothing was picked or it failed */
typedef void (*CastStarted)(GVariant *streams, gpointer data);
/* the compositor stopped the share (the window closed, the screen went) */
typedef void (*CastEnded)(gpointer data);

/* A cast, or NULL when the compositor offers no capture or PipeWire is not there. */
Cast *cast_new(void);
/* The sources an app asked for (SelectSources' options: types, cursor_mode). */
void cast_select(Cast *cast, GVariant *options);
/* Asks which source (live/screenshare-chooser), then starts its stream. */
void cast_start(Cast *cast, CastStarted started, CastEnded ended, gpointer data);
void cast_free(Cast *cast);
