#!/bin/sh
# Sourced by the session scripts instead of ~/.config/infynitae/config itself: loads the user's
# settings, then fixes the ones that are also environment variables other programs read.

# the layout env.sh found (the system's), for a config that leaves it empty
_infy_layout="$XKB_DEFAULT_LAYOUT" _infy_options="$XKB_DEFAULT_OPTIONS"
[ -f "$HOME/.config/infynitae/config" ] && . "$HOME/.config/infynitae/config"

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
