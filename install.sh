#!/usr/bin/env bash
# Infynitae Automated Installer for Pure Arch Linux
# Supports both:
#   1. curl -fsSL https://raw.githubusercontent.com/vysaely/infynitae/main/install.sh | bash
#   2. git clone https://github.com/vysaely/infynitae.git && cd infynitae && ./install.sh
set -e

REPO_URL="https://github.com/vysaely/infynitae.git"
INSTALL_DIR="$HOME/infynitae"

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
# polkit, fast app launcher (fuzzel), fonts, clipboard & screenshots, and Xwayland for Steam/games
for pkg in base-devel libinput xdg-desktop-portal xdg-desktop-portal-wlr pipewire wireplumber pipewire-pulse polkit fuzzel ttf-dejavu grim slurp wl-clipboard xorg-xwayland xwayland-satellite; do
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

# Only install rustup if neither rust nor cargo is installed
if ! command -v cargo >/dev/null 2>&1 && ! pacman -Qi rust >/dev/null 2>&1 && ! pacman -Qi rustup >/dev/null 2>&1; then
    PKGS+=("rustup")
fi

if [ ${#PKGS[@]} -gt 0 ]; then
    echo "==> Installing missing packages via pacman: ${PKGS[*]}"
    sudo pacman -S --needed --noconfirm "${PKGS[@]}"
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
    sudo usermod -aG video "$USER" 2>/dev/null || true
fi

# GPU, joystick and uinput access come from systemd (70-uaccess) and the steam package
# (60-steam-input); Infynitae needs no udev rules of its own.

# 5. Build Infynitae in release mode
echo "==> Building Infynitae compositor in release mode..."
cargo build --release -p infynitae

# 6. Install release binary
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/infynitae "$HOME/.local/bin/infynitae"
install -m 755 live/dev.sh "$HOME/.local/bin/infynitae-dev"
install -m 755 live/settings.sh "$HOME/.local/bin/infynitae-settings"
install -m 755 live/setup-drivers.sh "$HOME/.local/bin/infynitae-drivers"
install -m 755 update.sh "$HOME/.local/bin/infynitae-update"
if [ -f "live/wallpaper/infynitae-wallpaper" ]; then
    install -m 755 live/wallpaper/infynitae-wallpaper "$HOME/.local/bin/infynitae-wallpaper"
fi
echo "==> Installed binaries to ~/.local/bin/{infynitae, infynitae-dev, infynitae-settings, infynitae-drivers, infynitae-update, infynitae-wallpaper}"

# 7. Deploy live runtime scripts & companion helpers
mkdir -p "$HOME/infynitae/live"
SRC_LIVE="$(cd live 2>/dev/null && pwd -P || echo "")"
DST_LIVE="$(cd "$HOME/infynitae/live" 2>/dev/null && pwd -P || echo "")"
if [ -n "$SRC_LIVE" ] && [ "$SRC_LIVE" != "$DST_LIVE" ]; then
    cp -rf live/* "$HOME/infynitae/live/"
fi
if [ -f "$HOME/infynitae/live/portal/infynitae-portal.c" ] && pkg-config --exists gio-2.0 2>/dev/null; then
    gcc -O2 "$HOME/infynitae/live/portal/infynitae-portal.c" $(pkg-config --cflags --libs gio-2.0) -o "$HOME/.local/bin/infynitae-portal" 2>/dev/null || true
    chmod +x "$HOME/.local/bin/infynitae-portal" 2>/dev/null || true
fi
if [ -f "$HOME/infynitae/live/polkit/infynitae-polkit.c" ] && pkg-config --exists polkit-agent-1 gtk+-3.0 2>/dev/null; then
    gcc -O2 "$HOME/infynitae/live/polkit/infynitae-polkit.c" $(pkg-config --cflags --libs polkit-agent-1 gtk+-3.0) -o "$HOME/.local/bin/infynitae-polkit" 2>/dev/null || true
    chmod +x "$HOME/.local/bin/infynitae-polkit" 2>/dev/null || true
fi
if [ -f "$HOME/infynitae/live/lock/infynitae-lock.c" ] && pkg-config --exists wayland-client cairo pangocairo xkbcommon pam 2>/dev/null; then
    gcc -O2 "$HOME/infynitae/live/lock/infynitae-lock.c" "$HOME/infynitae/live/lock/ext-session-lock-v1-protocol.c" \
        $(pkg-config --cflags --libs wayland-client cairo pangocairo xkbcommon pam) -lm -lpthread \
        -o "$HOME/.local/bin/infynitae-lock" 2>/dev/null || true
    chmod +x "$HOME/.local/bin/infynitae-lock" 2>/dev/null || true
fi
if [ -f "$HOME/infynitae/live/wallpaper/infynitae-wallpaper.c" ] && pkg-config --exists wayland-client cairo gdk-pixbuf-2.0 2>/dev/null; then
    gcc -O2 "$HOME/infynitae/live/wallpaper/infynitae-wallpaper.c" "$HOME/infynitae/live/wallpaper/wlr-layer-shell-protocol.c" "$HOME/infynitae/live/wallpaper/xdg-shell-protocol.c" \
        $(pkg-config --cflags --libs wayland-client cairo gdk-pixbuf-2.0) -lm \
        -o "$HOME/.local/bin/infynitae-wallpaper" 2>/dev/null || true
    chmod +x "$HOME/.local/bin/infynitae-wallpaper" 2>/dev/null || true
fi
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

# Also install into /usr/local/bin so commands work immediately across all users and display managers
sudo mkdir -p /usr/local/bin 2>/dev/null || true
for b in infynitae infynitae-session infynitae-lock infynitae-dev infynitae-settings infynitae-drivers infynitae-update infynitae-wallpaper infynitae-portal infynitae-polkit xdg-open; do
    if [ -f "$HOME/.local/bin/$b" ]; then
        sudo install -m 755 "$HOME/.local/bin/$b" "/usr/local/bin/$b" 2>/dev/null || true
    fi
done

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

echo ""
echo "=========================================================="
echo "        INFYNITAE INSTALLATION COMPLETE!"
echo "=========================================================="
echo "  Run anytime:  infynitae-session"
echo ""
echo "  Keybindings:"
echo "    Super + 1..9         Focus Window Slot 1..9"
echo "    Super + Shift + 1..9 Focus Cluster Slot 1..9"
echo "    Super + Space        Recenter Active Window"
echo "    Super + Enter        Fit Window to Screen"
echo "    Super + Shift + Enter Fullscreen"
echo "    Super + Q            Terminal (auto: kitty, alacritty, foot)"
echo "    Super + W            Launcher (fuzzel / dmenu)"
echo "    Super + E            Files (yazi)"
echo "    Super + Shift + S    Screenshot"
echo "    Super + Shift + R    Reload Config"
echo "    Super + Delete       Close Window"
echo "    Ctrl + Alt + Backspace Exit Compositor"
echo "=========================================================="
echo "  [✓] Hardware & input permissions are active immediately!"
echo "      (Zero reboot or logout needed — udev uaccess & ACLs are active)"
echo ""
echo "  Start Infynitae right now with:"
echo "      infynitae-session"
echo "=========================================================="
