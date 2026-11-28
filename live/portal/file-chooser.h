/* FileChooser backend for Infynitae's portal (file-chooser.c). */
#pragma once
#include <gio/gio.h>

/* Registers org.freedesktop.impl.portal.FileChooser on `object_path`. */
void file_chooser_init(GDBusConnection *connection, const char *object_path);
