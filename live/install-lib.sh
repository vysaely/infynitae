# Shared by install.sh, update.sh and setup-drivers.sh (bash, sourced): installing packages
# unattended and safely, and the optional parts a user chose to leave out.

INFY_STATE="${XDG_STATE_HOME:-$HOME/.local/state}/infynitae"
# the optional parts left out at install (`SKIP="keyring fonts"`); infynitae-update keeps them out
INSTALL_CHOICES="$INFY_STATE/install-choices"
INFY_LIVE="$(cd "$(dirname "${BASH_SOURCE:-$0}")" 2>/dev/null && pwd -P)"
[ -f "$INFY_LIVE/packages.txt" ] || INFY_LIVE="${INFYNITAE_DIR:-$HOME/infynitae}/live"

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
# a touchscreen (the Steam Deck, a convertible): udev marks it ID_INPUT_TOUCHSCREEN
has_touchscreen() {
    grep -qs '^E:ID_INPUT_TOUCHSCREEN=1' /run/udev/data/c13:* 2>/dev/null
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
# a battery (laptop or handheld)
has_battery() {
    ls -d "${INFYNITAE_POWER_SUPPLY:-/sys/class/power_supply}"/BAT* >/dev/null 2>&1
}
# convertibles and handhelds (touchscreen + battery): a power-button tap suspends instead of powering off
wants_power_key_suspend() {
    has_tablet_mode_switch || has_accelerometer || { has_touchscreen && has_battery; }
}
# an Intel CPU in a laptop (a battery): thermald manages its heat
intel_laptop() {
    grep -q GenuineIntel /proc/cpuinfo 2>/dev/null && has_battery
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
        # an on-screen keyboard, on convertibles and touchscreens (the Steam Deck)
        osk) has_tablet_mode_switch || has_accelerometer || has_touchscreen ;;
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
    # an install from before these were saved: nothing added that it may have left out
    SKIP="" BLUETOOTH=false POWER_PROFILES=false TERMINAL_PKG=false FOREIGN_APPS=false DISK_GUARD=true SNAP_PAC=false
    # shellcheck disable=SC1090
    [ -f "$INSTALL_CHOICES" ] && . "$INSTALL_CHOICES"
    return 0
}
save_install_choices() {
    mkdir -p "$INFY_STATE"
    printf 'SKIP="%s"\nBLUETOOTH=%s\nPOWER_PROFILES=%s\nTERMINAL_PKG=%s\nFOREIGN_APPS=%s\nDISK_GUARD=%s\nSNAP_PAC=%s\n' "${SKIP:-}" \
        "${BLUETOOTH:-false}" "${POWER_PROFILES:-false}" "${TERMINAL_PKG:-false}" \
        "${FOREIGN_APPS:-false}" "${DISK_GUARD:-true}" "${SNAP_PAC:-false}" > "$INSTALL_CHOICES"
}

# The hardware parts: chosen, and the hardware is there. Asked again at each update, so an
# adapter added later gets its packages.
want_bluetooth() {
    [ "${BLUETOOTH:-false}" = true ] && [ -n "$(ls /sys/class/bluetooth 2>/dev/null)" ]
}
# tlp, when installed, manages power instead
want_power_profiles() {
    [ "${POWER_PROFILES:-false}" = true ] && ls -d /sys/class/power_supply/BAT* >/dev/null 2>&1 \
        && ! pacman -Qi tlp >/dev/null 2>&1
}
# Windows and Android programs (docs/SECURITY.md); umu-launcher is in [multilib]
want_foreign_apps() {
    [ "${FOREIGN_APPS:-false}" = true ]
}
# `sudo waydroid init` has run: it makes this folder (Waydroid's tools/actions/initializer.py)
waydroid_ready() {
    [ -d /var/lib/waydroid/rootfs ]
}
# a terminal for Super+Q, when there is none
want_terminal() {
    [ "${TERMINAL_PKG:-false}" = true ] && ! command -v kitty >/dev/null 2>&1 \
        && ! command -v alacritty >/dev/null 2>&1 && ! command -v foot >/dev/null 2>&1
}

# The packages Infynitae wants that are not installed, one per line: the required ones, the
# optional parts not left out that fit this machine, the hardware parts. NetworkManager is not
# among them: a machine on iwd or systemd-networkd keeps its own networking.
wanted_pkgs() {
    local group
    missing_pkgs $(cat "$INFY_LIVE/packages.txt")
    for group in $(optional_groups); do
        is_skipped "$group" && continue
        group_applies "$group" || continue
        missing_pkgs $(optional_pkgs "$group")
    done
    want_terminal && echo alacritty
    want_bluetooth && missing_pkgs bluez bluez-utils
    want_power_profiles && missing_pkgs power-profiles-daemon
    if want_foreign_apps; then
        missing_pkgs bubblewrap waydroid
        if grep -q '^\[multilib\]' /etc/pacman.conf 2>/dev/null; then
            missing_pkgs umu-launcher
        else
            warn "umu-launcher is in [multilib], which is off: Windows programs need it (infynitae-drivers --gaming turns it on)"
        fi
    fi
    if [ "${DISK_GUARD:-true}" = true ] && [ "$(findmnt -no FSTYPE / 2>/dev/null)" = btrfs ]; then
        missing_pkgs btrfs-progs
    fi
    if [ "${SNAP_PAC:-false}" = true ] && [ "$(findmnt -no FSTYPE / 2>/dev/null)" = btrfs ]; then
        missing_pkgs snap-pac
    fi
    # the compiler through rustup: the source pins its version (rust-toolchain.toml), which
    # Arch's own rust package ignores; rustup replaces it
    missing_pkgs rustup
}

# The hardware parts' services run.
enable_hw_services() {
    if want_bluetooth && ! systemctl is-enabled --quiet bluetooth.service 2>/dev/null; then
        sudo systemctl enable --now bluetooth.service || warn "Could not enable bluetooth.service"
    fi
    if want_power_profiles && ! systemctl is-enabled --quiet power-profiles-daemon.service 2>/dev/null; then
        sudo systemctl enable --now power-profiles-daemon.service || warn "Could not enable power-profiles-daemon.service"
    fi
}

# Build jobs: linking needs ~1.5 GB per job, so fewer on a machine with little memory.
build_jobs() {
    local mem_gb jobs
    mem_gb=$(awk '/MemTotal/ { print int($2 / 1048576) }' /proc/meminfo)
    jobs=$(nproc)
    [ $((mem_gb * 2 / 3)) -lt "$jobs" ] && jobs=$((mem_gb * 2 / 3))
    [ "$jobs" -ge 1 ] || jobs=1
    echo "$jobs"
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

# The root steps are done: the keeper stops and sudo's permission ends now, not minutes later,
# so nothing that runs afterwards (a build) can use it.
end_sudo() {
    [ -n "${SUDO_KEEPER:-}" ] && kill "$SUDO_KEEPER" 2>/dev/null
    SUDO_KEEPER=""
    sudo -k 2>/dev/null || true
}

# Whether commit $2 carries a good signature by key $1: a GnuPG fingerprint (or its end), or for
# an SSH signature the key as git prints it (SHA256:...; needs gpg.ssh.allowedSignersFile).
signed_by() {
    local want out
    want=$(printf '%s' "$1" | tr -d ' ' | tr '[:lower:]' '[:upper:]')
    out=$(git verify-commit --raw "$2" 2>&1) || return 1
    if printf '%s\n' "$out" | awk -v w="$want" '
        $2 == "VALIDSIG" {
            n = length(w)
            if (n >= 16 && (substr($3, length($3) - n + 1) == w || substr($NF, length($NF) - n + 1) == w)) ok = 1
        }
        END { exit !ok }'; then
        return 0
    fi
    case "$1" in
        SHA256:*) printf '%s\n' "$out" | grep -q 'Good "git" signature' && printf '%s\n' "$out" | grep -qF "$1" ;;
        *) return 1 ;;
    esac
}

# ---- the install steps install.sh and update.sh share -------------------------------------------
# One list each, so an installed copy never differs from a fresh one. A step that cannot be done
# is said and skipped: the rest of Infynitae still installs.
warn() { echo "==> $*" >&2; }

# Compiled binaries installed into ~/.local/bin (each keeps one .previous generation).
infynitae_binary_names() {
    printf '%s\n' \
        infynitae xwayland-satellite \
        infynitae-portal infynitae-polkit infynitae-lock \
        infynitae-wallpaper infynitae-record-stream infynitae-xres-pid \
        infynitae-edge-apps infynitae-shot infynitae-files infynitae-sandbox-socket
}

# Scripts from the source into ~/.local/bin ($1 source, $2 bin dir).
install_scripts() {
    local src="$1" bin="${2:-$HOME/.local/bin}" from to
    mkdir -p "$bin"
    put() {
        install -m 755 "$1" "$2.new" && mv -f "$2.new" "$2" || warn "Could not install $2"
    }
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
live/infynitae-sleep-lock infynitae-sleep-lock
live/infynitae-automount infynitae-automount
live/infynitae-format-drive infynitae-format-drive
live/infynitae-disk-guard infynitae-disk-guard
live/infynitae-update-check infynitae-update-check
live/windows-run infynitae-windows-run
live/android-run infynitae-android-run
live/greeter/infynitae-login-screen infynitae-login-screen
live/greeter/infynitae-greeter infynitae-greeter
live/greeter/infynitae-greeter-screen infynitae-greeter-screen
LIST
}

# Copy live/ scripts to ~/infynitae/live when updating from another checkout.
sync_live_dir() {
    # the scripts run from the checkout itself (recorded once, ~/.config/infynitae/checkout):
    # nothing is copied into ~/infynitae any more
    record_checkout "$1"
}

# Swap each installed binary with its .previous generation in place.
rollback_binaries() {
    local bin="${1:-$HOME/.local/bin}" b swapped=0 compositor_swapped=false satellite_swapped=false
    while IFS= read -r b; do
        if [ -e "$bin/$b" ] && [ -e "$bin/$b.previous" ] && ! cmp -s "$bin/$b" "$bin/$b.previous"; then
            cp -pf "$bin/$b" "$bin/$b.prev.tmp" \
                && mv -f "$bin/$b.previous" "$bin/$b.next.tmp" \
                && mv -f "$bin/$b.prev.tmp" "$bin/$b.previous" \
                && mv -f "$bin/$b.next.tmp" "$bin/$b" \
                || {
                    rm -f "$bin/$b.prev.tmp" "$bin/$b.next.tmp"
                    warn "Could not swap $bin/$b with $bin/$b.previous"
                    return 1
                }
            swapped=$((swapped + 1))
            [ "$b" = "infynitae" ] && compositor_swapped=true
            [ "$b" = "xwayland-satellite" ] && satellite_swapped=true
        fi
    done <<EOF
$(infynitae_binary_names)
EOF
    if [ "$satellite_swapped" = true ]; then
        rm -f "$bin/.xwayland-satellite.stamp"
    fi
    ROLLBACK_SWAPPED="$swapped"
    ROLLBACK_COMPOSITOR_SWAPPED="$compositor_swapped"
    return 0
}

# The binaries and scripts into ~/.local/bin, from the source at $1. Replaced by rename: a
# running compositor or script keeps its old file until it restarts, and each changed binary
# preserves one previous generation at ~/.local/bin/<name>.previous.
install_programs() {
    local src="$1" bin="$HOME/.local/bin" target_dir b sum
    local -A pre_sum=() post_sum=()
    BINARIES_CHANGED=false
    mkdir -p "$bin"
    while IFS= read -r b; do
        [ -e "$bin/$b" ] && pre_sum[$b]=$(cksum < "$bin/$b")
    done <<EOF
$(infynitae_binary_names)
EOF
    put_bin() {
        if [ -e "$2" ]; then
            cp -pf "$2" "$2.previous.tmp" && mv -f "$2.previous.tmp" "$2.previous" \
                || { rm -f "$2.previous.tmp"; warn "Could not back up $2.previous"; }
        fi
        install -m 755 "$1" "$2.new" && mv -f "$2.new" "$2" || warn "Could not install $2"
    }
    case "${CARGO_TARGET_DIR:-target}" in
        /*) target_dir="$CARGO_TARGET_DIR" ;;
        *)  target_dir="$src/${CARGO_TARGET_DIR:-target}" ;;
    esac
    # the compositor only when its content changed: a running one then still runs the installed
    # file, and COMPOSITOR_CHANGED tells infynitae-update whether a new start brings anything
    if cmp -s "$target_dir/release/infynitae" "$bin/infynitae"; then
        COMPOSITOR_CHANGED=false
    else
        COMPOSITOR_CHANGED=true
        put_bin "$target_dir/release/infynitae" "$bin/infynitae"
    fi
    install_scripts "$src" "$bin"
    # xwayland-satellite with our patch; a failed build keeps the copy already installed
    sh "$src/live/build-satellite.sh" "$src" "$bin"
    # the C helpers (portal, bus watcher, polkit agent, lock screen, wallpaper, recorder stream, X11 window process)
    sh "$src/live/build-helpers.sh" "$src" "$bin" \
        || warn "Some helpers were not built (see above); Infynitae runs without them"
    while IFS= read -r b; do
        if [ -e "$bin/$b" ]; then
            sum=$(cksum < "$bin/$b")
            post_sum[$b]="$sum"
            if [ -n "${pre_sum[$b]:-}" ] && [ "$sum" != "${pre_sum[$b]}" ]; then
                BINARIES_CHANGED=true
            fi
        fi
    done <<EOF
$(infynitae_binary_names)
EOF
    # Keep all pre-existing .previous files at the exact state before this update when any binary
    # or script commit moved; leave existing .previous untouched on a no-op update.
    if [ "$BINARIES_CHANGED" = true ] || [ "${UPDATE_SCRIPTS_CHANGED:-false}" = true ]; then
        while IFS= read -r b; do
            if [ -n "${pre_sum[$b]:-}" ] && [ -e "$bin/$b" ] \
                && [ "${post_sum[$b]:-}" = "${pre_sum[$b]}" ]; then
                cp -pf "$bin/$b" "$bin/$b.previous.tmp" \
                    && mv -f "$bin/$b.previous.tmp" "$bin/$b.previous" \
                    || { rm -f "$bin/$b.previous.tmp"; warn "Could not sync $bin/$b.previous"; }
            fi
        done <<EOF
$(infynitae_binary_names)
EOF
    fi
}

# The checkout at $1 is the one Infynitae runs from: the session scripts, the programs in
# ~/.local/bin, the compositor, the edge and the portal read this record (an install from before
# it has none, and they take ~/infynitae). Replaced by rename: a script reading it meanwhile gets
# the old or the new path, never half of one.
record_checkout() {
    mkdir -p "$HOME/.config/infynitae"
    printf '%s\n' "$1" > "$HOME/.config/infynitae/checkout.new" \
        && mv -f "$HOME/.config/infynitae/checkout.new" "$HOME/.config/infynitae/checkout"
}

# A file as root, only when it changed ($1 source, $2 destination, $3 mode).
install_root_file() {
    cmp -s "$1" "$2" && return 0
    sudo install -D -m "$3" "$1" "$2" || warn "Could not install $2"
}

# System speed settings: real-time scheduling rights, ntsync, memory compaction.
install_speed_settings() {
    local src="$1"
    # real-time priority needs the realtime group (realtime-privileges)
    if getent group realtime >/dev/null && ! id -nG "$USER" | grep -qw realtime; then
        if sudo usermod -aG realtime "$USER"; then
            echo "==> Added $USER to the realtime group (applies from the next login)"
        else
            warn "Could not add you to the realtime group"
        fi
    fi
    # ntsync: kernel NT synchronization primitive for Wine and Proton games
    if modinfo ntsync >/dev/null 2>&1; then
        install_root_file "$src/live/ntsync.conf" /etc/modules-load.d/ntsync.conf 644
        sudo modprobe ntsync || true
    fi
    # background memory compaction causes latency spikes and stutter under memory pressure
    install_root_file "$src/live/99-infynitae-memory.conf" /etc/sysctl.d/99-infynitae-memory.conf 644
    sudo sysctl -p /etc/sysctl.d/99-infynitae-memory.conf || true
}

# Btrfs root protection: dynamic/periodic chunk reclaim, weekly light data balance (never
# -musage), monthly scrub, 4 GiB emergency reserve (>= 20 GiB unallocated), and the root helper
# + polkit rule. No-op on non-btrfs roots.
install_btrfs_guard() {
    local src="$1" dry="${2:-false}" fstype reload_sys=false reclaim_needed=false f
    local btrfs_sys dev_out fs_unalloc dev_lines dev_count devid slack unalloc
    fstype=$(findmnt -no FSTYPE / 2>/dev/null || true)
    [ "$fstype" = btrfs ] || return 0

    btrfs_sys="${INFYNITAE_DISK_GUARD_BTRFS:-/sys/fs/btrfs}"
    for f in "$btrfs_sys"/*/allocation/data/dynamic_reclaim "$btrfs_sys"/*/allocation/data/periodic_reclaim; do
        [ -r "$f" ] || continue
        if [ "$(cat "$f" 2>/dev/null)" != 1 ]; then
            reclaim_needed=true
            break
        fi
    done

    if [ "$dry" = true ]; then
        cmp -s "$src/live/disk-guard/btrfs-reclaim.conf" /etc/tmpfiles.d/btrfs-reclaim.conf 2>/dev/null \
            || echo "==> [Dry-run] would install /etc/tmpfiles.d/btrfs-reclaim.conf"
        [ "$reclaim_needed" = true ] && echo "==> [Dry-run] would apply /etc/tmpfiles.d/btrfs-reclaim.conf"
        cmp -s "$src/live/disk-guard/btrfs-balance-light.service" /etc/systemd/system/btrfs-balance-light.service 2>/dev/null \
            || echo "==> [Dry-run] would install /etc/systemd/system/btrfs-balance-light.service"
        cmp -s "$src/live/disk-guard/btrfs-balance-light.timer" /etc/systemd/system/btrfs-balance-light.timer 2>/dev/null \
            || echo "==> [Dry-run] would install /etc/systemd/system/btrfs-balance-light.timer"
        systemctl is-enabled --quiet btrfs-balance-light.timer 2>/dev/null \
            || echo "==> [Dry-run] would enable btrfs-balance-light.timer"
        if systemctl cat btrfs-scrub@.timer >/dev/null 2>&1 \
            && ! systemctl is-enabled --quiet btrfs-scrub@-.timer 2>/dev/null; then
            echo "==> [Dry-run] would enable btrfs-scrub@-.timer"
        fi
        cmp -s "$src/live/infynitae-disk-fix" /usr/local/lib/infynitae-disk-fix 2>/dev/null \
            || echo "==> [Dry-run] would install /usr/local/lib/infynitae-disk-fix"
        cmp -s "$src/live/50-infynitae-disk-fix.rules" /etc/polkit-1/rules.d/50-infynitae-disk-fix.rules 2>/dev/null \
            || echo "==> [Dry-run] would install /etc/polkit-1/rules.d/50-infynitae-disk-fix.rules"
    else
        if ! cmp -s "$src/live/disk-guard/btrfs-reclaim.conf" /etc/tmpfiles.d/btrfs-reclaim.conf 2>/dev/null; then
            install_root_file "$src/live/disk-guard/btrfs-reclaim.conf" /etc/tmpfiles.d/btrfs-reclaim.conf 644
            reclaim_needed=true
        fi
        if [ "$reclaim_needed" = true ]; then
            sudo systemd-tmpfiles --create /etc/tmpfiles.d/btrfs-reclaim.conf 2>/dev/null || true
        fi
        if ! cmp -s "$src/live/disk-guard/btrfs-balance-light.service" /etc/systemd/system/btrfs-balance-light.service 2>/dev/null; then
            install_root_file "$src/live/disk-guard/btrfs-balance-light.service" \
                /etc/systemd/system/btrfs-balance-light.service 644
            reload_sys=true
        fi
        if ! cmp -s "$src/live/disk-guard/btrfs-balance-light.timer" /etc/systemd/system/btrfs-balance-light.timer 2>/dev/null; then
            install_root_file "$src/live/disk-guard/btrfs-balance-light.timer" \
                /etc/systemd/system/btrfs-balance-light.timer 644
            reload_sys=true
        fi
        [ "$reload_sys" = true ] && { sudo systemctl daemon-reload 2>/dev/null || true; }
        if ! systemctl is-enabled --quiet btrfs-balance-light.timer 2>/dev/null; then
            sudo systemctl enable --now btrfs-balance-light.timer \
                || warn "Could not enable btrfs-balance-light.timer"
        fi
        if systemctl cat btrfs-scrub@.timer >/dev/null 2>&1 \
            && ! systemctl is-enabled --quiet btrfs-scrub@-.timer 2>/dev/null; then
            sudo systemctl enable --now btrfs-scrub@-.timer \
                || warn "Could not enable btrfs-scrub@-.timer"
        fi
        install_root_file "$src/live/infynitae-disk-fix" /usr/local/lib/infynitae-disk-fix 755
        install_root_file "$src/live/50-infynitae-disk-fix.rules" \
            /etc/polkit-1/rules.d/50-infynitae-disk-fix.rules 644
    fi

    command -v btrfs >/dev/null 2>&1 || return 0
    if [ "$dry" = true ]; then
        dev_out=$(btrfs device usage -b / 2>/dev/null || true)
    else
        dev_out=$(sudo btrfs device usage -b / 2>/dev/null || true)
    fi
    dev_lines=$(printf '%s\n' "$dev_out" | awk '
        /, ID: [0-9]+$/ { if (id != "") print id, slack + 0, unalloc; id = $NF; slack = 0; unalloc = "NA"; next }
        /^[[:space:]]*Device slack:/ { slack = $NF; next }
        /^[[:space:]]*Unallocated:/ { unalloc = ($NF == "N/A" ? "NA" : $NF); next }
        END { if (id != "") print id, slack + 0, unalloc }')
    [ -n "$dev_lines" ] || return 0
    dev_count=$(printf '%s\n' "$dev_lines" | grep -c .)
    fs_unalloc=""
    while read -r devid slack unalloc; do
        [ -n "$devid" ] || continue
        if [ "$unalloc" = "NA" ] && [ "$dev_count" -eq 1 ]; then
            [ -n "$fs_unalloc" ] || fs_unalloc=$(btrfs filesystem usage -b / 2>/dev/null \
                | awk '/^[[:space:]]*Device unallocated:/ { print $NF; exit }')
            unalloc="${fs_unalloc:-0}"
        fi
        case "$unalloc" in '' | *[!0-9]*) unalloc=0 ;; esac
        if [ "$slack" -ge 4290772992 ] 2>/dev/null; then
            echo "==> Root btrfs 4 GiB reserve is already set (devid $devid)"
        elif [ "$unalloc" -ge 21474836480 ] 2>/dev/null; then
            if [ "$dry" = true ]; then
                echo "==> [Dry-run] would reserve 4 GiB below root btrfs (devid $devid)"
            elif sudo btrfs filesystem resize "${devid}:-4G" /; then
                echo "==> Reserved 4 GiB below root btrfs (devid $devid)"
            else
                warn "Could not reserve 4 GiB on root btrfs (devid $devid)"
            fi
        else
            echo "==> Root btrfs has less than 20 GiB unallocated ($((unalloc / 1073741824)) GiB): skipping 4 GiB reserve"
        fi
    done <<EOF
$dev_lines
EOF
}

# Disk guard: the 60 s user timer everywhere (it also reports apps a running systemd-oomd closed),
# plus btrfs root setup on btrfs roots. No oomd rules are installed: a swap-kill rule on -.slice
# closed a whole desktop session (compositor and all apps) on the owner PC on 2026-11-21. Pass --dry-run as $2 to preview without changing files.
install_disk_guard() {
    local src="$1" dry=false reload_user=false udir
    [ "${2:-}" = "--dry-run" ] && dry=true
    [ "${DISK_GUARD:-true}" = true ] || return 0

    udir="$HOME/.config/systemd/user"
    if [ "$dry" = true ]; then
        cmp -s "$src/live/disk-guard/infynitae-disk-guard.service" "$udir/infynitae-disk-guard.service" 2>/dev/null \
            || echo "==> [Dry-run] would install $udir/infynitae-disk-guard.service"
        cmp -s "$src/live/disk-guard/infynitae-disk-guard.timer" "$udir/infynitae-disk-guard.timer" 2>/dev/null \
            || echo "==> [Dry-run] would install $udir/infynitae-disk-guard.timer"
        systemctl --user is-enabled --quiet infynitae-disk-guard.timer 2>/dev/null \
            || echo "==> [Dry-run] would enable user infynitae-disk-guard.timer"
    else
        mkdir -p "$udir"
        if ! cmp -s "$src/live/disk-guard/infynitae-disk-guard.service" "$udir/infynitae-disk-guard.service" 2>/dev/null; then
            install -m 644 "$src/live/disk-guard/infynitae-disk-guard.service" "$udir/infynitae-disk-guard.service"
            reload_user=true
        fi
        if ! cmp -s "$src/live/disk-guard/infynitae-disk-guard.timer" "$udir/infynitae-disk-guard.timer" 2>/dev/null; then
            install -m 644 "$src/live/disk-guard/infynitae-disk-guard.timer" "$udir/infynitae-disk-guard.timer"
            reload_user=true
        fi
        [ "$reload_user" = true ] && { systemctl --user daemon-reload 2>/dev/null || true; }
        if ! systemctl --user is-enabled --quiet infynitae-disk-guard.timer 2>/dev/null \
            || ! systemctl --user is-active --quiet infynitae-disk-guard.timer 2>/dev/null; then
            systemctl --user enable --now infynitae-disk-guard.timer 2>/dev/null || true
        fi
    fi

    install_btrfs_guard "$src" "$dry"
}

install_update_check() {
    src="$1"
    dry="${2:-}"
    svc="$src/live/update-check/infynitae-update-check.service"
    tmr="$src/live/update-check/infynitae-update-check.timer"
    unit_dir="$HOME/.config/systemd/user"
    [ -f "$svc" ] && [ -f "$tmr" ] || return 0
    if [ "$dry" = "--dry-run" ]; then
        return 0
    fi
    mkdir -p "$unit_dir"
    reload_user=false
    for unit_src in "$svc" "$tmr"; do
        base=$(basename "$unit_src")
        if [ ! -f "$unit_dir/$base" ] || ! cmp -s "$unit_src" "$unit_dir/$base"; then
            cp "$unit_src" "$unit_dir/$base"
            reload_user=true
        fi
    done
    [ "$reload_user" = true ] && { systemctl --user daemon-reload 2>/dev/null || true; }
    if ! systemctl --user is-enabled --quiet infynitae-update-check.timer 2>/dev/null \
        || ! systemctl --user is-active --quiet infynitae-update-check.timer 2>/dev/null; then
        systemctl --user enable --now infynitae-update-check.timer 2>/dev/null || true
    fi
}

# The login screen (infynitae-login-screen on) runs its own copies of the compositor and the lock
# screen: after an update they get the new ones, so the login screen never runs an older compositor
# than the desktop. Nothing when the login screen is off.
refresh_login_screen() {
    grep -q infynitae-greeter /etc/greetd/config.toml 2>/dev/null || return 0
    [ -d /var/lib/infynitae-greeter/.local/bin ] || return 0
    for f in infynitae infynitae-lock infynitae-greeter infynitae-greeter-screen; do
        [ -x "$HOME/.local/bin/$f" ] || continue
        cmp -s "$HOME/.local/bin/$f" "/var/lib/infynitae-greeter/.local/bin/$f" && continue
        sudo install -o root -g root -m 755 "$HOME/.local/bin/$f" "/var/lib/infynitae-greeter/.local/bin/$f" \
            || warn "Could not update the login screen's $f"
    done
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
Interfaces=org.freedesktop.impl.portal.GlobalShortcuts;org.freedesktop.impl.portal.Settings;
UseIn=infynitae;wlroots;
PORTAL
    # "Format drive..." in file managers that read file-manager actions (PCManFM-Qt, PCManFM)
    mkdir -p "$HOME/.local/share/file-manager/actions"
    install -m 644 "$src/live/file-manager/infynitae-format-drive.desktop" \
        "$HOME/.local/share/file-manager/actions/infynitae-format-drive.desktop"
    # the battery's charge limit (BATTERY_LIMIT), settable by this user where supported
    if ls /sys/class/power_supply/BAT*/charge_control_end_threshold >/dev/null 2>&1; then
        tmp="$(mktemp)"
        escaped_grp=$(id -gn | sed 's/[&/\]/\\&/g')
        sed "s/@GROUP@/$escaped_grp/" "$src/live/99-infynitae-battery.rules" > "$tmp"
        if ! cmp -s "$tmp" /etc/udev/rules.d/99-infynitae-battery.rules; then
            install_root_file "$tmp" /etc/udev/rules.d/99-infynitae-battery.rules 644
            sudo udevadm trigger --subsystem-match=power_supply 2>/dev/null || true
        fi
        rm -f "$tmp"
    fi
    # a convertible's or handheld's power button works like a tablet's: a tap sleeps, holding it shuts down
    if wants_power_key_suspend; then
        if ! cmp -s "$src/live/50-infynitae-power-key.conf" /etc/systemd/logind.conf.d/50-infynitae-power-key.conf; then
            install_root_file "$src/live/50-infynitae-power-key.conf" \
                /etc/systemd/logind.conf.d/50-infynitae-power-key.conf 644
            # a reload re-reads the setting and leaves the sessions alone (a restart would end them)
            sudo systemctl reload systemd-logind 2>/dev/null || true
        fi
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
    # drives named by Roman numeral (DRIVE_NAMES, docs/DRIVES.md): the helper that writes their
    # block of /etc/fstab, and the rule that lets the person at the machine run it and mount
    # those lines without a password
    install_root_file "$src/live/infynitae-drive-name" /usr/local/lib/infynitae-drive-name 755
    install_root_file "$src/live/50-infynitae-drive-names.rules" \
        /etc/polkit-1/rules.d/50-infynitae-drive-names.rules 644
    # Clean shutdown/reboot guard (prevents respawning a locked session during system shutdown)
    mkdir -p "$HOME/.config/systemd/user"
    cat > "$HOME/.config/systemd/user/infynitae-shutdown-guard.service" <<'GUARD'
[Unit]
Description=Stop Infynitae session and show status screen on shutdown or reboot
DefaultDependencies=no
Before=shutdown.target exit.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/sh -c 'rm -f "%h/.local/state/infynitae/stop"'
ExecStop=/bin/sh -c '[ -f "%h/.config/infynitae/config" ] && . "%h/.config/infynitae/config" 2>/dev/null || true; mkdir -p "%h/.local/state/infynitae"; "%h/.local/bin/infynitae" restore save 2>/dev/null || true; pkill -f "infynitae restore [w]atch" 2>/dev/null || true; : > "%h/.local/state/infynitae/stop"; case "${SHUTDOWN_SCREEN:-true}" in 0|false|no|off|FALSE|False) ;; *) msg="Shutting down…"; systemctl list-jobs 2>/dev/null | grep -q "reboot.target" && msg="Rebooting…"; if ! pgrep -x infynitae-lock >/dev/null 2>&1 && [ -x "%h/.local/bin/infynitae-lock" ]; then setsid -f "%h/.local/bin/infynitae-lock" --status "$msg" >/dev/null 2>&1 || true; fi ;; esac'

[Install]
WantedBy=default.target
GUARD
    systemctl --user daemon-reload 2>/dev/null || true
    systemctl --user enable --now infynitae-shutdown-guard.service 2>/dev/null || true
    sh "$src/live/setup-autologin.sh" --autostart >/dev/null 2>&1 || true
    install_speed_settings "$src"
    install_disk_guard "$src"
    refresh_login_screen
    install_update_check "$src"
}

# The parts' own apps open their files: folders in Thunar (not a terminal that also says it opens
# them), videos and music in Celluloid, pictures in Ristretto, text in Mousepad. Only for the types
# with no default of yours in ~/.config/mimeapps.list: a choice you made is never changed.
install_default_apps() {
    local list="$HOME/.config/mimeapps.list" d t types chosen add=""
    mkdir -p "$HOME/.config"
    [ -f "$list" ] || : > "$list"
    chosen=$(awk '/^\[/ { s = ($0 == "[Default Applications]") } s && /=/ { sub(/=.*/, ""); print }' "$list")
    for d in thunar.desktop io.github.celluloid_player.Celluloid.desktop \
        org.xfce.ristretto.desktop org.xfce.mousepad.desktop; do
        [ -f "/usr/share/applications/$d" ] || continue
        types=$(sed -n 's/^MimeType=//p' "/usr/share/applications/$d" | tr ';' '\n')
        for t in $types; do
            printf '%s\n' "$chosen" | grep -qxF "$t" && continue
            add="$add$t=$d
"
            chosen="$chosen
$t"
        done
    done
    [ -n "$add" ] || return 0
    if grep -qx '\[Default Applications\]' "$list"; then
        awk -v add="$add" '{ print } $0 == "[Default Applications]" { printf "%s", add }' "$list" \
            > "$list.new" && mv -f "$list.new" "$list"
    else
        printf '\n[Default Applications]\n%s' "$add" >> "$list"
    fi
    echo "==> Default apps for $(printf '%s' "$add" | grep -c .) more file types (~/.config/mimeapps.list)"
}

# Windows and Android programs (FOREIGN_APPS): the app that opens a file type now, as GLib picks
# it (a choice in mimeapps.list, else the first app that says it handles the type); empty when
# none does.
default_app() {
    LC_ALL=C gio mime "$1" 2>/dev/null | sed -n 's/^Default application for .*: //p'
}
# "type<TAB>app" for each type of the handlers in $1/live/applications that another app opens now.
foreign_other_defaults() {
    local f t cur
    for f in "$1"/live/applications/*.desktop; do
        for t in $(sed -n 's/^MimeType=//p' "$f" | tr ';' ' '); do
            cur=$(default_app "$t")
            [ -n "$cur" ] && [ "$cur" != "${f##*/}" ] && printf '%s\t%s\n' "$t" "$cur"
        done
    done
    return 0
}
# The handlers into ~/.local/share/applications. Each opens its types by default where no other
# app does, and everywhere with FOREIGN_DEFAULTS=true (the installer's menu asked). An app that
# opens a type now is written in mimeapps.list as its default first: GLib would otherwise pick
# the handler in the user's own folder over it, a change nobody chose. GLib ignores an entry whose
# program is not on PATH, and the install runs before ~/.local/bin is on it.
install_foreign_apps() {
    local src="$1" apps="$HOME/.local/share/applications" others f id t cur
    local PATH="$HOME/.local/bin:$PATH"
    if ! command -v gio >/dev/null 2>&1; then
        warn "gio is missing (glib2): .exe and .apk files are not linked to Windows and Android programs"
        return 0
    fi
    others=$(foreign_other_defaults "$src")
    mkdir -p "$apps"
    for f in "$src"/live/applications/*.desktop; do
        id="${f##*/}"
        install -m 644 "$f" "$apps/$id" || { warn "Could not install $apps/$id"; continue; }
        for t in $(sed -n 's/^MimeType=//p' "$f" | tr ';' ' '); do
            cur=$(printf '%s\n' "$others" | awk -F '\t' -v t="$t" '$1 == t { print $2 }')
            if [ -n "$cur" ] && [ "${FOREIGN_DEFAULTS:-false}" != true ]; then
                gio mime "$t" "$cur" >/dev/null || warn "Could not keep $cur for $t"
            else
                gio mime "$t" "$id" >/dev/null || warn "Could not make $id open $t"
            fi
        done
    done
    # "Open with" lists the handlers for a type another app keeps: GLib finds them in the folder's
    # mimeinfo.cache
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q "$apps" || warn "Could not update $apps/mimeinfo.cache"
    fi
}

# The config: created when missing, with the parts left out switched off too, so it says what the
# desktop does. Yours is never overwritten: settings added since are appended with their
# defaults and comments (a value of yours is never changed), and the whole new file goes to
# config.template.
install_config() {
    local src="$1" cfg="$HOME/.config/infynitae/config" new
    mkdir -p "$HOME/.config/infynitae"
    if [ -f "$cfg" ]; then
        cp -f "$src/config/config" "$cfg.template"
        # renamed settings keep their values under the new name (else the new name would be added
        # below at its default, over the old one): live/config-renames
        local old_name new_name rest renamed=""
        while read -r old_name new_name rest; do
            case "$old_name$new_name" in '' | '#'* | *[!A-Z0-9_]*) continue ;; esac
            grep -q "^$old_name=" "$cfg" || continue
            [ -n "$renamed" ] || cp -f "$cfg" "$cfg.bak-$(date +%F)-rename"
            if grep -q "^$new_name=" "$cfg"; then
                sed -i "s/^$old_name=/#$old_name=/" "$cfg" # both: the new one is the one read
            else
                sed -i "s/^$old_name=/$new_name=/" "$cfg"
            fi
            renamed="$renamed $old_name"
        done < "$src/live/config-renames"
        [ -z "$renamed" ] || echo "==> Renamed in your config:$renamed"
        # each setting the config does not have, with the comment lines under it
        new=$(awk 'NR == FNR { if (match($0, /^[A-Z_]+=/)) have[substr($0, 1, RLENGTH)] = 1; next }
            match($0, /^[A-Z_]+=/) { take = !(substr($0, 1, RLENGTH) in have); if (take) print; next }
            take && /^[[:space:]]+#/ { print; next }
            { take = 0 }' "$cfg" "$src/config/config")
        if [ -n "$new" ]; then
            cp -f "$cfg" "$cfg.bak-$(date +%F)"
            [ -z "$(tail -c 1 "$cfg")" ] || echo >> "$cfg"
            printf '\n# New settings (%s), at their defaults:\n%s\n' "$(date +%F)" "$new" >> "$cfg"
            echo "==> Added $(printf '%s\n' "$new" | grep -c '^[A-Z_]*=') new settings to your config (before: $cfg.bak-$(date +%F))"
        else
            echo "==> Kept your config (new defaults: ~/.config/infynitae/config.template)"
        fi
        return 0
    fi
    cp -f "$src/config/config" "$cfg"
    set_option() {
        escaped_val=$(printf '%s\n' "$2" | sed 's/[&/\]/\\&/g')
        sed -i "s/^$1=\"[^\"]*\"/$1=\"$escaped_val\"/" "$cfg"
    }
    is_skipped notifications && set_option NOTIFICATIONS none
    is_skipped automount && set_option AUTOMOUNT false
    is_skipped clipboard && { set_option CLIPBOARD_HISTORY false; set_option CLIPBOARD_PERSIST false; }
    is_skipped autorotate && set_option AUTO_ROTATE false
    [ "${REPLAY_BUFFER:-false}" = true ] && set_option ENABLE_REPLAY_BUFFER true
    echo "==> Created ~/.config/infynitae/config"
}

# ---- the time zone ---------------------------------------------------------------------------
# Cloud images and live media start on UTC: the clock is right, the hour shown is not. The zone is
# a city (`timedatectl list-timezones`), so summer time follows by itself; nothing is looked up
# online (that would send the machine's address to someone).

# The system's zone ("UTC" when none is set).
current_timezone() {
    tz=$(timedatectl show -p Timezone --value 2>/dev/null)
    [ -n "$tz" ] || tz=$(readlink /etc/localtime 2>/dev/null | sed 's|.*/zoneinfo/||')
    echo "${tz:-UTC}"
}

# Whether the system still has no zone of its own.
timezone_unset() {
    case "$(current_timezone)" in UTC | Etc/UTC | Etc/Universal | Universal | Zulu) return 0 ;; esac
    return 1
}

# Asks for a zone in two whiptail menus (region, then city) and prints it; nothing on Cancel.
pick_timezone() {
    zones=$(timedatectl list-timezones 2>/dev/null) || return 1
    # shellcheck disable=SC2046
    area=$(whiptail --title "Time zone" --ok-button "Next" --cancel-button "Skip" --noitem \
        --menu "Your region:" "${MENU_H:-24}" "${MENU_W:-78}" 14 \
        $(printf '%s\n' "$zones" | cut -d/ -f1 | sort -u \
            | grep -xE 'Africa|America|Antarctica|Asia|Atlantic|Australia|Europe|Indian|Pacific' \
            | sed 's/$/ x/') \
        3>&1 1>&2 2>&3) || return 1
    # shellcheck disable=SC2046
    city=$(whiptail --title "Time zone: $area" --ok-button "Next" --cancel-button "Skip" --noitem \
        --menu "Your city (or the nearest one in your time zone):" "${MENU_H:-24}" "${MENU_W:-78}" 14 \
        $(printf '%s\n' "$zones" | sed -n "s|^$area/||p" | sed 's/$/ x/') \
        3>&1 1>&2 2>&3) || return 1
    echo "$area/$city"
}

# Sets the zone $1 (Area/City) and keeps the clock synchronised; false for a zone that is none.
set_timezone() {
    timedatectl list-timezones 2>/dev/null | grep -qx "$1" || { warn "Not a time zone: $1 (timedatectl list-timezones)"; return 1; }
    sudo timedatectl set-timezone "$1" || return 1
    timedatectl show -p NTP --value 2>/dev/null | grep -qx yes || sudo timedatectl set-ntp true 2>/dev/null || true
    echo "==> Time zone: $1 ($(date '+%H:%M'))"
}
