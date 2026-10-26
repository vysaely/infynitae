#!/usr/bin/env bash
# Infynitae Automated Installer for Pure Arch Linux
# Supports both:
#   1. curl -fsSL https://raw.githubusercontent.com/vysaely/infynitae/main/install.sh | bash
#   2. git clone https://github.com/vysaely/infynitae.git && cd infynitae && ./install.sh
set -e

REPO_URL="https://github.com/vysaely/infynitae.git"
INSTALL_DIR="$HOME/infynitae"

# Without a display manager the machine boots straight to the Infynitae lock screen (tty1
# autologin, the session started locked; live/setup-autologin.sh). --no-autologin keeps the text
# login instead (log in on tty1 and Infynitae starts). Through curl: ... | bash -s -- --no-autologin
AUTOLOGIN=true
for arg in "$@"; do
    case "$arg" in
        --no-autologin) AUTOLOGIN=false ;;
        *) echo "unknown option: $arg (known: --no-autologin)" >&2; exit 2 ;;
    esac
done

if [ "$(id -u)" -eq 0 ]; then
    echo "Run the installer as your own user, not root: it asks for your password itself." >&2
    exit 1
fi

# One password, at the start: everything after it runs unattended. sudo's permission is kept
# fresh in the background while the installer runs (the build takes minutes) and no longer.
echo "==> Infynitae needs your password once (packages, drivers, the login setup)."
sudo -v
( while kill -0 "$$" 2>/dev/null; do sudo -n true; sleep 50; done ) >/dev/null 2>&1 &
SUDO_KEEPER=$!
trap 'kill "$SUDO_KEEPER" 2>/dev/null' EXIT

echo "=========================================================="
echo "      INFYNITAE - PURE ARCH LINUX INSTALLER"
echo "=========================================================="

# 1. Ensure git and source repository
if [ ! -f "Cargo.toml" ] || [ ! -d "crates/infynitae" ]; then
    echo "==> Cloning Infynitae repository..."
    if ! command -v git >/dev/null 2>&1; then
        echo "==> Installing git..."
        sudo pacman -S --needed --noconfirm git
    fi
    mkdir -p "$(dirname "$INSTALL_DIR")"
    if [ ! -d "$INSTALL_DIR/.git" ]; then
        git clone "$REPO_URL" "$INSTALL_DIR"
    fi
    cd "$INSTALL_DIR"
    git checkout main 2>/dev/null || true
    git pull 2>/dev/null || true
else
    INSTALL_DIR="$(pwd)"
fi

# 2. Run universal hardware detection & graphics driver setup (Vulkan, 32-bit, Xwayland)
if [ -f "live/setup-drivers.sh" ]; then
    chmod +x live/setup-drivers.sh
    ./live/setup-drivers.sh
elif [ -f "$INSTALL_DIR/live/setup-drivers.sh" ]; then
    chmod +x "$INSTALL_DIR/live/setup-drivers.sh"
    "$INSTALL_DIR/live/setup-drivers.sh"
fi

echo "==> Checking essential system packages..."
PKGS=()
# Barebones essentials: build tools, libinput, portals (screenshare/dialogs), audio stack (PipeWire),
# polkit, fast app launcher (fuzzel), fonts, clipboard & screenshots, and Xwayland for Steam/games.
# Build and runtime libraries: seatd provides libseat (linked by the compositor, the daemon is not
# enabled), mesa (GBM/EGL), and the helpers' cairo/pango/gdk-pixbuf2/gtk3; ffmpeg for the recorder.
for pkg in $(cat "$INSTALL_DIR/live/packages.txt" 2>/dev/null || cat live/packages.txt); do
    if ! pacman -Qi "$pkg" >/dev/null 2>&1; then
        PKGS+=("$pkg")
    fi
done

# Ensure at least one fast Wayland terminal is installed for an out-of-the-box experience
if ! command -v kitty >/dev/null 2>&1 && ! command -v alacritty >/dev/null 2>&1 && ! command -v foot >/dev/null 2>&1; then
    echo "==> No terminal emulator detected. Adding alacritty for out-of-the-box terminal support..."
    if ! pacman -Qi alacritty >/dev/null 2>&1; then
        PKGS+=("alacritty")
    fi
fi

# Hardware the control menu (Super+A) can drive, only where it exists. NetworkManager is not added:
# a machine on iwd or systemd-networkd keeps its own networking, and the menu uses what is there.
if [ -n "$(ls /sys/class/bluetooth 2>/dev/null)" ]; then
    for pkg in bluez bluez-utils; do
        pacman -Qi "$pkg" >/dev/null 2>&1 || PKGS+=("$pkg")
    done
    ENABLE_BLUETOOTH=true
fi
if ls -d /sys/class/power_supply/BAT* >/dev/null 2>&1 && ! pacman -Qi tlp >/dev/null 2>&1; then
    # power modes on a laptop; tlp, when installed, manages power instead
    pacman -Qi power-profiles-daemon >/dev/null 2>&1 || PKGS+=("power-profiles-daemon")
    ENABLE_POWER_PROFILES=true
fi

# Only install rustup if neither rust nor cargo is installed
if ! command -v cargo >/dev/null 2>&1 && ! pacman -Qi rust >/dev/null 2>&1 && ! pacman -Qi rustup >/dev/null 2>&1; then
    PKGS+=("rustup")
fi

if [ ${#PKGS[@]} -gt 0 ]; then
    echo "==> Installing missing packages via pacman: ${PKGS[*]}"
    sudo pacman -S --needed --noconfirm "${PKGS[@]}"
fi
if [ "${ENABLE_BLUETOOTH:-false}" = true ] && ! systemctl is-enabled --quiet bluetooth.service 2>/dev/null; then
    echo "==> Enabling Bluetooth"
    sudo systemctl enable --now bluetooth.service || echo "==> Could not enable bluetooth.service"
fi
if [ "${ENABLE_POWER_PROFILES:-false}" = true ] && ! systemctl is-enabled --quiet power-profiles-daemon.service 2>/dev/null; then
    echo "==> Enabling power modes"
    sudo systemctl enable --now power-profiles-daemon.service || echo "==> Could not enable power-profiles-daemon.service"
fi

# 3. Setup Rust toolchain if rustup is used
if command -v rustup >/dev/null 2>&1 && ! rustup show active-toolchain >/dev/null 2>&1; then
    echo "==> Setting up default Rust toolchain..."
    rustup default stable
fi

# 4. Ensure seat and hardware permissions (Standard Systemd Logind / Seatd architecture)
# Wayland compositors (like Sway, GNOME, KDE, and Infynitae) open input devices and GPUs
# via libseat and systemd-logind. This securely passes device file descriptors without requiring
# the user to be in the raw 'input' group or granting arbitrary user-space processes access to keystrokes.
echo "==> Configuring hardware access and libseat integration..."
if command -v systemctl >/dev/null 2>&1; then
    if ! systemctl is-active --quiet systemd-logind 2>/dev/null; then
        sudo systemctl enable --now seatd 2>/dev/null || true
        sudo usermod -aG seat "$USER" 2>/dev/null || true
    fi
fi

# GPU, joystick and uinput access come from systemd (70-uaccess) and the steam package
# (60-steam-input); Infynitae needs no udev rules of its own.

# 5. Build Infynitae in release mode
echo "==> Building Infynitae compositor in release mode..."
cargo build --release -p infynitae

# 6. Install release binary
mkdir -p "$HOME/.local/bin"
install -m 755 "${CARGO_TARGET_DIR:-target}/release/infynitae" "$HOME/.local/bin/infynitae"
install -m 755 live/dev.sh "$HOME/.local/bin/infynitae-dev"
install -m 755 live/settings.sh "$HOME/.local/bin/infynitae-settings"
install -m 755 live/setup-drivers.sh "$HOME/.local/bin/infynitae-drivers"
install -m 755 update.sh "$HOME/.local/bin/infynitae-update"
install -m 755 live/infynitae-reload "$HOME/.local/bin/infynitae-reload"
install -m 755 live/infynitae-restore "$HOME/.local/bin/infynitae-restore"
install -m 755 live/record/infynitae-recorder "$HOME/.local/bin/infynitae-recorder"
echo "==> Installed binaries to ~/.local/bin"

# 7. Deploy live runtime scripts & companion helpers
mkdir -p "$HOME/infynitae/live"
SRC_LIVE="$(cd live 2>/dev/null && pwd -P || echo "")"
DST_LIVE="$(cd "$HOME/infynitae/live" 2>/dev/null && pwd -P || echo "")"
if [ -n "$SRC_LIVE" ] && [ "$SRC_LIVE" != "$DST_LIVE" ]; then
    cp -rf live/* "$HOME/infynitae/live/"
fi
# The C helpers (portal, polkit agent, lock screen, wallpaper, recorder stream): one recipe
# shared with infynitae-update; a failure is reported, never hidden.
sh "$HOME/infynitae/live/build-helpers.sh" "$HOME/infynitae" "$HOME/.local/bin" \
    || echo "==> Some helpers were not built (see above); Infynitae runs without them"
if command -v sudo >/dev/null 2>&1 && [ -f "$HOME/infynitae/live/lock/infynitae.pam" ] && [ ! -f /etc/pam.d/infynitae ]; then
    sudo cp -f "$HOME/infynitae/live/lock/infynitae.pam" /etc/pam.d/infynitae 2>/dev/null || true
fi
chmod +x "$HOME/infynitae/live"/*.sh "$HOME/infynitae/live"/*.py 2>/dev/null || true

# Deploy XDG Desktop Portal GlobalShortcuts backend definition
mkdir -p "$HOME/.local/share/xdg-desktop-portal/portals"
cat << 'EOF' > "$HOME/.local/share/xdg-desktop-portal/portals/infynitae.portal"
[portal]
DBusName=org.freedesktop.impl.portal.desktop.infynitae
Interfaces=org.freedesktop.impl.portal.GlobalShortcuts;
UseIn=infynitae;wlroots;
EOF

# 8. Deploy default configuration safely (preserving user changes)
mkdir -p "$HOME/.config/infynitae"
if [ -f "config/config" ]; then
    if [ ! -f "$HOME/.config/infynitae/config" ]; then
        cp -f "config/config" "$HOME/.config/infynitae/config"
        echo "==> Created default configuration at ~/.config/infynitae/config"
    else
        cp -f "config/config" "$HOME/.config/infynitae/config.template"
        echo "==> User config preserved at ~/.config/infynitae/config (template saved to config.template)"
    fi
fi

# 9. Install ~/.local/bin/infynitae-session (single source: live/infynitae-session)
cp -f "$HOME/infynitae/live/infynitae-session" "$HOME/.local/bin/infynitae-session"
chmod +x "$HOME/.local/bin/infynitae-session"
cp -f "$HOME/infynitae/live/xdg-open" "$HOME/.local/bin/xdg-open" 2>/dev/null || true
chmod +x "$HOME/.local/bin/xdg-open" 2>/dev/null || true

# Display managers start sessions without ~/.local/bin on PATH: /usr/local/bin gets a tiny
# launcher that runs the user's own copy (copies of the binaries there would go stale)
sudo install -D -m 755 "$HOME/infynitae/live/infynitae-session-shim" /usr/local/bin/infynitae-session

# 10. Ensure ~/.local/bin is in user PATH across interactive and login shells
for prof in "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.profile"; do
    if [ -f "$prof" ] && ! grep -q '\.local/bin' "$prof"; then
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$prof"
    fi
done

# 11. Create Wayland session desktop entry (selectable in login display manager)
mkdir -p "$HOME/.local/share/wayland-sessions"
cat << 'EOF' > "$HOME/.local/share/wayland-sessions/infynitae.desktop"
[Desktop Entry]
Name=Infynitae
Comment=Infynitae Infinite Canvas Wayland Compositor
Exec=infynitae-session
Type=Application
DesktopNames=infynitae;wlroots
EOF

# Also make available system-wide for display managers (GDM, SDDM, LightDM)
sudo mkdir -p /usr/share/wayland-sessions 2>/dev/null || true
sudo cp -f "$HOME/.local/share/wayland-sessions/infynitae.desktop" /usr/share/wayland-sessions/ 2>/dev/null || true

# 12. RustDesk, when installed: the session restarts its service once the compositor runs, so it
#     sees a Wayland session (live/session.sh); this rule allows that one restart, nothing else
if systemctl cat rustdesk.service >/dev/null 2>&1; then
    sudo install -D -m 644 "$HOME/infynitae/live/remote/50-infynitae-rustdesk.rules" \
        /etc/polkit-1/rules.d/50-infynitae-rustdesk.rules
    echo "==> RustDesk: allowed the session to restart its service"
fi

# 13. How Infynitae starts after a reboot: a display manager's session list; else tty1 logs in by
#     itself to the Infynitae lock screen (or, with --no-autologin, or when the lock screen could
#     not be installed, logging in on tty1 starts Infynitae)
if systemctl is-enabled --quiet display-manager.service 2>/dev/null; then
    START_HOW="pick \"Infynitae\" at your display manager's login"
elif [ "$AUTOLOGIN" = true ] && sudo sh "$HOME/infynitae/live/setup-autologin.sh"; then
    START_HOW="reboot: the machine starts at the Infynitae lock screen"
else
    sh "$HOME/infynitae/live/setup-autologin.sh" --autostart
    START_HOW="reboot (or log out) and log in on tty1: Infynitae starts"
fi

echo ""
echo "=========================================================="
echo "        INFYNITAE INSTALLATION COMPLETE!"
echo "=========================================================="
echo "  Start: $START_HOW"
echo "  Text login instead of the lock screen: sudo ~/infynitae/live/setup-autologin.sh --off"
echo ""
echo "  Keybindings:"
echo "    Super + 1..9         Focus Window Slot 1..9"
echo "    Super + Shift + 1..9 Focus Cluster Slot 1..9"
echo "    Super + Space        Recenter Active Window (Shift: keep zoom)"
echo "    Super + Tab          Next Window (Shift: overview)"
echo "    Super + Enter        Fit Window to Screen"
echo "    Super + Shift + Enter Fullscreen"
echo "    Super + Q            Terminal (auto: kitty, alacritty, foot)"
echo "    Super + W            Launcher (auto: fuzzel, rofi, …)"
echo "    Super + E            Files"
echo "    Super + L            Lock Screen"
echo "    Super + V            Clipboard History"
echo "    Super + A            Control Menu (Wi-Fi, Bluetooth, sound, power)"
echo "    Super + Shift + P    Hide Window from Screenshots & Sharing"
echo "    Super + Alt + C      Save Replay Clip"
echo "    Super + Shift + S    Screenshot"
echo "    Super + Shift + R    Region Screenshot"
echo "    Super + Shift + Backspace  Reload Config & Helpers"
echo "    Super + C            Close Window (Shift: force)"
echo "    Super + Middle drag  Pan the canvas"
echo "    Ctrl + Alt + Backspace Restart Compositor (infynitae-session stop: quit)"
echo "=========================================================="
