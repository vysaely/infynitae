#!/usr/bin/env bash
# ==============================================================================
# INFYNITAE - UNIVERSAL COMPOSITOR & SYSTEM UPDATER
#
# Single-command update: pulls latest code, builds optimized release binaries,
# syncs runtime companion scripts, and preserves all user configurations.
# ==============================================================================
set -e
# files it creates are readable by everyone, like any installed file: the session's private
# umask (077) would make /etc/pam.d/infynitae unreadable to the lock screen, and the session
# entry unreadable to a display manager
umask 022

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

# the shared, unattended pacman, the parts left out at install, and one password for the run
. "$REPO_DIR/live/install-lib.sh"
load_install_choices
echo "==> infynitae-update needs your password once (new packages, system files)."
sudo_once
trap 'kill "${SUDO_KEEPER:-}" 2>/dev/null || true' EXIT

echo "=========================================================="
echo "        INFYNITAE - UNIVERSAL COMPOSITOR UPDATER"
echo "=========================================================="
echo "==> Working in: $REPO_DIR"

# 1. Pull latest commits from GitHub
if [ -d ".git" ]; then
    echo "==> Fetching latest changes from remote repository..."
    CURRENT_BRANCH=$(git branch --show-current 2>/dev/null || echo "main")
    # Stash uncommitted changes safely if any
    PUSHED_STASH=false
    if ! git diff-index --quiet HEAD -- 2>/dev/null; then
        echo "==> Stashing local working tree changes..."
        STASH_BEFORE=$(git rev-parse -q --verify refs/stash 2>/dev/null || true)
        if git stash push -m "infynitae-auto-update-$(date +%s)" >/dev/null 2>&1; then
            STASH_AFTER=$(git rev-parse -q --verify refs/stash 2>/dev/null || true)
            if [ -n "$STASH_AFTER" ] && [ "$STASH_AFTER" != "$STASH_BEFORE" ]; then
                PUSHED_STASH=true
            fi
        fi
    fi

    echo "==> Pulling branch: $CURRENT_BRANCH..."
    git pull origin "$CURRENT_BRANCH" || {
        echo "[infynitae-update] Warning: git pull failed (offline or remote mismatch). Continuing build..."
    }

    if [ "$PUSHED_STASH" = true ]; then
        echo "==> Re-applying local changes..."
        if ! git stash pop; then
            echo "[infynitae-update] Error: Conflict while restoring stashed changes." >&2
            echo "[infynitae-update] Stopping before building or installing. Your changes remain in git stash." >&2
            exit 1
        fi
    fi
fi

# 2. Packages added since: the required ones and the optional parts not left out at install
#    (install-choices). Installed onto the current package lists: when a package is gone from
#    them, the system needs an update first (sudo pacman -Syu), which is the user's to start.
if command -v pacman >/dev/null 2>&1; then
    NEW_PKGS=()
    mapfile -t -O "${#NEW_PKGS[@]}" NEW_PKGS < <(missing_pkgs $(cat live/packages.txt))
    for group in $(optional_groups); do
        is_skipped "$group" && continue
        group_applies "$group" || continue
        mapfile -t -O "${#NEW_PKGS[@]}" NEW_PKGS < <(missing_pkgs $(optional_pkgs "$group"))
    done
    pacman -Qi rustup >/dev/null 2>&1 || NEW_PKGS+=(rustup)
    if [ ${#NEW_PKGS[@]} -gt 0 ]; then
        echo "==> Installing new packages: ${NEW_PKGS[*]}"
        pacman_run -S --needed "${NEW_PKGS[@]}" || {
            echo "[infynitae-update] Could not install ${NEW_PKGS[*]}: update the system (sudo pacman -Syu), then run infynitae-update again." >&2
            exit 1
        }
    fi
    rustup toolchain install 2>/dev/null || rustup show >/dev/null
fi

# GPU, joystick and uinput access come from systemd (70-uaccess) and the steam package
# (60-steam-input); Infynitae needs no udev rules of its own.

# 3. Build optimized release binary
echo "==> Compiling latest Infynitae release binary..."
cargo build --release -p infynitae

# 4. Install binaries to ~/.local/bin
mkdir -p "$HOME/.local/bin"
# replace by rename: the running compositor keeps its old file until the next restart
install -m 755 "${CARGO_TARGET_DIR:-target}/release/infynitae" "$HOME/.local/bin/infynitae.new"
mv -f "$HOME/.local/bin/infynitae.new" "$HOME/.local/bin/infynitae"
install -m 755 live/infynitae-reload "$HOME/.local/bin/infynitae-reload"
install -m 755 live/infynitae-restore "$HOME/.local/bin/infynitae-restore"
install -m 755 live/dev.sh "$HOME/.local/bin/infynitae-dev"
install -m 755 live/settings.sh "$HOME/.local/bin/infynitae-settings"
install -m 755 live/setup-drivers.sh "$HOME/.local/bin/infynitae-drivers"
install -m 755 update.sh "$HOME/.local/bin/infynitae-update"
install -m 755 live/infynitae-session "$HOME/.local/bin/infynitae-session.new" && mv -f "$HOME/.local/bin/infynitae-session.new" "$HOME/.local/bin/infynitae-session"
if [ -f "live/xdg-open" ]; then
    install -m 755 live/xdg-open "$HOME/.local/bin/xdg-open" 2>/dev/null || true
fi

# 5. Compile the C helpers (portal, polkit, lock, wallpaper, recorder stream)
sh "$REPO_DIR/live/build-helpers.sh" "$REPO_DIR" "$HOME/.local/bin" \
    || echo "[infynitae-update] Warning: some helpers were not built (see above)"
install -m 755 "$REPO_DIR/live/record/infynitae-recorder" "$HOME/.local/bin/infynitae-recorder.new"
mv -f "$HOME/.local/bin/infynitae-recorder.new" "$HOME/.local/bin/infynitae-recorder"

# 6. Deploy live companion scripts
mkdir -p "$HOME/infynitae/live"
SRC_LIVE="$(cd live 2>/dev/null && pwd -P || echo "")"
DST_LIVE="$(cd "$HOME/infynitae/live" 2>/dev/null && pwd -P || echo "")"
if [ -n "$SRC_LIVE" ] && [ "$SRC_LIVE" != "$DST_LIVE" ]; then
    cp -rf live/* "$HOME/infynitae/live/"
fi

# 6b. The lock screen's password check (PAM), kept current
if ! cmp -s live/lock/infynitae.pam /etc/pam.d/infynitae; then
    sudo install -m 644 live/lock/infynitae.pam /etc/pam.d/infynitae || true
fi

# 7. Display managers: /usr/local/bin/infynitae-session is a launcher for the user's own copy
if ! cmp -s live/infynitae-session-shim /usr/local/bin/infynitae-session && command -v sudo >/dev/null 2>&1; then
    sudo install -D -m 755 live/infynitae-session-shim /usr/local/bin/infynitae-session || true
fi

# 7b. RustDesk, when installed: the rule that lets the session restart its service (install.sh)
if systemctl cat rustdesk.service >/dev/null 2>&1 \
    && ! cmp -s live/remote/50-infynitae-rustdesk.rules /etc/polkit-1/rules.d/50-infynitae-rustdesk.rules \
    && command -v sudo >/dev/null 2>&1; then
    sudo install -D -m 644 live/remote/50-infynitae-rustdesk.rules \
        /etc/polkit-1/rules.d/50-infynitae-rustdesk.rules || true
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
    sudo install -D -m 644 "$HOME/.local/share/wayland-sessions/infynitae.desktop" \
        /usr/share/wayland-sessions/infynitae.desktop || true
    # an older install under a private umask made the folder root-only: display managers read it
    sudo chmod 755 /usr/share/wayland-sessions || true
fi

# 10. Live session: restart the helpers in place (windows stay open)
if pgrep -x infynitae >/dev/null 2>&1; then
    echo "==> Restarting helpers in the running session (windows stay open)..."
    "$HOME/.local/bin/infynitae-reload" settings wallpaper portal polkit || true
    echo "    (the replay recorder keeps running; 'infynitae-reload recorder' restarts it and clears its buffer)"
    "$HOME/.local/bin/infynitae-reload" status | tail -1
fi

echo ""
echo "=========================================================="
echo "          INFYNITAE UPDATE COMPLETE!"
echo "=========================================================="
echo "Infynitae is up to date and ready to use."
echo "To update anytime in the future, simply run: infynitae-update"
echo "=========================================================="
