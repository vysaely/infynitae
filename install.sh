#!/usr/bin/env bash
# Infynitae installer for Arch Linux. In a terminal with no options it asks, in menus, what to
# install; with options, or through curl, it runs by itself. Your password once.
#   git clone https://github.com/vysaely/infynitae.git ~/infynitae && ~/infynitae/install.sh
#   curl -fsSL https://raw.githubusercontent.com/vysaely/infynitae/main/install.sh | bash
# Options (through curl: ... | bash -s -- --minimal): ./install.sh --help
set -eE
# files it creates are readable by everyone, like any installed file: the session's private
# umask (077) would make /etc/pam.d/infynitae unreadable to the lock screen, and the session
# entry unreadable to a display manager
umask 022

REPO_URL="https://github.com/vysaely/infynitae.git"
INSTALL_DIR="$HOME/infynitae"

usage() {
    cat <<'USAGE'
Usage: install.sh [options]

Installs Infynitae on Arch Linux. In a terminal with no options, menus ask what to install;
with any option it runs by itself. Asks for your password once; reboot when it is done.

  --defaults           no menus: what fits this machine (as the menus would tick it)

Leave out a part:
  --no-notifications   notifications (mako)
  --no-keyring         password store for apps (gnome-keyring)
  --no-automount       USB drives mounted when plugged in (udiskie)
  --no-clipboard       clipboard history, Super+V (cliphist)
  --no-fonts           fonts for other languages and emoji (Noto)
  --no-recorder        screen recording and replay clips (ffmpeg)
  --no-files           a file manager (thunar) with previews, archives and phones, unless one
                       is in use
  --no-terminal        a terminal when none is installed (alacritty)
  --no-bluetooth       Bluetooth, even with an adapter
  --no-power-profiles  power modes on a laptop (power-profiles-daemon)
  --no-autorotate      screen rotation on convertibles (iio-sensor-proxy)
  --no-osk             an on-screen keyboard in tablet mode on convertibles (squeekboard)
  --no-gamemode        the CPU at full speed while a fullscreen game runs (gamemode)
  --no-thermal         Intel's thermal manager on Intel laptops (thermald)
  --no-viewers         apps for videos and music (celluloid), pictures (ristretto) and text
                       (mousepad)
  --minimal            all of the above

Turn on:
  --replay-buffer      keep the last minutes of the screen, saved with Super+Alt+C
  --gaming             32-bit graphics drivers for Steam, Wine and 32-bit games (~400 MB);
                       later: infynitae-drivers --gaming. Automatic when Steam or Wine is installed

Startup:
  --no-autologin       log in on tty1 instead of booting to the lock screen
                       (only without a display manager)

  -h, --help           show this help
USAGE
}

AUTOLOGIN=true
REPLAY_BUFFER=false
GAMING=false
TERMINAL_PKG=true
BLUETOOTH=true
POWER_PROFILES=true
SKIP=""
# menus: a person at a terminal, who gave no options
MENU=false
# (and a terminal that can draw them: without TERM, whiptail fails as if cancelled)
[ $# -eq 0 ] && [ -t 0 ] && [ -t 1 ] && [ -n "${TERM:-}" ] && [ "$TERM" != dumb ] && MENU=true
for arg in "$@"; do
    case "$arg" in
        --no-notifications | --no-keyring | --no-automount | --no-clipboard | --no-fonts | --no-recorder | --no-autorotate | --no-files | --no-osk | --no-gamemode | --no-thermal | --no-viewers)
            SKIP="$SKIP ${arg#--no-}" ;;
        --no-terminal) TERMINAL_PKG=false ;;
        --no-bluetooth) BLUETOOTH=false ;;
        --no-power-profiles) POWER_PROFILES=false ;;
        --minimal)
            SKIP="notifications keyring automount clipboard fonts recorder autorotate files osk gamemode thermal viewers"
            TERMINAL_PKG=false BLUETOOTH=false POWER_PROFILES=false ;;
        --no-autologin) AUTOLOGIN=false ;;
        --replay-buffer) REPLAY_BUFFER=true ;;
        --gaming) GAMING=true ;;
        --defaults) ;;
        -h | --help) usage; exit 0 ;;
        *) echo "Unknown option: $arg" >&2; usage >&2; exit 2 ;;
    esac
done
SKIP="$(echo $SKIP)"
case " $SKIP " in
    *" recorder "*) [ "$REPLAY_BUFFER" = true ] && { echo "--replay-buffer needs the recorder: drop --no-recorder (or --minimal)" >&2; exit 2; } ;;
esac

# ---- checks before anything is changed -------------------------------------------------------
fail() { echo "==> $*" >&2; exit 1; }
[ "$(id -u)" -ne 0 ] || fail "Run the installer as your own user, not root: it asks for your password itself."
command -v pacman >/dev/null 2>&1 || fail "This installer is for Arch Linux (pacman was not found)."
[ -d /run/systemd/system ] || fail "Infynitae needs systemd (logind runs the seat)."
getent ahosts archlinux.org >/dev/null 2>&1 || getent ahosts github.com >/dev/null 2>&1 \
    || fail "No internet connection (archlinux.org and github.com cannot be resolved)."
# the build needs ~3 GB (the compiler and the build), packages up to ~2 GB more
free_gb() { df -Pk "$1" 2>/dev/null | awk 'NR == 2 { print int($4 / 1048576) }'; }
[ "$(free_gb "$HOME")" -ge 4 ] || fail "Not enough free space in $HOME: 4 GB are needed for the build."
[ "$(free_gb /var/cache/pacman)" -ge 2 ] || fail "Not enough free space for packages: 2 GB are needed in /var."
# what the build and the install write must be this user's: a folder left from a run with sudo
# (a clone, the compiler, the build) fails the build with "Permission denied (os error 13)"
not_mine=""
for p in "$HOME/.rustup" "$HOME/.cargo" "$INSTALL_DIR" "$INSTALL_DIR/target" "$HOME/.local/bin" \
    "$HOME/.local/state/infynitae" "$HOME/.config/infynitae"; do
    [ -e "$p" ] || continue
    if [ -n "$(find -H "$p" -maxdepth 3 ! -user "$(id -u)" -print -quit 2>/dev/null)" ] || [ ! -w "$p" ]; then
        not_mine="$not_mine $p"
    fi
done
[ -z "$not_mine" ] || fail "These belong to another user (probably root, from a run with sudo):$not_mine
    Give them back, then run this again:  sudo chown -R $USER: $not_mine"

# run from a clone elsewhere: ~/infynitae points to it (the session scripts run from there)
if [ -f "Cargo.toml" ] && [ -d "crates/infynitae" ]; then
    HERE="$(pwd -P)"
    if [ ! -e "$INSTALL_DIR" ]; then
        ln -s "$HERE" "$INSTALL_DIR"
        echo "==> $INSTALL_DIR points to $HERE"
    elif [ "$(cd "$INSTALL_DIR" && pwd -P)" != "$HERE" ]; then
        fail "$INSTALL_DIR already exists and is another folder: Infynitae runs from $INSTALL_DIR. Run the installer there, or move that folder away."
    fi
    cd "$INSTALL_DIR"
fi

# ---- 1. your password, once ---------------------------------------------------------------------
echo "==> Your password, once (packages, drivers, login setup)"
if [ -f "$INSTALL_DIR/live/install-lib.sh" ]; then
    . "$INSTALL_DIR/live/install-lib.sh"
    sudo_once
else
    # through curl, before the clone: the clone brings the shared library
    sudo -v || fail "Your user cannot use sudo: add it to the wheel group and allow wheel in sudoers (visudo)."
    ( parent=$$; while kill -0 "$parent" 2>/dev/null; do sudo -n true; sleep 50; done ) >/dev/null 2>&1 &
    SUDO_KEEPER=$!
fi
trap 'kill "${SUDO_KEEPER:-}" 2>/dev/null || true' EXIT

# the system up to date first (Arch is only consistent whole: installing onto stale packages
# breaks things), the signing keys first so old keys do not refuse the new packages
update_system() {
    while [ -e /var/lib/pacman/db.lck ]; do
        pgrep -x 'pacman|pamac|pamac-daemon|packagekitd|yay|paru' >/dev/null 2>&1 \
            || fail "pacman's lock file is left over (no package manager is running): sudo rm /var/lib/pacman/db.lck, then run this again."
        echo "==> Waiting for another package manager to finish..."
        sleep 5
    done
    echo "==> Updating the system..."
    sudo pacman -Sy --needed --noconfirm archlinux-keyring
    sudo pacman -Su --noconfirm --ask 4
    SYSTEM_UPDATED=true
}

# ---- 2. the source --------------------------------------------------------------------------------
if [ ! -f "$INSTALL_DIR/Cargo.toml" ]; then
    [ -e "$INSTALL_DIR" ] && fail "$INSTALL_DIR exists but is not Infynitae: move it away, then run this again."
    if ! command -v git >/dev/null 2>&1; then
        update_system
        sudo pacman -S --needed --noconfirm git
    fi
    echo "==> Downloading Infynitae..."
    git clone -q "$REPO_URL" "$INSTALL_DIR"
elif [ -d "$INSTALL_DIR/.git" ] && git -C "$INSTALL_DIR" diff --quiet 2>/dev/null; then
    git -C "$INSTALL_DIR" pull -q --ff-only 2>/dev/null || echo "==> Could not get the latest source (offline or local commits): installing it as it is"
fi
cd "$INSTALL_DIR"
. "$INSTALL_DIR/live/install-lib.sh"
[ -n "${SUDO_KEEPER:-}" ] || sudo_once

# ---- 3. the menus (a person at a terminal, no options) ------------------------------------------
if [ "$MENU" = true ]; then
    command -v whiptail >/dev/null 2>&1 || sudo pacman -S --needed --noconfirm libnewt >/dev/null 2>&1 || true
    if command -v whiptail >/dev/null 2>&1; then
        . "$INSTALL_DIR/live/install-menu.sh"
        install_menu
        clear
    else
        echo "==> No menus (whiptail could not be installed): installing what fits this machine"
    fi
fi

# from here on everything is also written to the install log
mkdir -p "$INFY_STATE"
LOG="$INFY_STATE/install.log"
exec > >(tee -a "$LOG") 2>&1
echo "==> $(date '+%F %T') Infynitae install${SKIP:+, leaving out: $SKIP}$([ "$GAMING" = true ] && echo ', gaming')"
trap 'echo "==> The install stopped (install.sh line $LINENO). Log: $LOG" >&2' ERR

[ "${SYSTEM_UPDATED:-false}" = true ] || update_system

# ---- 4. graphics drivers (Vulkan, Xwayland, NVIDIA's kernel driver; 32-bit ones for gaming) -----
if [ "$GAMING" = true ]; then
    bash "$INSTALL_DIR/live/setup-drivers.sh" --gaming
else
    bash "$INSTALL_DIR/live/setup-drivers.sh"
fi

# ---- 5. packages ----------------------------------------------------------------------------------
echo "==> Checking packages..."
PKGS=()
mapfile -t -O "${#PKGS[@]}" PKGS < <(missing_pkgs $(cat "$INSTALL_DIR/live/packages.txt"))
for group in $(optional_groups); do
    is_skipped "$group" && continue
    group_applies "$group" || continue
    mapfile -t -O "${#PKGS[@]}" PKGS < <(missing_pkgs $(optional_pkgs "$group"))
done
# a terminal for Super+Q, when there is none
if [ "$TERMINAL_PKG" = true ] && ! command -v kitty >/dev/null 2>&1 \
    && ! command -v alacritty >/dev/null 2>&1 && ! command -v foot >/dev/null 2>&1; then
    PKGS+=(alacritty)
fi
# hardware the control menu (Super+A) can drive, only where it exists. NetworkManager is not
# added: a machine on iwd or systemd-networkd keeps its own networking
if [ "$BLUETOOTH" = true ] && [ -n "$(ls /sys/class/bluetooth 2>/dev/null)" ]; then
    mapfile -t -O "${#PKGS[@]}" PKGS < <(missing_pkgs bluez bluez-utils)
else
    BLUETOOTH=false
fi
if [ "$POWER_PROFILES" = true ] && ls -d /sys/class/power_supply/BAT* >/dev/null 2>&1 \
    && ! pacman -Qi tlp >/dev/null 2>&1; then
    # tlp, when installed, manages power instead
    mapfile -t -O "${#PKGS[@]}" PKGS < <(missing_pkgs power-profiles-daemon)
else
    POWER_PROFILES=false
fi
# the compiler through rustup: the source pins its version (rust-toolchain.toml), which Arch's
# own rust package ignores; rustup replaces it
pacman -Qi rustup >/dev/null 2>&1 || PKGS+=(rustup)
if [ ${#PKGS[@]} -gt 0 ]; then
    echo "==> Installing ${#PKGS[@]} packages: ${PKGS[*]}"
    pacman_run -S --needed "${PKGS[@]}"
fi
if [ "$BLUETOOTH" = true ] && ! systemctl is-enabled --quiet bluetooth.service 2>/dev/null; then
    sudo systemctl enable --now bluetooth.service || warn "Could not enable bluetooth.service"
fi
if [ "$POWER_PROFILES" = true ] && ! systemctl is-enabled --quiet power-profiles-daemon.service 2>/dev/null; then
    sudo systemctl enable --now power-profiles-daemon.service || warn "Could not enable power-profiles-daemon.service"
fi
save_install_choices
# seat access: logind gives the compositor the GPU and the input devices; seatd only where
# logind does not run
if ! systemctl is-active --quiet systemd-logind 2>/dev/null; then
    sudo systemctl enable --now seatd 2>/dev/null || true
    sudo usermod -aG seat "$USER" 2>/dev/null || true
fi

# ---- 6. build ----------------------------------------------------------------------------------------
echo "==> Getting the Rust toolchain..."
rustup toolchain install || rustup show >/dev/null \
    || fail "The Rust toolchain could not be installed (see the error above)."
# linking needs ~1.5 GB per job: fewer jobs on a machine with little memory
MEM_GB=$(awk '/MemTotal/ { print int($2 / 1048576) }' /proc/meminfo)
JOBS=$(nproc)
[ $((MEM_GB * 2 / 3)) -lt "$JOBS" ] && JOBS=$((MEM_GB * 2 / 3))
[ "$JOBS" -ge 1 ] || JOBS=1
# a build folder linked to another drive that is not mounted: build here instead
if [ -L target ] && [ ! -d target/ ]; then
    echo "==> The build folder's drive is not mounted ($(readlink target)): building in ./target"
    rm target && mkdir target
fi
echo "==> Building Infynitae ($JOBS jobs, a few minutes)..."
cargo build --release -p infynitae -j "$JOBS"

# ---- 7. install -------------------------------------------------------------------------------------
echo "==> Installing..."
install_programs "$INSTALL_DIR"
install_system_files "$INSTALL_DIR"
install_config "$INSTALL_DIR"
install_default_apps
# terminals started outside Infynitae find its commands too
for prof in "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.profile" "$HOME/.zprofile"; do
    if [ -f "$prof" ] && ! grep -q '\.local/bin' "$prof"; then
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$prof"
    fi
done

# ---- 8. how Infynitae starts after a reboot: a display manager's session list; else tty1 logs in
#         by itself to the Infynitae lock screen (or, with --no-autologin or no lock screen,
#         logging in on tty1 starts Infynitae) --------------------------------------------------
AUTOLOGIN_ON=false
if systemctl is-enabled --quiet display-manager.service 2>/dev/null; then
    START_HOW="pick Infynitae at your display manager's login"
elif [ "$AUTOLOGIN" = true ] && sudo sh live/setup-autologin.sh; then
    START_HOW="it starts at the Infynitae lock screen"
    AUTOLOGIN_ON=true
else
    sh live/setup-autologin.sh --autostart
    START_HOW="log in on tty1 to start Infynitae"
fi

echo ""
echo "==> Infynitae is installed. Reboot, then $START_HOW."
echo "    Update:      infynitae-update"
echo "    Config:      ~/.config/infynitae/config"
echo "    Install log: $LOG"
if [ "$AUTOLOGIN_ON" = true ]; then
    echo "    Text login instead: sudo ~/infynitae/live/setup-autologin.sh --off"
fi
