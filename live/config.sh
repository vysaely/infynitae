#!/bin/sh
# Sourced by the session scripts instead of ~/.config/infynitae/config itself: loads the user's
# settings, then fixes the ones that are also environment variables other programs read.

[ -f "$HOME/.config/infynitae/config" ] && . "$HOME/.config/infynitae/config"

# BROWSER is also the variable command-line programs (git, gh, Python's webbrowser, sign-in
# flows) run to open a link, as one program name: "auto" means the desktop's default opener
# (xdg-open), never a program "auto", and never a command with a space in it.
case "${BROWSER:-auto}" in
    auto) export BROWSER="xdg-open" ;;
esac
