#!/usr/bin/env bash
# infynitae-update: pull the source, install new packages, rebuild, reinstall; the config is
# never overwritten and a running session keeps its windows.
set -e
# files it creates are readable by everyone, like any installed file: the session's private
# umask (077) would make /etc/pam.d/infynitae unreadable to the lock screen, and the session
# entry unreadable to a display manager
umask 022

# the source: next to this script, or ~/infynitae for the installed copy
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd -P)"
if [ ! -f "$REPO_DIR/Cargo.toml" ]; then
    if [ -d "$HOME/infynitae" ] && [ -f "$HOME/infynitae/Cargo.toml" ]; then
        REPO_DIR="$HOME/infynitae"
    else
        echo "==> Infynitae source not found (expected ~/infynitae)." >&2
        exit 1
    fi
fi

cd "$REPO_DIR"

# the shared, unattended pacman, the parts left out at install, and one password for the run
. "$REPO_DIR/live/install-lib.sh"
load_install_choices
echo "==> Your password, once (new packages, system files)"
sudo_once
trap 'kill "${SUDO_KEEPER:-}" 2>/dev/null || true' EXIT

echo "==> Updating Infynitae in $REPO_DIR"

# 1. the source; local changes are stashed and put back
if [ -d ".git" ]; then
    CURRENT_BRANCH=$(git branch --show-current 2>/dev/null || echo "main")
    PUSHED_STASH=false
    if ! git diff-index --quiet HEAD -- 2>/dev/null; then
        echo "==> Stashing local changes..."
        STASH_BEFORE=$(git rev-parse -q --verify refs/stash 2>/dev/null || true)
        if git stash push -m "infynitae-auto-update-$(date +%s)" >/dev/null 2>&1; then
            STASH_AFTER=$(git rev-parse -q --verify refs/stash 2>/dev/null || true)
            if [ -n "$STASH_AFTER" ] && [ "$STASH_AFTER" != "$STASH_BEFORE" ]; then
                PUSHED_STASH=true
            fi
        fi
    fi

    echo "==> Pulling $CURRENT_BRANCH..."
    git pull origin "$CURRENT_BRANCH" || {
        echo "==> Could not pull (offline?); building the source as it is"
    }

    if [ "$PUSHED_STASH" = true ]; then
        echo "==> Restoring local changes..."
        if ! git stash pop; then
            echo "==> Your local changes conflict with the update; nothing was built or installed." >&2
            echo "    They are kept in git stash." >&2
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
        echo "==> Installing: ${NEW_PKGS[*]}"
        pacman_run -S --needed "${NEW_PKGS[@]}" || {
            echo "==> Could not install ${NEW_PKGS[*]}: run sudo pacman -Syu, then infynitae-update again." >&2
            exit 1
        }
    fi
    rustup toolchain install || rustup show >/dev/null || { echo "==> The Rust toolchain could not be installed (see the error above; a folder from a run with sudo: sudo chown -R $USER: ~/.rustup ~/.cargo)" >&2; exit 1; }
fi

# GPU, joystick and uinput access come from systemd (70-uaccess) and the steam package
# (60-steam-input); Infynitae needs no udev rules of its own.

# 3. build
# a build folder linked to another drive that is not mounted: build here instead
if [ -L target ] && [ ! -d target/ ]; then
    echo "==> The build folder's drive is not mounted ($(readlink target)): building in ./target"
    rm target && mkdir target
fi
echo "==> Building..."
cargo build --release -p infynitae

# 4. install to ~/.local/bin
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
    install -m 755 live/xdg-terminal-exec "$HOME/.local/bin/xdg-terminal-exec" 2>/dev/null || true
    install -m 755 live/infynitae-open-with "$HOME/.local/bin/infynitae-open-with" 2>/dev/null || true
fi

# 5. the C helpers (portal, polkit, lock, wallpaper, recorder stream)
sh "$REPO_DIR/live/build-helpers.sh" "$REPO_DIR" "$HOME/.local/bin" \
    || echo "==> Some helpers were not built (see above); Infynitae runs without them"
install -m 755 "$REPO_DIR/live/record/infynitae-recorder" "$HOME/.local/bin/infynitae-recorder.new"
mv -f "$HOME/.local/bin/infynitae-recorder.new" "$HOME/.local/bin/infynitae-recorder"

# 6. the session scripts, when this source is not ~/infynitae itself
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
# apps out of sight run at a lower priority (BACKGROUND_PRIORITY): a limit that lets them come
# back to normal (nice 0) and never above it, from the next login
if ! cmp -s live/90-infynitae-nice.conf /etc/security/limits.d/90-infynitae-nice.conf; then
    sudo install -D -m 644 live/90-infynitae-nice.conf /etc/security/limits.d/90-infynitae-nice.conf || true
fi
# gamemode (game mode for fullscreen games): its CPU settings need the gamemode group
if getent group gamemode >/dev/null && ! id -nG "$USER" | grep -qw gamemode; then
    sudo usermod -aG gamemode "$USER" || true
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

# 8. the config: created when missing, otherwise only the new defaults as config.template
mkdir -p "$HOME/.config/infynitae"
if [ -f "config/config" ]; then
    if [ ! -f "$HOME/.config/infynitae/config" ]; then
        cp -f "config/config" "$HOME/.config/infynitae/config"
        echo "==> Created ~/.config/infynitae/config"
    else
        cp -f "config/config" "$HOME/.config/infynitae/config.template"
        echo "==> Kept your config (new defaults: ~/.config/infynitae/config.template)"
    fi
fi

# 9. the session entry for display managers
mkdir -p "$HOME/.local/share/wayland-sessions"
cat << 'EOF' > "$HOME/.local/share/wayland-sessions/infynitae.desktop"
[Desktop Entry]
Name=Infynitae
Comment=Infinite-canvas Wayland compositor
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

# 10. a running session: restart the helpers in place (windows stay open)
if pgrep -x infynitae >/dev/null 2>&1; then
    echo "==> Restarting the helpers (windows stay open)..."
    "$HOME/.local/bin/infynitae-reload" settings wallpaper portal polkit || true
    echo "    The replay recorder keeps running; infynitae-reload recorder restarts it (clears its buffer)."
    "$HOME/.local/bin/infynitae-reload" status | tail -1
fi

echo ""
echo "==> Infynitae is up to date. A new compositor runs from its next start (Ctrl+Alt+Backspace)."
