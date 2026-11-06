/* org.freedesktop.impl.portal.Settings: the desktop's appearance from the Infynitae config, so apps
 * that ask the portal (GTK 4 / libadwaita, Firefox, Chromium and Electron, Qt 6) follow it: dark or
 * light (COLOR_SCHEME) and the accent colour (BORDER_ACTIVE), live when the config changes.
 * Everything else (fonts, cursor, GNOME keys) still comes from the GTK portal: portals.conf lists
 * "infynitae;gtk" for Settings and xdg-desktop-portal asks both. */
#ifndef INFYNITAE_PORTAL_SETTINGS_H
#define INFYNITAE_PORTAL_SETTINGS_H

#include <gio/gio.h>

typedef struct {
    guint32 color_scheme; /* 0 no preference, 1 dark, 2 light */
    double accent[3];     /* 0..1 */
    guint32 contrast;     /* 0 normal */
} PortalAppearance;

/* The appearance a config file asks for (missing keys: Infynitae's defaults). */
void portal_appearance_read(const char *config_path, PortalAppearance *out);

/* Registers the interface on `path` and reads the config. */
void portal_settings_init(GDBusConnection *connection, const char *path, const char *config_path);

/* The config changed: reread it and tell apps what changed (SettingChanged). */
void portal_settings_config_changed(void);

#endif
