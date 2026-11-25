#!/usr/bin/env bash
# infynitae-update: pull the source, install new packages, rebuild, reinstall; the config is
# never overwritten and a running session keeps its windows.
set -e
# files it creates are readable by everyone, like any installed file: the session's private
# umask (077) would make /etc/pam.d/infynitae unreadable to the lock screen, and the session
# entry unreadable to a display manager
umask 022

# the source: next to this script, or for the installed copy the checkout install.sh recorded
# (else ~/infynitae)
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd -P)"
if [ ! -f "$REPO_DIR/Cargo.toml" ]; then
    REPO_DIR=; { IFS= read -r REPO_DIR < "$HOME/.config/infynitae/checkout"; } 2>/dev/null || :
    REPO_DIR="${REPO_DIR:-$HOME/infynitae}"
    if [ ! -f "$REPO_DIR/Cargo.toml" ]; then
        echo "==> Infynitae source not found (expected $REPO_DIR)." >&2
        exit 1
    fi
fi

cd "$REPO_DIR"

# the shared, unattended pacman, the parts left out at install, and one password for the run
. "$REPO_DIR/live/install-lib.sh"

MODE="update"
for arg in "$@"; do
    case "$arg" in
        --rollback) MODE="rollback" ;;
        -h|--help)
            echo "Usage: infynitae-update [--rollback]"
            exit 0
            ;;
        *)
            echo "Unknown option: $arg" >&2
            echo "Usage: infynitae-update [--rollback]" >&2
            exit 2
            ;;
    esac
done

ROLLBACK_STATE="$INFY_STATE/update-rollback"

# Recorded PID first (catches infynitae.previous, whose kernel comm is truncated to
# infynitae.previ); fall back to -x infynitae for sessions started before PID tracking.
compositor_running() {
    local pid_file pid comm
    for pid_file in "$INFY_STATE/compositor.pid" "$HOME/.local/state/infynitae/compositor.pid"; do
        [ -r "$pid_file" ] || continue
        IFS= read -r pid < "$pid_file" 2>/dev/null || continue
        case "$pid" in
            ""|*[!0-9]*) continue ;;
        esac
        [ -d "/proc/$pid" ] || continue
        [ "$(stat -c '%u' "/proc/$pid" 2>/dev/null)" = "$(id -u)" ] || continue
        IFS= read -r comm < "/proc/$pid/comm" 2>/dev/null || continue
        case "$comm" in
            infynitae|infynitae.*) return 0 ;;
        esac
    done
    pgrep -u "$(id -u)" -x infynitae >/dev/null 2>&1
}

rollback_infynitae() {
    local pre_head="" post_head="" scripts_changed=false cur_head=""
    local scripts_reverted=false scripts_manual=false k v
    mkdir -p "$INFY_STATE"
    if [ -f "$ROLLBACK_STATE" ]; then
        while IFS='=' read -r k v; do
            case "$k" in
                PRE_HEAD) pre_head="$v" ;;
                POST_HEAD) post_head="$v" ;;
                SCRIPTS_CHANGED) scripts_changed="$v" ;;
            esac
        done < "$ROLLBACK_STATE"
    fi

    rollback_binaries "$HOME/.local/bin" || return 1

    if [ "$scripts_changed" = true ] && [ -n "$pre_head" ] && [ -n "$post_head" ] && [ "$pre_head" != "$post_head" ]; then
        cur_head=$(git rev-parse HEAD 2>/dev/null || true)
        if [ -d ".git" ] && [ "$cur_head" = "$post_head" ] \
            && git diff --quiet HEAD -- 2>/dev/null \
            && git diff --cached --quiet HEAD -- 2>/dev/null; then
            echo "==> Scripts changed; resetting checkout in $REPO_DIR back to ${pre_head:0:12}..."
            git reset --hard "$pre_head" >/dev/null
            install_scripts "$REPO_DIR" "$HOME/.local/bin"
            sync_live_dir "$REPO_DIR"
            printf 'PRE_HEAD=%s\nPOST_HEAD=%s\nSCRIPTS_CHANGED=true\n' "$post_head" "$pre_head" > "$ROLLBACK_STATE.tmp" \
                && mv -f "$ROLLBACK_STATE.tmp" "$ROLLBACK_STATE"
            scripts_reverted=true
        else
            scripts_manual=true
            echo "==> Scripts changed (${pre_head:0:12} -> ${post_head:0:12}), but $REPO_DIR moved or has local changes." >&2
            echo "    To roll scripts back manually (discarding uncommitted changes, if safe):" >&2
            echo "    git -C \"$REPO_DIR\" reset --hard \"$pre_head\"" >&2
        fi
    fi

    if [ "$ROLLBACK_SWAPPED" -eq 0 ] && [ "$scripts_reverted" = false ]; then
        if [ "$scripts_manual" = true ]; then
            return 1
        fi
        echo "==> Nothing to roll back (no .previous binaries or recorded script update)." >&2
        return 1
    fi

    if compositor_running; then
        echo "==> Restarting the helpers (windows stay open)..."
        "$HOME/.local/bin/infynitae-reload" settings wallpaper edge portal polkit || true
    fi

    if [ "$ROLLBACK_COMPOSITOR_SWAPPED" = true ]; then
        echo "==> Rolled back Infynitae ($ROLLBACK_SWAPPED binaries swapped). Log out (Ctrl+Alt+Backspace) or reboot to run the previous compositor."
    else
        echo "==> Rolled back Infynitae ($ROLLBACK_SWAPPED binaries swapped)."
    fi
}

if [ "$MODE" = "rollback" ]; then
    rollback_infynitae
    exit $?
fi

load_install_choices
echo "==> Your password, once (new packages, system files)"
sudo_once
trap 'kill "${SUDO_KEEPER:-}" 2>/dev/null || true' EXIT

# everything is also written to the install log
mkdir -p "$INFY_STATE"
LOG="$INFY_STATE/install.log"
exec > >(tee -a "$LOG") 2>&1
echo "==> $(date '+%F %T') Updating Infynitae in $REPO_DIR"

PRE_HEAD=""
POST_HEAD=""
UPDATE_SCRIPTS_CHANGED=false

# 1. the source; local changes are stashed and put back
if [ -d ".git" ]; then
    PRE_HEAD=$(git rev-parse HEAD 2>/dev/null || true)
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
    # only forward: a source with commits of its own is built as it is, never merged. With
    # UPDATE_SIGNING_KEY in the config, the new commit must be signed by that key (docs/SECURITY.md);
    # an unsigned or otherwise signed one is not taken, and the source stays as it is
    if git fetch origin "$CURRENT_BRANCH"; then
        SIGNER=$( . "$HOME/.config/infynitae/config" 2>/dev/null; printf '%s' "${UPDATE_SIGNING_KEY:-}")
        if [ -n "$SIGNER" ] && [ "$(git rev-parse FETCH_HEAD)" != "$(git rev-parse HEAD)" ] \
            && ! signed_by "$SIGNER" FETCH_HEAD; then
            echo "==> The update is not signed by your UPDATE_SIGNING_KEY: not taken; building the source as it is" >&2
        else
            git merge --ff-only FETCH_HEAD || \
                echo "==> Could not move forward (local commits); building the source as it is"
        fi
    else
        echo "==> Could not pull (offline); building the source as it is"
    fi

    if [ "$PUSHED_STASH" = true ]; then
        echo "==> Restoring local changes..."
        if ! git stash pop; then
            echo "==> Your local changes conflict with the update; nothing was built or installed." >&2
            echo "    They are kept in git stash." >&2
            exit 1
        fi
    fi
    POST_HEAD=$(git rev-parse HEAD 2>/dev/null || true)
    if [ -n "$PRE_HEAD" ] && [ -n "$POST_HEAD" ] && [ "$PRE_HEAD" != "$POST_HEAD" ]; then
        if ! git diff --quiet "$PRE_HEAD" "$POST_HEAD" -- \
            update.sh install.sh live/ ':(exclude)live/*/*.rs' ':(exclude)live/*/*.c' \
            ':(exclude)live/*/src/' ':(exclude)live/*/Cargo.*' 2>/dev/null; then
            UPDATE_SCRIPTS_CHANGED=true
        fi
    fi
fi

# 2. Packages added since: the required ones, the optional parts not left out at install and the
#    hardware parts chosen then (install-choices). Installed onto the current package lists:
#    when a package is gone from them, the system needs an update first (sudo pacman -Syu),
#    which is the user's to start. Graphics drivers: infynitae-drivers.
if command -v pacman >/dev/null 2>&1; then
    mapfile -t NEW_PKGS < <(wanted_pkgs)
    if [ ${#NEW_PKGS[@]} -gt 0 ]; then
        echo "==> Installing: ${NEW_PKGS[*]}"
        pacman_run -S --needed "${NEW_PKGS[@]}" || {
            echo "==> Could not install ${NEW_PKGS[*]}: run sudo pacman -Syu, then infynitae-update again." >&2
            exit 1
        }
    fi
    enable_hw_services
    rustup toolchain install || rustup show >/dev/null || { echo "==> The Rust toolchain could not be installed (see the error above; a folder from a run with sudo: sudo chown -R $USER: ~/.rustup ~/.cargo)" >&2; exit 1; }
fi

# GPU, joystick and uinput access come from systemd (70-uaccess) and the steam package
# (60-steam-input); Infynitae needs no udev rules of its own.

# 3. what Infynitae needs outside the home folder (files of the source, none built): done with
#    root now, so that the build runs with no sudo permission left (a build script of a
#    dependency can never use it)
install_system_files "$REPO_DIR"
end_sudo

# 4. build
# a build folder linked to another drive that is not mounted: build here instead
if [ -L target ] && [ ! -d target/ ]; then
    echo "==> The build folder's drive is not mounted ($(readlink target)): building in ./target"
    rm target && mkdir target
fi
JOBS=$(build_jobs)
echo "==> Building ($JOBS jobs)..."
cargo build --release -p infynitae -j "$JOBS"

# 5. the programs, into ~/.local/bin (a running compositor keeps its old file until it restarts;
#    the compositor's file is replaced only when it changed: COMPOSITOR_CHANGED)
export UPDATE_SCRIPTS_CHANGED
install_programs "$REPO_DIR"

# 6. the session scripts run from this source from now on (shortcuts, reload, the edge's
#    commands); a running compositor keeps the folder it started with for launch.sh itself
record_checkout "$REPO_DIR"

if [ -n "$PRE_HEAD" ] && [ -n "$POST_HEAD" ] && [ "$PRE_HEAD" != "$POST_HEAD" ]; then
    printf 'PRE_HEAD=%s\nPOST_HEAD=%s\nSCRIPTS_CHANGED=%s\n' \
        "$PRE_HEAD" "$POST_HEAD" "$UPDATE_SCRIPTS_CHANGED" > "$ROLLBACK_STATE.tmp" \
        && mv -f "$ROLLBACK_STATE.tmp" "$ROLLBACK_STATE"
elif [ "${BINARIES_CHANGED:-false}" = true ]; then
    printf 'PRE_HEAD=%s\nPOST_HEAD=%s\nSCRIPTS_CHANGED=false\n' \
        "${PRE_HEAD:-}" "${POST_HEAD:-}" > "$ROLLBACK_STATE.tmp" \
        && mv -f "$ROLLBACK_STATE.tmp" "$ROLLBACK_STATE"
fi

# 7. the config (yours is never overwritten)
install_config "$REPO_DIR"
install_default_apps
want_foreign_apps && install_foreign_apps "$REPO_DIR"

# 8. a running session: restart the helpers in place (windows stay open)
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/infynitae"
STATUS_FILE="$STATE_DIR/update-status"
if [ -f "$STATUS_FILE" ]; then
    sys_val=$(sed -n 's/^system=//p' "$STATUS_FILE" | head -n1)
    cu_val=$(sed -n 's/^checkupdates=//p' "$STATUS_FILE" | head -n1)
    now_ts=$(date +%s 2>/dev/null || printf '0')
    {
        printf 'system=%s\n' "$sys_val"
        printf 'infynitae=0\n'
        printf 'checkupdates=%s\n' "${cu_val:-present}"
        printf 'checked_at=%s\n' "$now_ts"
    } > "$STATUS_FILE.tmp.$$" && mv -f "$STATUS_FILE.tmp.$$" "$STATUS_FILE"
    case "$sys_val" in
        ''|0) rm -f "$STATE_DIR/update-notified" ;;
        *) printf '%s:0\n' "$sys_val" > "$STATE_DIR/update-notified.tmp.$$" \
            && mv -f "$STATE_DIR/update-notified.tmp.$$" "$STATE_DIR/update-notified" ;;
    esac
fi

if compositor_running; then
    echo "==> Restarting the helpers (windows stay open)..."
    "$HOME/.local/bin/infynitae-reload" settings wallpaper edge portal polkit || true
    echo "    The replay recorder keeps running; infynitae-reload recorder restarts it (clears its buffer)."
fi

echo ""
if [ "$COMPOSITOR_CHANGED" = false ]; then
    echo "==> Infynitae is up to date. The compositor did not change: nothing needs a restart."
else
    echo "==> Infynitae is up to date. The new compositor runs from the next login or reboot."
fi
