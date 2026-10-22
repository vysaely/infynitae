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

# 2. Check and install core system dependencies
echo "==> Checking essential system packages..."
PKGS=()
for pkg in base-devel rustup foot wl-clipboard grim mesa vulkan-radeon libinput seatd; do
    if ! pacman -Qi "$pkg" >/dev/null 2>&1; then
        PKGS+=("$pkg")
    fi
done

if [ ${#PKGS[@]} -gt 0 ]; then
    echo "==> Installing missing packages via pacman: ${PKGS[*]}"
    sudo pacman -S --needed --noconfirm "${PKGS[@]}"
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

# 7. Deploy live runtime scripts & Quickshell dynamic island
mkdir -p "$HOME/infynitae/live"
cp -rf live/* "$HOME/infynitae/live/"
chmod +x "$HOME/infynitae/live"/*.sh "$HOME/infynitae/live"/*.py 2>/dev/null || true

# 8. Deploy default configuration
mkdir -p "$HOME/.config/infynitae"
if [ -d "config" ]; then
    cp -rf config/* "$HOME/.config/infynitae/" 2>/dev/null || true
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

# 10. Configure auto-start on TTY1 for Fish shell
if [ -f "$HOME/.config/fish/config.fish" ]; then
    if ! grep -q "infynitae-session" "$HOME/.config/fish/config.fish"; then
        cat << 'EOF' >> "$HOME/.config/fish/config.fish"

# Auto-start Infynitae on TTY1 console login
if status is-login
    if test -z "$WAYLAND_DISPLAY" -a "$XDG_VTNR" = 1
        exec infynitae-session
    end
end
EOF
        echo "==> Configured TTY1 auto-start in ~/.config/fish/config.fish"
    fi
fi

# 11. Configure auto-start for Bash/Zsh login
for profile in "$HOME/.bash_profile" "$HOME/.zprofile"; do
    if [ -f "$profile" ] && ! grep -q "infynitae-session" "$profile"; then
        cat << 'EOF' >> "$profile"

# Auto-start Infynitae on TTY1 console login
if [ -z "$WAYLAND_DISPLAY" ] && [ "$(tty)" = "/dev/tty1" ]; then
    exec infynitae-session
fi
EOF
        echo "==> Configured TTY1 auto-start in $profile"
    fi
done

# 12. Create Wayland session desktop entry
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
echo "  Run standalone anytime:  infynitae-session"
echo "  Or simply log in on TTY1 console."
echo ""
echo "  Keybindings:"
echo "    Super + 1..9         Focus Window Slot 1..9"
echo "    Super + Shift + 1..9 Focus Cluster Slot 1..9"
echo "    Super + Space        Recenter Active Window"
echo "    Super + Enter        Fit Window to Screen"
echo "    Super + Q            Terminal (foot)"
echo "    Super + E            Files (yazi)"
echo "    Super + W            Dynamic Island Launcher"
echo "    Super + Shift + S    Screenshot"
echo "    Super + Shift + Tab  Canvas Overview & Radar"
echo "    Super + Shift + R    Hot-Reload Shell UI & Scripts"
echo "=========================================================="
echo "  Live Tweak Utilities:"
echo "    infynitae-dev test   Run nested sandbox to test live changes"
echo "    infynitae-dev logs   Stream live session, shell & crash logs"
echo "    infynitae-dev build  Rebuild release and update binary"
echo "    infynitae-dev check  Run test suite and architecture check"
echo "=========================================================="
