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
