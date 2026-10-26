#!/usr/bin/env bash
# Infynitae installer for Arch Linux: one command, your password once, then it runs by itself.
#   curl -fsSL https://raw.githubusercontent.com/vysaely/infynitae/main/install.sh | bash
#   git clone https://github.com/vysaely/infynitae.git ~/infynitae && ~/infynitae/install.sh
# Options (through curl: ... | bash -s -- --no-keyring): ./install.sh --help
set -eE

REPO_URL="https://github.com/vysaely/infynitae.git"
INSTALL_DIR="$HOME/infynitae"

usage() {
    cat <<'USAGE'
Infynitae installer for Arch Linux. It asks for your password once, then runs by itself;
reboot when it is done.

Leave out an optional part (the session starts each one only when it is installed):
  --no-notifications   no notification daemon (mako)
  --no-keyring         no password store for apps (gnome-keyring)
  --no-automount       USB drives are not mounted when plugged in (udiskie)
  --no-clipboard       no clipboard history (Super+V) and clipboard keeping
  --no-fonts           no Noto fonts (other languages, emoji)
  --no-recorder        no screen recording or replay clips (ffmpeg)
  --no-terminal        no terminal added when none is installed (alacritty)
  --no-bluetooth       Bluetooth is not set up, even with an adapter
  --no-power-profiles  no power modes on a laptop (power-profiles-daemon)
  --minimal            all of the above

How it starts:
  --no-autologin       without a display manager: log in on tty1 to start Infynitae, instead of
                       the machine booting to the Infynitae lock screen
  -h, --help           this help
USAGE
}

AUTOLOGIN=true
TERMINAL_PKG=true
BLUETOOTH=true
POWER_PROFILES=true
SKIP=""
for arg in "$@"; do
    case "$arg" in
        --no-notifications | --no-keyring | --no-automount | --no-clipboard | --no-fonts | --no-recorder)
            SKIP="$SKIP ${arg#--no-}" ;;
        --no-terminal) TERMINAL_PKG=false ;;
        --no-bluetooth) BLUETOOTH=false ;;
        --no-power-profiles) POWER_PROFILES=false ;;
        --minimal)
            SKIP="notifications keyring automount clipboard fonts recorder"
            TERMINAL_PKG=false BLUETOOTH=false POWER_PROFILES=false ;;
        --no-autologin) AUTOLOGIN=false ;;
        -h | --help) usage; exit 0 ;;
        *) echo "Unknown option: $arg" >&2; usage >&2; exit 2 ;;
    esac
done
SKIP="$(echo $SKIP)"

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

# ---- the source: ~/infynitae (the session scripts run from there) ----------------------------
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

# from here on everything is also written to the install log
mkdir -p "${XDG_STATE_HOME:-$HOME/.local/state}/infynitae"
LOG="${XDG_STATE_HOME:-$HOME/.local/state}/infynitae/install.log"
exec > >(tee -a "$LOG") 2>&1
echo "==> $(date '+%F %T') Infynitae install${SKIP:+ (leaving out: $SKIP)}"
trap 'echo "==> The install stopped at line $LINENO of install.sh. The log: $LOG" >&2' ERR

# ---- your password, once ------------------------------------------------------------------------
echo "==> Infynitae needs your password once (packages, drivers, the login setup)."
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

echo "=========================================================="
echo "      INFYNITAE - ARCH LINUX INSTALLER"
echo "=========================================================="

# ---- 1. the system up to date (Arch is only consistent whole: installing onto stale packages
#         breaks things), the signing keys first so old keys do not refuse the new packages ----
waitlock() {
    while [ -e /var/lib/pacman/db.lck ]; do
        pgrep -x 'pacman|pamac|pamac-daemon|packagekitd|yay|paru' >/dev/null 2>&1 \
            || fail "pacman's lock file is left over (no package manager is running): sudo rm /var/lib/pacman/db.lck, then run this again."
        echo "==> Waiting for another package manager to finish..."
        sleep 5
    done
}
waitlock
echo "==> Updating the system (signing keys first)..."
sudo pacman -Sy --needed --noconfirm archlinux-keyring
sudo pacman -Su --noconfirm --ask 4

# ---- 2. the source --------------------------------------------------------------------------------
if [ ! -f "$INSTALL_DIR/Cargo.toml" ]; then
    command -v git >/dev/null 2>&1 || sudo pacman -S --needed --noconfirm git
    if [ -e "$INSTALL_DIR" ]; then
        fail "$INSTALL_DIR exists but is not Infynitae: move it away, then run this again."
    fi
    echo "==> Downloading Infynitae..."
    git clone "$REPO_URL" "$INSTALL_DIR"
elif [ -d "$INSTALL_DIR/.git" ] && git -C "$INSTALL_DIR" diff --quiet 2>/dev/null; then
    git -C "$INSTALL_DIR" pull --ff-only 2>/dev/null || echo "==> Could not update the source (offline, or local commits); installing it as it is"
fi
cd "$INSTALL_DIR"
. "$INSTALL_DIR/live/install-lib.sh"
[ -n "${SUDO_KEEPER:-}" ] || sudo_once

# ---- 3. graphics drivers (Vulkan, 32-bit for Steam, Xwayland; NVIDIA's kernel driver) --------
bash "$INSTALL_DIR/live/setup-drivers.sh"

# ---- 4. packages ----------------------------------------------------------------------------------
echo "==> Checking the packages Infynitae needs..."
PKGS=()
mapfile -t -O "${#PKGS[@]}" PKGS < <(missing_pkgs $(cat "$INSTALL_DIR/live/packages.txt"))
for group in $(optional_groups); do
    is_skipped "$group" && continue
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
    echo "==> Installing: ${PKGS[*]}"
    pacman_run -S --needed "${PKGS[@]}"
fi
if [ "$BLUETOOTH" = true ] && ! systemctl is-enabled --quiet bluetooth.service 2>/dev/null; then
    sudo systemctl enable --now bluetooth.service || echo "==> Could not enable bluetooth.service"
fi
if [ "$POWER_PROFILES" = true ] && ! systemctl is-enabled --quiet power-profiles-daemon.service 2>/dev/null; then
    sudo systemctl enable --now power-profiles-daemon.service || echo "==> Could not enable power-profiles-daemon.service"
fi
save_install_choices

# ---- 5. seat access: logind gives the compositor the GPU and the input devices (no input group);
#         seatd only where logind does not run ---------------------------------------------------
if ! systemctl is-active --quiet systemd-logind 2>/dev/null; then
    sudo systemctl enable --now seatd 2>/dev/null || true
    sudo usermod -aG seat "$USER" 2>/dev/null || true
fi
# GPU, joystick and uinput access come from systemd (70-uaccess) and the steam package
# (60-steam-input); Infynitae needs no udev rules of its own.

# ---- 6. build ----------------------------------------------------------------------------------------
echo "==> Getting the pinned Rust compiler..."
rustup toolchain install 2>/dev/null || rustup show >/dev/null
# linking needs ~1.5 GB per job: fewer jobs on a machine with little memory
MEM_GB=$(awk '/MemTotal/ { print int($2 / 1048576) }' /proc/meminfo)
JOBS=$(nproc)
[ $((MEM_GB * 2 / 3)) -lt "$JOBS" ] && JOBS=$((MEM_GB * 2 / 3))
[ "$JOBS" -ge 1 ] || JOBS=1
echo "==> Building Infynitae ($JOBS jobs; this takes a few minutes)..."
cargo build --release -p infynitae -j "$JOBS"

# ---- 7. install ------------------------------------------------------------------------------------
mkdir -p "$HOME/.local/bin"
install -m 755 "${CARGO_TARGET_DIR:-target}/release/infynitae" "$HOME/.local/bin/infynitae"
install -m 755 live/dev.sh "$HOME/.local/bin/infynitae-dev"
install -m 755 live/settings.sh "$HOME/.local/bin/infynitae-settings"
install -m 755 live/setup-drivers.sh "$HOME/.local/bin/infynitae-drivers"
install -m 755 update.sh "$HOME/.local/bin/infynitae-update"
install -m 755 live/infynitae-reload "$HOME/.local/bin/infynitae-reload"
install -m 755 live/infynitae-restore "$HOME/.local/bin/infynitae-restore"
install -m 755 live/record/infynitae-recorder "$HOME/.local/bin/infynitae-recorder"
install -m 755 live/infynitae-session "$HOME/.local/bin/infynitae-session"
install -m 755 live/xdg-open "$HOME/.local/bin/xdg-open"
chmod +x live/*.sh live/*.py 2>/dev/null || true
# the C helpers (portal, polkit agent, lock screen, wallpaper, recorder stream): one recipe
# shared with infynitae-update; a failure is reported, never hidden
sh live/build-helpers.sh "$INSTALL_DIR" "$HOME/.local/bin" \
    || echo "==> Some helpers were not built (see above); Infynitae runs without them"
# the lock screen's password check (PAM): kept current
if ! cmp -s live/lock/infynitae.pam /etc/pam.d/infynitae; then
    sudo install -m 644 live/lock/infynitae.pam /etc/pam.d/infynitae
fi
echo "==> Installed to ~/.local/bin"

# the GlobalShortcuts portal (Discord, OBS push-to-talk)
mkdir -p "$HOME/.local/share/xdg-desktop-portal/portals"
cat <<'PORTAL' > "$HOME/.local/share/xdg-desktop-portal/portals/infynitae.portal"
[portal]
DBusName=org.freedesktop.impl.portal.desktop.infynitae
Interfaces=org.freedesktop.impl.portal.GlobalShortcuts;
UseIn=infynitae;wlroots;
PORTAL

# ---- 8. configuration (yours is never overwritten) -------------------------------------------
mkdir -p "$HOME/.config/infynitae"
if [ ! -f "$HOME/.config/infynitae/config" ]; then
    cp -f config/config "$HOME/.config/infynitae/config"
    # a part left out is switched off too, so the config says what the desktop does
    is_skipped notifications && sed -i 's/^NOTIFICATIONS="[^"]*"/NOTIFICATIONS="none"/' "$HOME/.config/infynitae/config"
    is_skipped automount && sed -i 's/^AUTOMOUNT="[^"]*"/AUTOMOUNT="false"/' "$HOME/.config/infynitae/config"
    if is_skipped clipboard; then
        sed -i 's/^CLIPBOARD_HISTORY="[^"]*"/CLIPBOARD_HISTORY="false"/; s/^CLIPBOARD_PERSIST="[^"]*"/CLIPBOARD_PERSIST="false"/' \
            "$HOME/.config/infynitae/config"
    fi
    echo "==> Created your configuration: ~/.config/infynitae/config"
else
    cp -f config/config "$HOME/.config/infynitae/config.template"
    echo "==> Your configuration is kept (the new template: ~/.config/infynitae/config.template)"
fi

# ---- 9. sessions ------------------------------------------------------------------------------------
# display managers start sessions without ~/.local/bin on PATH: /usr/local/bin gets a tiny
# launcher that runs the user's own copy (copies of the binaries there would go stale)
sudo install -D -m 755 live/infynitae-session-shim /usr/local/bin/infynitae-session
for prof in "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.profile" "$HOME/.zprofile"; do
    if [ -f "$prof" ] && ! grep -q '\.local/bin' "$prof"; then
        echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$prof"
    fi
done
mkdir -p "$HOME/.local/share/wayland-sessions"
cat <<'SESSION' > "$HOME/.local/share/wayland-sessions/infynitae.desktop"
[Desktop Entry]
Name=Infynitae
Comment=Infynitae Infinite Canvas Wayland Compositor
Exec=infynitae-session
Type=Application
DesktopNames=infynitae;wlroots
SESSION
sudo install -D -m 644 "$HOME/.local/share/wayland-sessions/infynitae.desktop" \
    /usr/share/wayland-sessions/infynitae.desktop

# RustDesk, when installed: the session restarts its service once the compositor runs, so it
# sees a Wayland session (live/session.sh); this rule allows that one restart, nothing else
if systemctl cat rustdesk.service >/dev/null 2>&1; then
    sudo install -D -m 644 live/remote/50-infynitae-rustdesk.rules \
        /etc/polkit-1/rules.d/50-infynitae-rustdesk.rules
    echo "==> RustDesk: allowed the session to restart its service"
fi

# ---- 10. how Infynitae starts after a reboot: a display manager's session list; else tty1 logs
#          in by itself to the Infynitae lock screen (--no-autologin, or a lock screen that could
#          not be installed: logging in on tty1 starts Infynitae) ----------------------------------
if systemctl is-enabled --quiet display-manager.service 2>/dev/null; then
    START_HOW="pick \"Infynitae\" at your display manager's login"
elif [ "$AUTOLOGIN" = true ] && sudo sh live/setup-autologin.sh; then
    START_HOW="the machine starts at the Infynitae lock screen"
else
    sh live/setup-autologin.sh --autostart
    START_HOW="log in on tty1: Infynitae starts"
fi

echo ""
echo "=========================================================="
echo "        INFYNITAE INSTALLATION COMPLETE"
echo "=========================================================="
echo "  Reboot, then: $START_HOW"
echo "  Text login instead of the lock screen: sudo ~/infynitae/live/setup-autologin.sh --off"
echo "  Update any time: infynitae-update    The install log: $LOG"
echo ""
echo "  Keybindings:"
echo "    Super + 1..9         Focus Window Slot 1..9"
echo "    Super + Shift + 1..9 Focus Cluster Slot 1..9"
echo "    Super + Space        Recenter Active Window (Shift: keep zoom)"
echo "    Super + Tab          Next Window (Shift: overview)"
echo "    Super + Enter        Fit Window to Screen"
echo "    Super + Shift + Enter Fullscreen"
echo "    Super + Q            Terminal (auto: kitty, alacritty, foot)"
echo "    Super + W            Launcher (auto: fuzzel, rofi, …)"
echo "    Super + E            Files"
echo "    Super + L            Lock Screen"
echo "    Super + V            Clipboard History"
echo "    Super + A            Control Menu (Wi-Fi, Bluetooth, sound, power)"
echo "    Super + Shift + P    Hide Window from Screenshots & Sharing"
echo "    Super + Alt + C      Save Replay Clip"
echo "    Super + Shift + S    Screenshot"
echo "    Super + Shift + R    Region Screenshot"
echo "    Super + Shift + Backspace  Reload Config & Helpers"
echo "    Super + C            Close Window (Shift: force)"
echo "    Super + Middle drag  Pan the canvas"
echo "    Ctrl + Alt + Backspace Restart Compositor (infynitae-session stop: quit)"
echo "=========================================================="
