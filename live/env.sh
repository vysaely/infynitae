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
export QT_QPA_PLATFORM="wayland;xcb"
export QT_QPA_PLATFORMTHEME=""
export QT_WAYLAND_DISABLE_WINDOWDECORATION="1"
export QT_AUTO_SCREEN_SCALE_FACTOR="1"

# 4. Standard toolkit Wayland backends (with X11 fallback for Xwayland)
export GDK_BACKEND="wayland,x11,*"
export MOZ_ENABLE_WAYLAND="1"
export SDL_VIDEODRIVER="wayland,x11"
export CLUTTER_BACKEND="wayland"
export ELECTRON_OZONE_PLATFORM_HINT="auto"
export _JAVA_AWT_WM_NONREPARENTING="1"
unset GTK_MODULES
unset GTK_RC_FILES

# 4b. Hardware video acceleration & GPU vendor routing
if [ -e "/dev/nvidia0" ] || [ -d "/sys/module/nvidia" ]; then
    export LIBVA_DRIVER_NAME="nvidia"
    export __GLX_VENDOR_LIBRARY_NAME="nvidia"
fi

# 4b. Ensure CLI URL openers (xdg-open generic fallback) dispatch asynchronously
export BROWSER="gio open"

# 5. Ensure XDG_RUNTIME_DIR is valid (required for Wayland sockets)
if [ -z "$XDG_RUNTIME_DIR" ]; then
    if [ -d "/run/user/$(id -u)" ] && [ -w "/run/user/$(id -u)" ]; then
        export XDG_RUNTIME_DIR="/run/user/$(id -u)"
    else
        FALLBACK_DIR="/tmp/infynitae-runtime-$(id -u)"
        if [ -d "$FALLBACK_DIR" ] && [ ! -O "$FALLBACK_DIR" ]; then
            FALLBACK_DIR=$(mktemp -d -t "infynitae-runtime-$(id -u)-XXXXXX")
        else
            mkdir -p -m 0700 "$FALLBACK_DIR"
        fi
        chmod 0700 "$FALLBACK_DIR"
        export XDG_RUNTIME_DIR="$FALLBACK_DIR"
    fi
fi

# 5b. Route libseat to systemd-logind if systemd is active (avoids seatd conflicts and raw input hacks)
if [ -z "$LIBSEAT_BACKEND" ]; then
    if command -v systemctl >/dev/null 2>&1 && systemctl is-active --quiet systemd-logind 2>/dev/null; then
        export LIBSEAT_BACKEND="logind"
    fi
fi

# 6. Load user config and synchronize universal settings
if [ -f "$HOME/.config/infynitae/config" ]; then
    . "$HOME/.config/infynitae/config"
fi
export XCURSOR_THEME="${CURSOR_THEME:-${XCURSOR_THEME:-Adwaita}}"
export XCURSOR_SIZE="${CURSOR_SIZE:-${XCURSOR_SIZE:-24}}"

# Autodetect system keyboard layout if not explicitly set in ~/.config/infynitae/config
if [ -z "$XKB_DEFAULT_LAYOUT" ]; then
    if command -v localectl >/dev/null 2>&1; then
        _SYS_LAYOUT=$(localectl status 2>/dev/null | awk '/X11 Layout:/ {print $3}')
        if [ -n "$_SYS_LAYOUT" ] && [ "$_SYS_LAYOUT" != "n/a" ]; then
            export XKB_DEFAULT_LAYOUT="$_SYS_LAYOUT"
        fi
        _SYS_VARIANT=$(localectl status 2>/dev/null | awk '/X11 Variant:/ {print $3}')
        if [ -n "$_SYS_VARIANT" ] && [ "$_SYS_VARIANT" != "n/a" ]; then
            export XKB_DEFAULT_VARIANT="$_SYS_VARIANT"
        fi
        _SYS_OPTIONS=$(localectl status 2>/dev/null | awk -F': ' '/X11 Options:/ {print $2}')
        if [ -n "$_SYS_OPTIONS" ] && [ "$_SYS_OPTIONS" != "n/a" ]; then
            export XKB_DEFAULT_OPTIONS="$_SYS_OPTIONS"
        fi
    elif [ -f /etc/vconsole.conf ]; then
        _SYS_LAYOUT=$(grep -E '^KEYMAP=' /etc/vconsole.conf 2>/dev/null | cut -d= -f2 | tr -d '"')
        if [ -n "$_SYS_LAYOUT" ]; then
            export XKB_DEFAULT_LAYOUT="$_SYS_LAYOUT"
        fi
    fi
fi
export XKB_DEFAULT_LAYOUT="${XKB_DEFAULT_LAYOUT:-us}"
export XKB_DEFAULT_OPTIONS="${XKB_DEFAULT_OPTIONS:-}"
export FOCUS_FOLLOWS_MOUSE="${FOCUS_FOLLOWS_MOUSE:-1}"
export MOUSE_ACCEL_PROFILE="${MOUSE_ACCEL_PROFILE:-flat}"
export MOUSE_ACCEL_SPEED="${MOUSE_ACCEL_SPEED:-0.0}"
export KEYBOARD_REPEAT_DELAY="${KEYBOARD_REPEAT_DELAY:-200}"
export KEYBOARD_REPEAT_RATE="${KEYBOARD_REPEAT_RATE:-35}"
export PAN_MODE="${PAN_MODE:-free}"
export SCREENCOPY_CURSOR="${SCREENCOPY_CURSOR:-false}"
export SCALING_FILTER="${SCALING_FILTER:-nearest}"
export OUTPUT_SCALE="${OUTPUT_SCALE:-1.0}"

if [ "${INFYNITAE_SETTINGS_APPLIED:-0}" != "1" ] && [ -f "$HOME/infynitae/live/settings.sh" ]; then
    export INFYNITAE_SETTINGS_APPLIED="1"
    sh "$HOME/infynitae/live/settings.sh" apply >/dev/null 2>&1 || true
fi

# 6. Isolated D-Bus/dconf database profile (prevents inheriting host desktop defaults)
INFYNITAE_CFG_DIR="$HOME/.config/infynitae"
mkdir -p "$INFYNITAE_CFG_DIR"
if [ ! -f "$INFYNITAE_CFG_DIR/dconf-profile" ]; then
    printf 'user-db:infynitae\n' > "$INFYNITAE_CFG_DIR/dconf-profile"
fi
export DCONF_PROFILE="$INFYNITAE_CFG_DIR/dconf-profile"

# 7. XDG Desktop Portal routing & Global Shortcuts backend
PORTAL_DATA_DIR="$HOME/.local/share/xdg-desktop-portal/portals"
mkdir -p "$PORTAL_DATA_DIR"
cat << 'EOF' > "$PORTAL_DATA_DIR/infynitae.portal"
[portal]
DBusName=org.freedesktop.impl.portal.desktop.infynitae
Interfaces=org.freedesktop.impl.portal.GlobalShortcuts;
UseIn=infynitae;wlroots;
EOF

PORTAL_CFG_DIR="$HOME/.config/xdg-desktop-portal"
mkdir -p "$PORTAL_CFG_DIR"
cat << 'EOF' > "$PORTAL_CFG_DIR/infynitae-portals.conf"
[preferred]
default=gtk;wlr
org.freedesktop.impl.portal.GlobalShortcuts=infynitae
org.freedesktop.impl.portal.ScreenCast=wlr
org.freedesktop.impl.portal.Screenshot=wlr
org.freedesktop.impl.portal.Settings=gtk
org.freedesktop.impl.portal.FileChooser=gtk
org.freedesktop.impl.portal.AppChooser=gtk
org.freedesktop.impl.portal.Notification=gtk
EOF

# Ensure infynitae-portal is running
PORTAL_LOG="$HOME/.local/state/infynitae/portal.log"
mkdir -p "$HOME/.local/state/infynitae"
if [ -x "$HOME/infynitae/live/infynitae-portal" ] && ! pgrep -f "live/infynitae-portal" >/dev/null 2>&1; then
    "$HOME/infynitae/live/infynitae-portal" >> "$PORTAL_LOG" 2>&1 &
fi

# 8. Resolve WAYLAND_DISPLAY, DISPLAY, and XAUTHORITY if present
if [ -z "$WAYLAND_DISPLAY" ] && [ -n "$XDG_RUNTIME_DIR" ]; then
    for wsock in "$XDG_RUNTIME_DIR"/wayland-[0-9]*; do
        if [ -S "$wsock" ]; then
            export WAYLAND_DISPLAY="${wsock##*/}"
            break
        fi
    done
fi
if [ -n "$WAYLAND_DISPLAY" ] && [ "$WAYLAND_DISPLAY" != "wayland-0" ] && [ -n "$XDG_RUNTIME_DIR" ]; then
    if [ ! -e "$XDG_RUNTIME_DIR/wayland-0" ] && [ -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; then
        ln -sf "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" "$XDG_RUNTIME_DIR/wayland-0" 2>/dev/null || true
        ln -sf "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY.lock" "$XDG_RUNTIME_DIR/wayland-0.lock" 2>/dev/null || true
    fi
fi
if [ -z "$DISPLAY" ]; then
    for xsock in /tmp/.X11-unix/X*; do
        if [ -S "$xsock" ]; then
            num="${xsock##*/X}"
            export DISPLAY=":$num"
            break
        fi
    done
fi
if [ -z "$XAUTHORITY" ] && [ -n "$XDG_RUNTIME_DIR" ]; then
    AUTH_FILE=$(ls -t "$XDG_RUNTIME_DIR"/infynitae-xauth-* 2>/dev/null | head -1)
    if [ -f "$AUTH_FILE" ]; then
        export XAUTHORITY="$AUTH_FILE"
    fi
fi
if [ -n "$XAUTHORITY" ] && [ -f "$XAUTHORITY" ]; then
    ln -sf "$XAUTHORITY" "$HOME/.Xauthority" 2>/dev/null || true
fi

# 9. Notify D-Bus / systemd activation environment if available
if command -v dbus-update-activation-environment >/dev/null 2>&1; then
    dbus-update-activation-environment --systemd \
        WAYLAND_DISPLAY \
        DISPLAY \
        XAUTHORITY \
        XDG_CURRENT_DESKTOP \
        XDG_SESSION_DESKTOP \
        DESKTOP_SESSION \
        XDG_SESSION_TYPE \
        XCURSOR_THEME \
        XCURSOR_SIZE \
        DCONF_PROFILE \
        BROWSER 2>/dev/null || true
fi
if command -v systemctl >/dev/null 2>&1; then
    systemctl --user import-environment \
        WAYLAND_DISPLAY \
        DISPLAY \
        XAUTHORITY \
        XDG_CURRENT_DESKTOP \
        XDG_SESSION_DESKTOP \
        DESKTOP_SESSION \
        XDG_SESSION_TYPE \
        XCURSOR_THEME \
        XCURSOR_SIZE \
        DCONF_PROFILE \
        BROWSER 2>/dev/null || true
fi
