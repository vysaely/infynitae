#!/bin/sh
# Infynitae Direct Boot & Greeter Setup
# Replaces raw text TTY login prompt with the sleek infynitae-lock Start Screen.
# Zero bloat, instant startup (< 500ms boot to lock screen).

set -e

USER_NAME="${SUDO_USER:-$(whoami)}"
USER_HOME=$(eval echo "~$USER_NAME")

echo "=========================================================="
echo " Infynitae Start Screen / Direct Boot Setup"
echo " User: $USER_NAME"
echo "=========================================================="

# 1. Ensure START_LOCKED="true" in ~/.config/infynitae/config
CONFIG_DIR="$USER_HOME/.config/infynitae"
CONFIG_FILE="$CONFIG_DIR/config"
mkdir -p "$CONFIG_DIR"

if [ -f "$CONFIG_FILE" ]; then
    if grep -q "START_LOCKED=" "$CONFIG_FILE"; then
        sed -i 's/START_LOCKED=.*/START_LOCKED="true"/' "$CONFIG_FILE"
    else
        echo 'START_LOCKED="true"' >> "$CONFIG_FILE"
    fi
else
    # a new config is the shipped template, started locked
    sed 's/^START_LOCKED=.*/START_LOCKED="true"/' "$USER_HOME/infynitae/config/config" > "$CONFIG_FILE"
    chown -R "$USER_NAME:$USER_NAME" "$CONFIG_DIR"
fi

echo "  -> Configured START_LOCKED=\"true\" in $CONFIG_FILE"

# 2. Add auto-start to ~/.bash_profile or ~/.profile for tty1
PROFILE_TARGET=""
if [ -f "$USER_HOME/.bash_profile" ]; then
    PROFILE_TARGET="$USER_HOME/.bash_profile"
elif [ -f "$USER_HOME/.profile" ]; then
    PROFILE_TARGET="$USER_HOME/.profile"
else
    PROFILE_TARGET="$USER_HOME/.bash_profile"
    touch "$PROFILE_TARGET"
    chown "$USER_NAME:$USER_NAME" "$PROFILE_TARGET"
fi

AUTORUN_MARKER="# Infynitae tty1 autostart"
if ! grep -q "$AUTORUN_MARKER" "$PROFILE_TARGET"; then
    cat <<'EOF' >> "$PROFILE_TARGET"

# Infynitae tty1 autostart
if [ -z "$WAYLAND_DISPLAY" ] && [ "$XDG_VTNR" = 1 ]; then
    if [ -x "$HOME/.local/bin/infynitae-session" ]; then
        exec "$HOME/.local/bin/infynitae-session"
    elif [ -x "$HOME/infynitae/live/launch.sh" ]; then
        exec "$HOME/infynitae/live/launch.sh"
    fi
fi
EOF
    echo "  -> Added tty1 direct launch to $PROFILE_TARGET"
else
    echo "  -> tty1 autostart already present in $PROFILE_TARGET"
fi

# 3. Check for root privileges to install systemd getty override
if [ "$(id -u)" -eq 0 ]; then
    echo "  -> Configuring systemd getty@tty1 autologin..."
    mkdir -p /etc/systemd/system/getty@tty1.service.d
    cat <<EOF > /etc/systemd/system/getty@tty1.service.d/autologin.conf
[Service]
ExecStart=
ExecStart=-/sbin/agetty -o '-p -f -- \\u' --noclear --autologin $USER_NAME %I \$TERM
Type=idle
EOF
    systemctl daemon-reload
    echo "  -> systemd getty@tty1 autologin enabled successfully!"
else
    echo ""
    echo "Notice: To finish enabling boot autologin without password prompt at TTY1,"
    echo "run this script with sudo:"
    echo "  sudo ./live/setup-autologin.sh"
fi

echo ""
echo "Setup complete! On reboot, your system will start straight into the Infynitae Lock Screen."
