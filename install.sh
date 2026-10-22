#!/usr/bin/env bash
# Infynitae Automated Installer for Pure Arch Linux
# Supports both:
#   1. curl -fsSL https://raw.githubusercontent.com/vysaely/infynitae/main/install.sh | bash
#   2. git clone https://github.com/vysaely/infynitae.git && cd infynitae && ./install.sh
set -e

REPO_URL="https://github.com/vysaely/infynitae.git"
INSTALL_DIR="$HOME/infynitae/infynitae-native"

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
    if [ ! -d "$INSTALL_DIR" ]; then
        git clone "$REPO_URL" "$INSTALL_DIR"
    fi
    cd "$INSTALL_DIR"
    git checkout main 2>/dev/null || true
    git pull 2>/dev/null || true
else
    INSTALL_DIR="$(pwd)"
fi

# 2. Check and install core system dependencies (universal build & graphics libraries)
echo "==> Checking essential system packages..."
PKGS=()
# Universal core: build tools, mesa (universal DRM/GBM/GLES for AMD/Intel/Nvidia), libinput, seatd
for pkg in base-devel mesa libinput seatd; do
    if ! pacman -Qi "$pkg" >/dev/null 2>&1; then
        PKGS+=("$pkg")
    fi
done

# Only install rustup if neither rust nor cargo is installed
if ! command -v cargo >/dev/null 2>&1 && ! pacman -Qi rust >/dev/null 2>&1 && ! pacman -Qi rustup >/dev/null 2>&1; then
    PKGS+=("rustup")
fi

if [ ${#PKGS[@]} -gt 0 ]; then
    echo "==> Installing missing packages via pacman: ${PKGS[*]}"
    sudo pacman -S --needed --noconfirm "${PKGS[@]}"
fi

# Inform user if no terminal emulator is present
if ! command -v kitty >/dev/null 2>&1 && ! command -v alacritty >/dev/null 2>&1 && ! command -v foot >/dev/null 2>&1; then
    echo "==> Note: No terminal found. Install whatever you prefer (e.g. sudo pacman -S kitty or alacritty)."
fi

# 3. Setup Rust toolchain if rustup is used
if command -v rustup >/dev/null 2>&1 && ! command -v cargo >/dev/null 2>&1; then
    echo "==> Setting up default Rust toolchain..."
    rustup default stable
fi

# 4. Ensure seatd / libseat permission for console TTY
if command -v systemctl >/dev/null 2>&1; then
    if ! systemctl is-active --quiet seatd 2>/dev/null; then
        sudo systemctl enable --now seatd 2>/dev/null || true
    fi
    # Add current user to input and seat groups
    sudo usermod -aG input,video,seat "$USER" 2>/dev/null || true
fi

# 5. Build Infynitae in release mode
echo "==> Building Infynitae compositor in release mode..."
cargo build --release -p infynitae

# 6. Install release binary
mkdir -p "$HOME/.local/bin"
cp -f target/release/infynitae "$HOME/.local/bin/infynitae"
chmod +x "$HOME/.local/bin/infynitae"
cp -f live/dev.sh "$HOME/.local/bin/infynitae-dev"
chmod +x "$HOME/.local/bin/infynitae-dev"
cp -f live/settings.sh "$HOME/.local/bin/infynitae-settings"
chmod +x "$HOME/.local/bin/infynitae-settings"
echo "==> Installed binaries to ~/.local/bin/{infynitae, infynitae-dev, infynitae-settings}"

# 7. Deploy live runtime scripts & companion helpers
mkdir -p "$HOME/infynitae/live"
SRC_LIVE="$(cd live 2>/dev/null && pwd -P || echo "")"
DST_LIVE="$(cd "$HOME/infynitae/live" 2>/dev/null && pwd -P || echo "")"
if [ -n "$SRC_LIVE" ] && [ "$SRC_LIVE" != "$DST_LIVE" ]; then
    cp -rf live/* "$HOME/infynitae/live/"
fi
chmod +x "$HOME/infynitae/live"/*.sh "$HOME/infynitae/live"/*.py 2>/dev/null || true

# 8. Deploy default configuration
mkdir -p "$HOME/.config/infynitae"
if [ -d "config" ]; then
    SRC_CFG="$(cd config 2>/dev/null && pwd -P || echo "")"
    DST_CFG="$(cd "$HOME/.config/infynitae" 2>/dev/null && pwd -P || echo "")"
    if [ -n "$SRC_CFG" ] && [ "$SRC_CFG" != "$DST_CFG" ]; then
        cp -rf config/* "$HOME/.config/infynitae/" 2>/dev/null || true
    fi
fi

# 9. Create ~/.local/bin/infynitae-session
cat << 'EOF' > "$HOME/.local/bin/infynitae-session"
#!/bin/sh
# Infynitae Standalone Session Launcher with TTY restoration and crash guard

cleanup() {
    stty sane 2>/dev/null || true
    tput cnorm 2>/dev/null || true
}
trap cleanup EXIT INT TERM HUP
umask 077

if [ -f "$HOME/infynitae/live/env.sh" ]; then
    . "$HOME/infynitae/live/env.sh"
fi

export RUST_BACKTRACE=1
STATE_DIR="$HOME/.local/state/infynitae"
mkdir -p "$STATE_DIR"
CRASH_COUNT_FILE="$STATE_DIR/crash_count"
SESSION_LOG="$STATE_DIR/session.log"

BIN="$HOME/.local/bin/infynitae"
SESSION_SCRIPT="$HOME/infynitae/live/session.sh"

XWLS=""
if command -v xwayland-satellite >/dev/null 2>&1; then
    XWLS="--xwayland-satellite $(command -v xwayland-satellite)"
fi

START_TIME=$(date +%s)
echo "[infynitae-session] Starting Infynitae compositor..." >> "$SESSION_LOG"

# Run compositor, logging output while displaying
"$BIN" --tty $XWLS --exec "$SESSION_SCRIPT" "$@" 2>&1 | tee -a "$SESSION_LOG"
STATUS=$?

END_TIME=$(date +%s)
DURATION=$((END_TIME - START_TIME))

cleanup

if [ $STATUS -ne 0 ] || [ $DURATION -lt 5 ]; then
    COUNT=1
    if [ -f "$CRASH_COUNT_FILE" ]; then
        PREV_COUNT=$(cat "$CRASH_COUNT_FILE" 2>/dev/null || echo 0)
        COUNT=$((PREV_COUNT + 1))
    fi
    echo "$COUNT" > "$CRASH_COUNT_FILE"

    if [ "$COUNT" -ge 2 ]; then
        echo ""
        echo "=========================================================="
        echo "  [infynitae] Rapid crash loop detected ($COUNT consecutive failures)."
        echo "  Aborting auto-restart to prevent lockout."
        echo "  Dropping to rescue shell with full TTY controls."
        echo ""
        if [ -f "$STATE_DIR/crash.log" ]; then
            echo "--- Recent Crash Log ($STATE_DIR/crash.log) ---"
            tail -n 20 "$STATE_DIR/crash.log"
        elif [ -f "$SESSION_LOG" ]; then
            echo "--- Recent Session Log ($SESSION_LOG) ---"
            tail -n 20 "$SESSION_LOG"
        fi
        echo "=========================================================="
        rm -f "$CRASH_COUNT_FILE"
        exec "${SHELL:-/bin/sh}"
    fi
else
    rm -f "$CRASH_COUNT_FILE"
fi

exit $STATUS
EOF
chmod +x "$HOME/.local/bin/infynitae-session"

# 10. Ensure ~/.local/bin is in user PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$HOME/.local/bin"; then
    for prof in "$HOME/.bash_profile" "$HOME/.profile"; do
        if [ -f "$prof" ]; then
            if ! grep -q '\.local/bin' "$prof"; then
                echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$prof"
            fi
            break
        elif [ "$prof" = "$HOME/.bash_profile" ]; then
            echo 'export PATH="$HOME/.local/bin:$PATH"' > "$prof"
            break
        fi
    done
fi

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
echo "  NOTE: If this is a fresh install, please log out and back in"
echo "        (or reboot) for seat/video permissions to take effect!"
echo "=========================================================="
