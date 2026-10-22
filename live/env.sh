#!/bin/sh
# Infynitae Standalone Desktop Environment Setup
# Decouples Wayland clients from host desktop environments (e.g. KDE Plasma, GNOME)

# 1. Desktop Identity
export XDG_CURRENT_DESKTOP="infynitae:wlroots"
export XDG_SESSION_DESKTOP="infynitae"
export DESKTOP_SESSION="infynitae"
export XDG_SESSION_TYPE="wayland"

# 2. Decouple from KDE Plasma session daemons and hooks
unset KDE_FULL_SESSION
unset KDE_SESSION_VERSION
unset KDE_SESSION_UID
unset KDE_APPLICATIONS_AS_SCOPE
unset XDG_MENU_PREFIX
unset KDE_COLOR_SCHEME_PATH
unset KDE_FORBIDDEN_FILESYSTEMS

# 3. Decouple Qt from KDE platform theme daemon (kded6 / plasma-workspace)
export QT_QPA_PLATFORM="wayland"
export QT_QPA_PLATFORMTHEME=""
export QT_WAYLAND_DISABLE_WINDOWDECORATION="1"

# 4. Standard toolkit Wayland backends
export GDK_BACKEND="wayland"
export MOZ_ENABLE_WAYLAND="1"
export SDL_VIDEODRIVER="wayland"
export CLUTTER_BACKEND="wayland"
unset GTK_MODULES
unset GTK_RC_FILES

# 5. Ensure XDG_RUNTIME_DIR is valid (required for Wayland sockets)
if [ -z "$XDG_RUNTIME_DIR" ]; then
    if [ -d "/run/user/$(id -u)" ] && [ -w "/run/user/$(id -u)" ]; then
        export XDG_RUNTIME_DIR="/run/user/$(id -u)"
    else
        export XDG_RUNTIME_DIR="/tmp/infynitae-runtime-$(id -u)"
        mkdir -p -m 0700 "$XDG_RUNTIME_DIR"
    fi
fi

# 6. Load user config and synchronize universal settings
if [ -f "$HOME/.config/infynitae/config" ]; then
    . "$HOME/.config/infynitae/config"
fi
export XCURSOR_THEME="${CURSOR_THEME:-${XCURSOR_THEME:-Adwaita}}"
export XCURSOR_SIZE="${CURSOR_SIZE:-${XCURSOR_SIZE:-24}}"
export XKB_DEFAULT_LAYOUT="${XKB_DEFAULT_LAYOUT:-us}"
export XKB_DEFAULT_OPTIONS="${XKB_DEFAULT_OPTIONS:-caps:escape}"
export MOUSE_ACCEL_PROFILE="${MOUSE_ACCEL_PROFILE:-flat}"
export MOUSE_ACCEL_SPEED="${MOUSE_ACCEL_SPEED:-0.0}"
export KEYBOARD_REPEAT_DELAY="${KEYBOARD_REPEAT_DELAY:-200}"
export KEYBOARD_REPEAT_RATE="${KEYBOARD_REPEAT_RATE:-35}"

if [ -f "$HOME/infynitae/live/settings.sh" ]; then
    sh "$HOME/infynitae/live/settings.sh" apply >/dev/null 2>&1 || true
fi

# 6. Isolated D-Bus/dconf database profile (prevents inheriting host desktop defaults)
INFYNITAE_CFG_DIR="$HOME/.config/infynitae"
mkdir -p "$INFYNITAE_CFG_DIR"
if [ ! -f "$INFYNITAE_CFG_DIR/dconf-profile" ]; then
    printf 'user-db:infynitae\n' > "$INFYNITAE_CFG_DIR/dconf-profile"
fi
export DCONF_PROFILE="$INFYNITAE_CFG_DIR/dconf-profile"

# 7. XDG Desktop Portal routing: use GTK portal instead of KDE Plasma portal
PORTAL_CFG_DIR="$HOME/.config/xdg-desktop-portal"
mkdir -p "$PORTAL_CFG_DIR"
cat << 'EOF' > "$PORTAL_CFG_DIR/infynitae-portals.conf"
[preferred]
default=gtk
org.freedesktop.impl.portal.Settings=gtk
org.freedesktop.impl.portal.FileChooser=gtk
org.freedesktop.impl.portal.AppChooser=gtk
org.freedesktop.impl.portal.Notification=gtk
EOF

# 8. Notify D-Bus / systemd activation environment if available
if command -v dbus-update-activation-environment >/dev/null 2>&1; then
    dbus-update-activation-environment --systemd \
        XDG_CURRENT_DESKTOP \
        XDG_SESSION_DESKTOP \
        DESKTOP_SESSION \
        XDG_SESSION_TYPE \
        XCURSOR_THEME \
        XCURSOR_SIZE \
        DCONF_PROFILE 2>/dev/null || true
fi
