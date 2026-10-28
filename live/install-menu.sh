# The installer's menus (bash, sourced by install.sh after install-lib.sh): what was found, the
# parts to install, how Infynitae starts, then a summary to confirm. Shown when install.sh runs in
# a terminal with no options; the options (--minimal, --gaming …) make the same choices without
# them. whiptail (libnewt) draws them, on the text console too.

# The size of a menu: the terminal's, at most 24 by 78.
menu_size() {
    local rows cols
    rows=$(tput lines 2>/dev/null || echo 24)
    cols=$(tput cols 2>/dev/null || echo 80)
    MENU_H=$((rows < 24 ? rows : 24))
    MENU_W=$((cols - 2 < 78 ? cols - 2 : 78))
}

# What this machine is, in one line each.
machine_summary() {
    local gpu ram disk kind
    gpu=$(lspci 2>/dev/null | grep -iE 'vga|3d|display' | head -1 | cut -d: -f3- \
        | sed 's/^ //; s/ (rev .*//; s/Advanced Micro Devices, Inc\. \[AMD\/ATI\]/AMD/; s/Intel Corporation/Intel/; s/NVIDIA Corporation/NVIDIA/' \
        | cut -c1-62)
    ram=$(awk '/MemTotal/ { printf "%.1f GB", $2 / 1048576 }' /proc/meminfo)
    disk=$(df -Ph "$HOME" | awk 'NR == 2 { print $4 }')
    if ls -d /sys/class/power_supply/BAT* >/dev/null 2>&1; then kind="Laptop"; else kind="Desktop"; fi
    if has_tablet_mode_switch || has_accelerometer; then kind="$kind, convertible"; fi
    printf 'Graphics:  %s\nMachine:   %s\nMemory:    %s\nFree:      %s in your home folder\n' \
        "${gpu:-unknown}" "$kind" "$ram" "$disk"
}

# A part's line in the list.
part_label() {
    case "$1" in
        notifications) echo "Notifications (mako)" ;;
        keyring) echo "Password store for apps (gnome-keyring)" ;;
        automount) echo "USB drives mount when plugged in" ;;
        clipboard) echo "Clipboard history, Super+V" ;;
        fonts) echo "Fonts for other languages and emoji" ;;
        recorder) echo "Screen recording (ffmpeg)" ;;
        files) echo "File manager (Thunar, with previews)" ;;
        autorotate) echo "Screen rotation (convertible)" ;;
        osk) echo "On-screen keyboard in tablet mode" ;;
        gamemode) echo "Game mode: full CPU speed in games" ;;
        thermal) echo "Intel's thermal manager (thermald)" ;;
        viewers) echo "Video, picture and text apps" ;;
        terminal) echo "A terminal (alacritty)" ;;
        bluetooth) echo "Bluetooth" ;;
        power-profiles) echo "Power modes (power-profiles-daemon)" ;;
        gaming) echo "Gaming: 32-bit drivers for Steam, Wine (~400 MB)" ;;
        replay-buffer) echo "Replay buffer: save the last minutes" ;;
    esac
}

# The menus; they set SKIP, TERMINAL_PKG, BLUETOOTH, POWER_PROFILES, GAMING, REPLAY_BUFFER and
# AUTOLOGIN as the options would. Cancel ends the install before anything changed.
install_menu() {
    local items=() group chosen status startup
    menu_size

    whiptail --title "Infynitae" --yes-button "Next" --no-button "Cancel" --yesno \
"Infynitae, an infinite-canvas desktop, for this machine:

$(machine_summary)

Next: what to install. Nothing is changed before the last screen." "$MENU_H" "$MENU_W" \
        || { echo "==> Cancelled: nothing was changed."; exit 0; }

    # the parts that fit this machine, ticked; the others are not offered
    for group in $(optional_groups); do
        group_applies "$group" || continue
        items+=("$group" "$(part_label "$group")" "ON")
    done
    if ! command -v kitty >/dev/null 2>&1 && ! command -v alacritty >/dev/null 2>&1 \
        && ! command -v foot >/dev/null 2>&1; then
        items+=(terminal "$(part_label terminal)" ON)
    fi
    [ -n "$(ls /sys/class/bluetooth 2>/dev/null)" ] && items+=(bluetooth "$(part_label bluetooth)" ON)
    if ls -d /sys/class/power_supply/BAT* >/dev/null 2>&1 && ! pacman -Qi tlp >/dev/null 2>&1; then
        items+=(power-profiles "$(part_label power-profiles)" ON)
    fi
    if pacman -Qq steam wine wine-staging lutris 2>/dev/null | grep -q .; then status=ON; else status=OFF; fi
    items+=(gaming "$(part_label gaming)" "$status")
    items+=(replay-buffer "$(part_label replay-buffer)" OFF)

    chosen=$(whiptail --title "Parts to install" --separate-output --ok-button "Next" \
        --cancel-button "Cancel" --checklist "Space ticks a part, Enter goes on." \
        "$MENU_H" "$MENU_W" $((MENU_H - 8)) "${items[@]}" 3>&1 1>&2 2>&3) \
        || { echo "==> Cancelled: nothing was changed."; exit 0; }
    has() { printf '%s\n' "$chosen" | grep -qx "$1"; }

    SKIP=""
    for group in $(optional_groups); do
        has "$group" || SKIP="$SKIP $group"
    done
    SKIP="$(echo $SKIP)"
    has terminal || TERMINAL_PKG=false
    has bluetooth || BLUETOOTH=false
    has power-profiles || POWER_PROFILES=false
    has gaming && GAMING=true
    has replay-buffer && REPLAY_BUFFER=true
    # the replay buffer records with the recorder: it stays, and the summary says so
    if [ "$REPLAY_BUFFER" = true ] && is_skipped recorder; then
        SKIP=$(printf '%s\n' $SKIP | grep -vx recorder | tr '\n' ' ')
        SKIP="$(echo $SKIP)"
        chosen="$chosen
recorder"
    fi

    # how it starts: a display manager lists it; without one, tty1 logs in by itself
    if systemctl is-enabled --quiet display-manager.service 2>/dev/null; then
        startup="Your display manager lists Infynitae at its login."
    else
        startup=$(whiptail --title "Starting Infynitae" --ok-button "Next" --cancel-button "Cancel" \
            --menu "After a reboot:" "$MENU_H" "$MENU_W" 2 \
            lock "Start at the Infynitae lock screen (recommended)" \
            console "Log in on the text console, then Infynitae starts" 3>&1 1>&2 2>&3) \
            || { echo "==> Cancelled: nothing was changed."; exit 0; }
        if [ "$startup" = console ]; then
            AUTOLOGIN=false
            startup="Log in on the text console; Infynitae starts."
        else
            startup="It starts at the Infynitae lock screen."
        fi
    fi

    whiptail --title "Ready to install" --yes-button "Install" --no-button "Cancel" --yesno \
"Parts:    $(printf '%s\n' "$chosen" | while read -r p; do [ -n "$p" ] && printf '%s, ' "$p"; done | sed 's/, $//')
Left out: ${SKIP:-nothing}
Startup:  $startup

The system is updated first, then Infynitae is built from source (a few minutes, longer on a slow machine). It then runs by itself; the log is in ~/.local/state/infynitae/install.log." \
        "$MENU_H" "$MENU_W" \
        || { echo "==> Cancelled: nothing was changed."; exit 0; }
}
