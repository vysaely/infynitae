#!/bin/sh
# How Infynitae starts after a reboot, for a machine without a display manager.
#
#   setup-autologin.sh --autostart   (as you) logging in on tty1 starts Infynitae; you still
#                                    type your password at the text login. The installer runs this.
#   sudo setup-autologin.sh          tty1 logs you in by itself and Infynitae starts at its lock
#                                    screen (START_LOCKED): the machine boots to the lock screen,
#                                    also reachable remotely (RustDesk) after a reboot.
#   sudo setup-autologin.sh --off    back to the text login with a password (auto-start stays).
#
# Autologin is safe only because the session starts locked: the compositor itself starts locked
# (--locked) before any app runs, every restart comes back locked, and a lock screen that cannot
# start ends the session instead of leaving it open.

set -e

MODE="${1:-autologin}"
case "$MODE" in
    --autostart) MODE=autostart ;;
    --off) MODE=off ;;
    autologin) ;;
    *)
        echo "usage: setup-autologin.sh [--autostart | --off]" >&2
        exit 2
        ;;
esac

if [ "$MODE" != autostart ] && [ "$(id -u)" -ne 0 ]; then
    echo "Autologin changes a system setting: run it with sudo (sudo $0 ${1:-})." >&2
    echo "Only the tty1 auto-start, without autologin: $0 --autostart" >&2
    exit 1
fi

USER_NAME="${SUDO_USER:-$(id -un)}"
USER_HOME=$(getent passwd "$USER_NAME" | cut -d: -f6)
USER_SHELL=$(getent passwd "$USER_NAME" | cut -d: -f7)
GETTY_CONF=/etc/systemd/system/getty@tty1.service.d/autologin.conf

# autologin only onto a working lock screen: without one the session ends at once (it never
# starts unlocked), tty1 logs in again, and the machine loops
if [ "$MODE" = autologin ] \
    && { [ ! -x "$USER_HOME/.local/bin/infynitae-lock" ] || [ ! -f /etc/pam.d/infynitae ]; }; then
    echo "The lock screen is not installed (~/.local/bin/infynitae-lock, /etc/pam.d/infynitae):" >&2
    echo "autologin not enabled. Run the installer (or infynitae-update) first." >&2
    exit 1
fi

if [ "$MODE" = off ]; then
    rm -f "$GETTY_CONF"
    systemctl daemon-reload
    echo "  -> tty1 asks for your password again (Infynitae still starts after you log in)"
    exit 0
fi

# 1. Logging in on tty1 starts Infynitae: the login shell's profile (zsh reads .zprofile, bash
#    .bash_profile, other shells .profile)
case "$USER_SHELL" in
    */zsh) PROFILE_TARGET="$USER_HOME/.zprofile" ;;
    */bash)
        if [ -f "$USER_HOME/.bash_profile" ] || [ ! -f "$USER_HOME/.profile" ]; then
            PROFILE_TARGET="$USER_HOME/.bash_profile"
        else
            PROFILE_TARGET="$USER_HOME/.profile"
        fi
        ;;
    *) PROFILE_TARGET="$USER_HOME/.profile" ;;
esac
if [ ! -f "$PROFILE_TARGET" ]; then
    # a new .bash_profile hides .bashrc from login shells unless it reads it, as Arch's own does
    case "$PROFILE_TARGET" in
        */.bash_profile) echo '[[ -f ~/.bashrc ]] && . ~/.bashrc' > "$PROFILE_TARGET" ;;
        *) touch "$PROFILE_TARGET" ;;
    esac
    chown "$USER_NAME:" "$PROFILE_TARGET"
fi
AUTORUN_MARKER="# Infynitae tty1 autostart v2"
if ! grep -q "$AUTORUN_MARKER" "$PROFILE_TARGET"; then
    # an older block (it ended the login with the session) is replaced
    sed -i '/^# Infynitae tty1 autostart$/,/^fi$/d' "$PROFILE_TARGET"
    cat <<'EOF' >> "$PROFILE_TARGET"

# Infynitae tty1 autostart v2
if [ -z "$WAYLAND_DISPLAY" ] && [ "$XDG_VTNR" = 1 ] && [ -x "$HOME/.local/bin/infynitae-session" ]; then
    "$HOME/.local/bin/infynitae-session"
    # the session ended (stopped, or its lock screen could not start): a shell on this console
    # only after your password, since with autologin nobody typed one. A wrong one logs out and
    # tty1 starts the session again, locked.
    exec su -l "$(id -un)"
fi
EOF
    chown "$USER_NAME:" "$PROFILE_TARGET"
    echo "  -> Logging in on tty1 starts Infynitae ($PROFILE_TARGET)"
else
    echo "  -> tty1 already starts Infynitae ($PROFILE_TARGET)"
fi

[ "$MODE" = autostart ] && exit 0

# 2. The session starts at the lock screen (START_LOCKED="true" in the config)
CONFIG_DIR="$USER_HOME/.config/infynitae"
CONFIG_FILE="$CONFIG_DIR/config"
if [ -f "$CONFIG_FILE" ]; then
    if grep -q '^START_LOCKED=' "$CONFIG_FILE"; then
        sed -i 's/^START_LOCKED="[^"]*"/START_LOCKED="true"/' "$CONFIG_FILE"
    else
        echo 'START_LOCKED="true"' >> "$CONFIG_FILE"
    fi
else
    mkdir -p "$CONFIG_DIR"
    sed 's/^START_LOCKED="[^"]*"/START_LOCKED="true"/' "$USER_HOME/infynitae/config/config" > "$CONFIG_FILE"
fi
chown -R "$USER_NAME:" "$CONFIG_DIR"
if ! grep -q '^START_LOCKED="true"' "$CONFIG_FILE"; then
    # never autologin into a session that does not start locked
    echo "START_LOCKED could not be set in $CONFIG_FILE: autologin not enabled" >&2
    exit 1
fi
echo "  -> The session starts at the lock screen (START_LOCKED in $CONFIG_FILE)"

# 3. tty1 logs in by itself
mkdir -p "$(dirname "$GETTY_CONF")"
cat <<EOF > "$GETTY_CONF"
[Service]
ExecStart=
ExecStart=-/usr/bin/agetty -o '-p -f -- \\\\u' --noclear --autologin $USER_NAME %I \$TERM
Type=idle
EOF
systemctl daemon-reload
echo "  -> tty1 logs $USER_NAME in by itself"
echo ""
echo "Done: after a reboot the machine starts at the Infynitae lock screen."
echo "Undo: sudo $0 --off"
