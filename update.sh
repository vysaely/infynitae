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
    for pkg in base-devel libinput seatd xdg-desktop-portal xdg-desktop-portal-wlr xdg-desktop-portal-gtk pipewire wireplumber polkit swaybg grim slurp wl-clipboard xorg-xwayland xwayland-satellite; do
        if ! pacman -Qi "$pkg" >/dev/null 2>&1; then
            NEW_PKGS+=("$pkg")
        fi
    done
    if [ ${#NEW_PKGS[@]} -gt 0 ]; then
        echo "==> Installing missing packages: ${NEW_PKGS[*]}"
        sudo pacman -S --needed --noconfirm "${NEW_PKGS[@]}"
    fi
fi

# 3. Build optimized release binary
echo "==> Compiling latest Infynitae release binary..."
cargo build --release -p infynitae

# 4. Install binaries to ~/.local/bin and /usr/local/bin
mkdir -p "$HOME/.local/bin"
cp -f target/release/infynitae "$HOME/.local/bin/infynitae"
chmod +x "$HOME/.local/bin/infynitae"

cp -f live/dev.sh "$HOME/.local/bin/infynitae-dev"
chmod +x "$HOME/.local/bin/infynitae-dev"

cp -f live/settings.sh "$HOME/.local/bin/infynitae-settings"
chmod +x "$HOME/.local/bin/infynitae-settings"

cp -f live/setup-drivers.sh "$HOME/.local/bin/infynitae-drivers"
chmod +x "$HOME/.local/bin/infynitae-drivers"

cp -f update.sh "$HOME/.local/bin/infynitae-update"
chmod +x "$HOME/.local/bin/infynitae-update"

cp -f live/wallpaper/infynitae-wallpaper.sh "$HOME/.local/bin/infynitae-wallpaper"
chmod +x "$HOME/.local/bin/infynitae-wallpaper"

cp -f live/xdg-open "$HOME/.local/bin/xdg-open" 2>/dev/null || true
chmod +x "$HOME/.local/bin/xdg-open" 2>/dev/null || true

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
    sudo mkdir -p /usr/local/bin 2>/dev/null || true
    for b in infynitae infynitae-session infynitae-dev infynitae-settings infynitae-drivers infynitae-update infynitae-wallpaper infynitae-portal infynitae-polkit xdg-open; do
        if [ -f "$HOME/.local/bin/$b" ]; then
            sudo cp -f "$HOME/.local/bin/$b" /usr/local/bin/ 2>/dev/null || true
        fi
    done
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
