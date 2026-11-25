/* The Screenshot portal for Infynitae's portal (screenshot.c). */
#pragma once
#include <gio/gio.h>

/* Registers org.freedesktop.impl.portal.Screenshot on `object_path`. */
void screenshot_init(GDBusConnection *conn, const char *object_path);
