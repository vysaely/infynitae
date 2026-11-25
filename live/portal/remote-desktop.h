/* Remote desktop for Infynitae's portal (remote-desktop.c). */
#pragma once
#include <gio/gio.h>

/* Registers RemoteDesktop and ScreenCast on `object_path`. */
void remote_desktop_init(GDBusConnection *connection, const char *object_path);

/* Whether `sender` is xdg-desktop-portal, the only caller a portal backend serves. */
gboolean from_portal(GDBusConnection *c, const gchar *sender);
