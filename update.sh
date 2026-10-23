#!/usr/bin/env bash
# ==============================================================================
# INFYNITAE - UNIVERSAL COMPOSITOR & SYSTEM UPDATER
#
# Single-command update: pulls latest code, builds optimized release binaries,
# syncs runtime companion scripts, and preserves all user configurations.
# ==============================================================================
set -e

# Detect Infynitae repository location
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd -P)"
if [ ! -f "$REPO_DIR/Cargo.toml" ]; then
    if [ -d "$HOME/infynitae" ] && [ -f "$HOME/infynitae/Cargo.toml" ]; then
        REPO_DIR="$HOME/infynitae"
    else
        echo "[infynitae-update] Error: Cannot find Infynitae repository directory." >&2
        exit 1
    fi
fi

cd "$REPO_DIR"

echo "=========================================================="
echo "        INFYNITAE - UNIVERSAL COMPOSITOR UPDATER"
echo "=========================================================="
echo "==> Working in: $REPO_DIR"

# 1. Pull latest commits from GitHub
if [ -d ".git" ]; then
    echo "==> Fetching latest changes from remote repository..."
    CURRENT_BRANCH=$(git branch --show-current 2>/dev/null || echo "main")
    # Stash uncommitted changes safely if any
    HAS_DIFF=false
    if ! git diff-index --quiet HEAD -- 2>/dev/null; then
        echo "==> Stashing local working tree changes..."
        git stash push -m "infynitae-auto-update-$(date +%s)" >/dev/null 2>&1 || true
        HAS_DIFF=true
    fi

    echo "==> Pulling branch: $CURRENT_BRANCH..."
    git pull origin "$CURRENT_BRANCH" || {
        echo "[infynitae-update] Warning: git pull failed (offline or remote mismatch). Continuing build..."
    }

    if [ "$HAS_DIFF" = true ]; then
        echo "==> Re-applying local changes..."
        git stash pop >/dev/null 2>&1 || true
    fi
fi

# 2. Check and install any newly added essential packages
if command -v pacman >/dev/null 2>&1; then
    NEW_PKGS=()
    for pkg in base-devel libinput seatd xdg-desktop-portal xdg-desktop-portal-wlr xdg-desktop-portal-gtk pipewire wireplumber pipewire-pulse polkit fuzzel swaylock libnotify brightnessctl playerctl ttf-dejavu wl-clip-persist qt5-wayland qt6-wayland swaybg grim slurp wl-clipboard xorg-xwayland xwayland-satellite; do
        if ! pacman -Qi "$pkg" >/dev/null 2>&1; then
            NEW_PKGS+=("$pkg")
        fi
    done
    if ! command -v kitty >/dev/null 2>&1 && ! command -v alacritty >/dev/null 2>&1 && ! command -v foot >/dev/null 2>&1; then
        if ! pacman -Qi alacritty >/dev/null 2>&1; then
            NEW_PKGS+=("alacritty")
        fi
    fi
    if [ ${#NEW_PKGS[@]} -gt 0 ]; then
        echo "==> Installing missing packages: ${NEW_PKGS[*]}"
        sudo pacman -S --needed --noconfirm "${NEW_PKGS[@]}"
    fi
fi

# Ensure zero-reboot udev device permissions
if [ -d "/etc/udev/rules.d" ] && [ ! -f "/etc/udev/rules.d/71-infynitae-devices.rules" ]; then
    if command -v sudo >/dev/null 2>&1; then
        echo "==> Deploying zero-reboot hardware access rules (udev uaccess)..."
        sudo tee /etc/udev/rules.d/71-infynitae-devices.rules >/dev/null << 'EOF'
# Infynitae Compositor: grant the active local seat user direct access to input, uinput, and DRM nodes
SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"
SUBSYSTEM=="misc", KERNEL=="uinput", TAG+="uaccess", OPTIONS+="static_node=uinput"
SUBSYSTEM=="drm", KERNEL=="card*", TAG+="uaccess"
SUBSYSTEM=="drm", KERNEL=="renderD*", TAG+="uaccess"
EOF
        sudo udevadm control --reload-rules 2>/dev/null || true
        sudo udevadm trigger --subsystem-match=input 2>/dev/null || true
        sudo udevadm trigger --subsystem-match=drm 2>/dev/null || true
        sudo udevadm trigger --subsystem-match=misc 2>/dev/null || true
    fi
fi

# 3. Build optimized release binary
echo "==> Compiling latest Infynitae release binary..."
cargo build --release -p infynitae

# 4. Install binaries to ~/.local/bin and /usr/local/bin
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/infynitae "$HOME/.local/bin/infynitae"
install -m 755 live/dev.sh "$HOME/.local/bin/infynitae-dev"
install -m 755 live/settings.sh "$HOME/.local/bin/infynitae-settings"
install -m 755 live/setup-drivers.sh "$HOME/.local/bin/infynitae-drivers"
install -m 755 update.sh "$HOME/.local/bin/infynitae-update"
install -m 755 live/wallpaper/infynitae-wallpaper.sh "$HOME/.local/bin/infynitae-wallpaper"
if [ -f "live/xdg-open" ]; then
    install -m 755 live/xdg-open "$HOME/.local/bin/xdg-open" 2>/dev/null || true
fi

# 5. Compile companion helpers (polkit & portal)
if [ -f "$REPO_DIR/live/portal/infynitae-portal.c" ] && pkg-config --exists gio-2.0 2>/dev/null; then
    echo "==> Compiling native desktop portal backend..."
    gcc -O2 "$REPO_DIR/live/portal/infynitae-portal.c" $(pkg-config --cflags --libs gio-2.0) -o "$HOME/.local/bin/infynitae-portal" 2>/dev/null || true
    chmod +x "$HOME/.local/bin/infynitae-portal" 2>/dev/null || true
fi

if [ -f "$REPO_DIR/live/polkit/infynitae-polkit.c" ] && pkg-config --exists polkit-agent-1 gtk+-3.0 2>/dev/null; then
    echo "==> Compiling native Polkit authentication agent..."
    gcc -O2 "$REPO_DIR/live/polkit/infynitae-polkit.c" $(pkg-config --cflags --libs polkit-agent-1 gtk+-3.0) -o "$HOME/.local/bin/infynitae-polkit" 2>/dev/null || true
    chmod +x "$HOME/.local/bin/infynitae-polkit" 2>/dev/null || true
fi

# 6. Deploy live companion scripts
mkdir -p "$HOME/infynitae/live"
SRC_LIVE="$(cd live 2>/dev/null && pwd -P || echo "")"
DST_LIVE="$(cd "$HOME/infynitae/live" 2>/dev/null && pwd -P || echo "")"
if [ -n "$SRC_LIVE" ] && [ "$SRC_LIVE" != "$DST_LIVE" ]; then
    cp -rf live/* "$HOME/infynitae/live/"
fi
chmod +x "$HOME/infynitae/live"/*.sh "$HOME/infynitae/live"/*.py 2>/dev/null || true

# 7. Update system-wide binaries if sudo permission is available
if command -v sudo >/dev/null 2>&1; then
    sudo mkdir -p /usr/local/bin /usr/bin 2>/dev/null || true
    for b in infynitae infynitae-session infynitae-dev infynitae-settings infynitae-drivers infynitae-update infynitae-wallpaper infynitae-portal infynitae-polkit xdg-open; do
        if [ -f "$HOME/.local/bin/$b" ]; then
            sudo install -m 755 "$HOME/.local/bin/$b" "/usr/local/bin/$b" 2>/dev/null || true
        fi
    done
    sudo ln -sf /usr/local/bin/infynitae-session /usr/bin/infynitae-session 2>/dev/null || true
    sudo ln -sf /usr/local/bin/infynitae /usr/bin/infynitae 2>/dev/null || true
fi

# 8. Safely synchronize configuration templates without overwriting user settings
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

# 9. Update desktop entry
mkdir -p "$HOME/.local/share/wayland-sessions"
cat << 'EOF' > "$HOME/.local/share/wayland-sessions/infynitae.desktop"
[Desktop Entry]
Name=Infynitae
Comment=High-Performance Infinite Canvas Wayland Compositor
Exec=infynitae-session
Type=Application
DesktopNames=infynitae;wlroots
EOF
if command -v sudo >/dev/null 2>&1; then
    sudo mkdir -p /usr/share/wayland-sessions 2>/dev/null || true
    sudo cp -f "$HOME/.local/share/wayland-sessions/infynitae.desktop" /usr/share/wayland-sessions/ 2>/dev/null || true
fi

# 10. Notify user if desktop is currently running
if [ -n "$WAYLAND_DISPLAY" ]; then
    if [ -x "$HOME/.local/bin/infynitae-settings" ]; then
        "$HOME/.local/bin/infynitae-settings" apply >/dev/null 2>&1 || true
    fi
    if command -v notify-send >/dev/null 2>&1; then
        notify-send -a Infynitae -u normal "Infynitae Updated" "Compositor and tools updated to latest version." 2>/dev/null || true
    fi
fi

echo ""
echo "=========================================================="
echo "          INFYNITAE UPDATE COMPLETE!"
echo "=========================================================="
echo "Infynitae is up to date and ready to use."
echo "To update anytime in the future, simply run: infynitae-update"
echo "=========================================================="
