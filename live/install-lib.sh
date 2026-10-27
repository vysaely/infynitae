# Shared by install.sh, update.sh and setup-drivers.sh (bash, sourced): installing packages
# unattended and safely, and the optional parts a user chose to leave out.

INFY_STATE="${XDG_STATE_HOME:-$HOME/.local/state}/infynitae"
# the optional parts left out at install (`SKIP="keyring fonts"`); infynitae-update keeps them out
INSTALL_CHOICES="$INFY_STATE/install-choices"
INFY_LIVE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"

# pacman holds a lock while it runs: wait for another package manager (an update in a terminal,
# pamac) instead of failing; a lock nothing holds is left by a crashed pacman, and removing it
# is the user's decision
pacman_wait_lock() {
    local waited=0
    while [ -e /var/lib/pacman/db.lck ]; do
        if ! pgrep -x 'pacman|pamac|pamac-daemon|packagekitd|yay|paru' >/dev/null 2>&1; then
            echo "==> pacman's lock file is left over, and no package manager is running." >&2
            echo "    Remove it (sudo rm /var/lib/pacman/db.lck), then run this again." >&2
            return 1
        fi
        [ "$waited" = 0 ] && echo "==> Waiting for another package manager to finish..."
        sleep 3
        waited=$((waited + 3))
        if [ "$waited" -ge 900 ]; then
            echo "==> Another package manager has been busy for 15 minutes; try again later." >&2
            return 1
        fi
    done
}

# pacman with nobody at the keyboard. --noconfirm takes each question's default answer, and the
# default for "replace a conflicting package?" is no, which stops the install (pulseaudio for
# pipewire-pulse, rust for rustup); --ask 4 answers that one question yes.
pacman_run() {
    pacman_wait_lock || return 1
    sudo pacman "$@" --noconfirm --ask 4
}

# the packages of "$@" that are not installed, one per line
missing_pkgs() {
    local p
    for p in "$@"; do
        pacman -Qi "$p" >/dev/null 2>&1 || printf '%s\n' "$p"
    done
}

# the optional parts: "group package…" lines of packages-optional.txt
optional_groups() {
    grep -v '^[[:space:]]*\(#\|$\)' "$INFY_LIVE/packages-optional.txt" | awk '{print $1}'
}
optional_pkgs() {
    grep -v '^[[:space:]]*\(#\|$\)' "$INFY_LIVE/packages-optional.txt" \
        | awk -v g="$1" '$1 == g { for (i = 2; i <= NF; i++) print $i }'
}
has_accelerometer() {
    local dev f
    for dev in /sys/bus/iio/devices/iio:device*; do
        [ -d "$dev" ] || continue
        for f in "$dev"/in_accel*_raw "$dev"/in_accel_raw; do
            [ -e "$f" ] && return 0
        done
        if [ -f "$dev/name" ]; then
            case "$(cat "$dev/name" 2>/dev/null)" in
                *accel*|*ACCEL*|*accelerometer*|*Accelerometer*) return 0 ;;
            esac
        fi
    done
    return 1
}
# a tablet-mode switch (convertibles): SW_TABLET_MODE, bit 1 of an input device's switches
has_tablet_mode_switch() {
    awk '/^B: SW=/ { split($2, a, "="); n = split(a[2], w, " ");
        if (and(strtonum("0x" w[n]), 2)) found = 1 } END { exit !found }' /proc/bus/input/devices 2>/dev/null
}
# an Intel CPU in a laptop (a battery): thermald manages its heat
intel_laptop() {
    grep -q GenuineIntel /proc/cpuinfo 2>/dev/null && ls -d /sys/class/power_supply/BAT* >/dev/null 2>&1
}
# a file manager other than Thunar is in use: Thunar is not added beside it
other_file_manager() {
    local fm
    for fm in dolphin nautilus nemo pcmanfm pcmanfm-qt caja yazi lf; do
        command -v "$fm" >/dev/null 2>&1 && return 0
    done
    return 1
}
group_applies() {
    case "$1" in
        autorotate) has_accelerometer ;;
        # an on-screen keyboard for tablet mode, on convertibles only
        osk) has_tablet_mode_switch || has_accelerometer ;;
        # Intel's thermal manager, on Intel laptops only
        thermal) intel_laptop ;;
        # Thunar with its previews (tumbler) when it is there or nothing else is
        files) command -v thunar >/dev/null 2>&1 || ! other_file_manager ;;
        *) return 0 ;;
    esac
}
is_skipped() {
    case " ${SKIP:-} " in *" $1 "*) return 0 ;; esac
    return 1
}
load_install_choices() {
    SKIP=""
    # shellcheck disable=SC1090
    [ -f "$INSTALL_CHOICES" ] && . "$INSTALL_CHOICES"
    return 0
}
save_install_choices() {
    mkdir -p "$INFY_STATE"
    printf 'SKIP="%s"\n' "${SKIP:-}" > "$INSTALL_CHOICES"
}

# One password, at the start: what needs root later runs unattended. sudo's permission is kept
# fresh in the background while the calling script runs (a build takes minutes), and no longer.
sudo_once() {
    if ! command -v sudo >/dev/null 2>&1; then
        echo "==> sudo is not installed: install it and allow your user (the wheel group), then run this again." >&2
        return 1
    fi
    if ! sudo -v; then
        echo "==> Your user cannot use sudo: add it to the wheel group and allow wheel in sudoers (visudo)." >&2
        return 1
    fi
    local parent=$$
    ( while kill -0 "$parent" 2>/dev/null; do sudo -n true; sleep 50; done ) >/dev/null 2>&1 &
    SUDO_KEEPER=$!
}

# ---- the install steps install.sh and update.sh share -------------------------------------------
# One list each, so an installed copy never differs from a fresh one. A step that cannot be done
# is said and skipped: the rest of Infynitae still installs.
warn() { echo "==> $*" >&2; }

# The binaries and scripts into ~/.local/bin, from the source at $1. Replaced by rename: a
# running compositor or script keeps its old file until it restarts.
install_programs() {
    local src="$1" bin="$HOME/.local/bin" from to
    mkdir -p "$bin"
    put() {
        install -m 755 "$1" "$2.new" && mv -f "$2.new" "$2" || warn "Could not install $2"
    }
    put "$src/${CARGO_TARGET_DIR:-target}/release/infynitae" "$bin/infynitae"
    while read -r from to; do
        [ -n "$from" ] && put "$src/$from" "$bin/$to"
    done <<'LIST'
live/dev.sh infynitae-dev
live/settings.sh infynitae-settings
live/setup-drivers.sh infynitae-drivers
update.sh infynitae-update
live/infynitae-reload infynitae-reload
live/infynitae-restore infynitae-restore
live/record/infynitae-recorder infynitae-recorder
live/infynitae-session infynitae-session
live/xdg-open xdg-open
live/xdg-terminal-exec xdg-terminal-exec
live/infynitae-open-with infynitae-open-with
live/battery-limit infynitae-battery-limit
live/infynitae-xkill infynitae-xkill
LIST
    # the C helpers (portal, polkit agent, lock screen, wallpaper, recorder stream)
    sh "$src/live/build-helpers.sh" "$src" "$bin" \
        || warn "Some helpers were not built (see above); Infynitae runs without them"
}

# A file as root, only when it changed ($1 source, $2 destination, $3 mode).
install_root_file() {
    cmp -s "$1" "$2" && return 0
    sudo install -D -m "$3" "$1" "$2" || warn "Could not install $2"
}

# What outside the home folder Infynitae needs, for this machine.
install_system_files() {
    local src="$1" tmp
    # the lock screen's password check (PAM)
    install_root_file "$src/live/lock/infynitae.pam" /etc/pam.d/infynitae 644
    # apps out of sight run at a lower priority (BACKGROUND_PRIORITY): a limit that lets them
    # come back to normal (nice 0) and never above it, from the next login
    install_root_file "$src/live/90-infynitae-nice.conf" /etc/security/limits.d/90-infynitae-nice.conf 644
    # display managers start sessions without ~/.local/bin on PATH: a tiny launcher of the
    # user's own copy, and the session entry they list
    install_root_file "$src/live/infynitae-session-shim" /usr/local/bin/infynitae-session 755
    mkdir -p "$HOME/.local/share/wayland-sessions"
    cat > "$HOME/.local/share/wayland-sessions/infynitae.desktop" <<'SESSION'
[Desktop Entry]
Name=Infynitae
Comment=Infinite-canvas Wayland compositor
Exec=infynitae-session
Type=Application
DesktopNames=infynitae;wlroots
SESSION
    install_root_file "$HOME/.local/share/wayland-sessions/infynitae.desktop" \
        /usr/share/wayland-sessions/infynitae.desktop 644
    # an older install under a private umask made the folder root-only: display managers read it
    sudo chmod 755 /usr/share/wayland-sessions 2>/dev/null || true
    # the GlobalShortcuts portal (Discord, OBS push-to-talk)
    mkdir -p "$HOME/.local/share/xdg-desktop-portal/portals"
    cat > "$HOME/.local/share/xdg-desktop-portal/portals/infynitae.portal" <<'PORTAL'
[portal]
DBusName=org.freedesktop.impl.portal.desktop.infynitae
Interfaces=org.freedesktop.impl.portal.GlobalShortcuts;
UseIn=infynitae;wlroots;
PORTAL
    # the battery's charge limit (BATTERY_LIMIT), settable by this user where supported
    if ls /sys/class/power_supply/BAT*/charge_control_end_threshold >/dev/null 2>&1; then
        tmp="$(mktemp)"
        sed "s/@GROUP@/$(id -gn)/" "$src/live/99-infynitae-battery.rules" > "$tmp"
        if ! cmp -s "$tmp" /etc/udev/rules.d/99-infynitae-battery.rules; then
            install_root_file "$tmp" /etc/udev/rules.d/99-infynitae-battery.rules 644
            sudo udevadm trigger --subsystem-match=power_supply 2>/dev/null || true
        fi
        rm -f "$tmp"
    fi
    # thermald (the thermal part, Intel laptops) runs
    if pacman -Qi thermald >/dev/null 2>&1 && ! systemctl is-enabled --quiet thermald.service 2>/dev/null; then
        sudo systemctl enable --now thermald.service || warn "Could not enable thermald.service"
    fi
    # gamemode's CPU settings need the gamemode group
    if getent group gamemode >/dev/null && ! id -nG "$USER" | grep -qw gamemode; then
        sudo usermod -aG gamemode "$USER" || warn "Could not add you to the gamemode group"
    fi
    # RustDesk, when installed: the session restarts its service once the compositor runs, so it
    # sees a Wayland session (live/session.sh); this rule allows that one restart, nothing else
    if systemctl cat rustdesk.service >/dev/null 2>&1; then
        install_root_file "$src/live/remote/50-infynitae-rustdesk.rules" \
            /etc/polkit-1/rules.d/50-infynitae-rustdesk.rules 644
    fi
}

# The config: created when missing, with the parts left out switched off too, so it says what the
# desktop does; yours is never overwritten (the new defaults go to config.template).
install_config() {
    local src="$1" cfg="$HOME/.config/infynitae/config"
    mkdir -p "$HOME/.config/infynitae"
    if [ -f "$cfg" ]; then
        cp -f "$src/config/config" "$cfg.template"
        echo "==> Kept your config (new defaults: ~/.config/infynitae/config.template)"
        return 0
    fi
    cp -f "$src/config/config" "$cfg"
    set_option() { sed -i "s/^$1=\"[^\"]*\"/$1=\"$2\"/" "$cfg"; }
    is_skipped notifications && set_option NOTIFICATIONS none
    is_skipped automount && set_option AUTOMOUNT false
    is_skipped clipboard && { set_option CLIPBOARD_HISTORY false; set_option CLIPBOARD_PERSIST false; }
    is_skipped autorotate && set_option AUTO_ROTATE false
    [ "${REPLAY_BUFFER:-false}" = true ] && set_option ENABLE_REPLAY_BUFFER true
    echo "==> Created ~/.config/infynitae/config"
}
