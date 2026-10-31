#!/bin/sh
# Sourced by the session scripts instead of ~/.config/infynitae/config itself: loads the user's
# settings, then fixes the ones that are also environment variables other programs read.

# the layout env.sh found (the system's), for a config that leaves it empty
_infy_layout="$XKB_DEFAULT_LAYOUT" _infy_options="$XKB_DEFAULT_OPTIONS"
_infy_config="$HOME/.config/infynitae/config"
if [ -f "$_infy_config" ]; then
    if sh -n "$_infy_config" 2>/dev/null; then
        . "$_infy_config"
    else
        # a line the shell cannot read would end the script loading it (and the session with it):
        # only the settings that read on their own load, the others keep their defaults
        # (live/config-check names them in a notification)
        while IFS= read -r _infy_line || [ -n "$_infy_line" ]; do
            case "$_infy_line" in
                *=*) sh -n -c "$_infy_line" 2>/dev/null && eval "$_infy_line" ;;
            esac
        done < "$_infy_config"
        unset _infy_line
    fi
fi
unset _infy_config
# settings renamed: the old name still works where the new one is not set
# (the island became the edge, 2026-10-31)
: "${ENABLE_EDGE:=${ENABLE_ISLAND:-}}" "${EDGE_POSITION:=${ISLAND_EDGE:-}}" "${EDGE_CONTROL_SIDE:=${ISLAND_CONTROL_SIDE:-}}"
[ -n "$ENABLE_EDGE" ] || unset ENABLE_EDGE
[ -n "$EDGE_POSITION" ] || unset EDGE_POSITION
[ -n "$EDGE_CONTROL_SIDE" ] || unset EDGE_CONTROL_SIDE

# the compositor checked the layout it uses makes a keymap (an empty or misspelled one does
# not) and gives it to what it starts: the apps use that one, never another
if [ -n "$INFYNITAE_XKB_LAYOUT" ]; then
    export XKB_DEFAULT_LAYOUT="$INFYNITAE_XKB_LAYOUT"
    if [ -n "$INFYNITAE_XKB_OPTIONS" ]; then export XKB_DEFAULT_OPTIONS="$INFYNITAE_XKB_OPTIONS"; else unset XKB_DEFAULT_OPTIONS; fi
fi
# XKB_DEFAULT_LAYOUT="" in the config means the system's layout: never an empty variable, which
# xkbcommon takes as no layout, so apps that build their own keymap from it (kitty) type nothing
if [ -z "$XKB_DEFAULT_LAYOUT" ]; then
    export XKB_DEFAULT_LAYOUT="${_infy_layout:-us}"
fi
if [ -z "$XKB_DEFAULT_OPTIONS" ]; then
    if [ -n "$_infy_options" ]; then export XKB_DEFAULT_OPTIONS="$_infy_options"; else unset XKB_DEFAULT_OPTIONS; fi
fi
unset _infy_layout _infy_options

# BROWSER is also the variable command-line programs (git, gh, Python's webbrowser, sign-in
# flows) run to open a link, as one program name: "auto" means the desktop's default opener
# (xdg-open), never a program "auto", and never a command with a space in it.
case "${BROWSER:-auto}" in
    auto) export BROWSER="xdg-open" ;;
esac
