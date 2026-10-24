#!/bin/sh
# Sourced by the session scripts instead of ~/.config/infynitae/config itself: loads the user's
# settings, then fixes the ones that are also environment variables other programs read.

[ -f "$HOME/.config/infynitae/config" ] && . "$HOME/.config/infynitae/config"

# BROWSER is also the variable command-line programs (git, gh, Python's webbrowser, sign-in
# flows) run to open a link: "auto" means the desktop's default opener, never a program "auto".
case "${BROWSER:-auto}" in
    auto) export BROWSER="gio open" ;;
esac
