#!/bin/sh
# Builds Infynitae's C helpers and installs them into a bin directory (install.sh, update.sh).
#   build-helpers.sh <repo> <bin dir>
# Each binary is built to a temporary file and renamed into place, so a running helper keeps its
# old file. A helper whose libraries are missing is reported, never silently skipped.
REPO="$1"
BIN="$2"
LIVE="$REPO/live"
mkdir -p "$BIN"
FAILED=""

# build <name> <pkg-config modules> <sources...> [-- extra link flags]
build() {
    name="$1"
    mods="$2"
    shift 2
    srcs=""
    while [ $# -gt 0 ] && [ "$1" != "--" ]; do
        srcs="$srcs $1"
        shift
    done
    [ "$1" = "--" ] && shift
    if ! pkg-config --exists $mods 2>/dev/null; then
        echo "[build-helpers] $name: missing libraries ($mods); install them and run infynitae-update" >&2
        FAILED="$FAILED $name"
        return
    fi
    # shellcheck disable=SC2086
    if gcc -O2 -Wall $(pkg-config --cflags $mods) -o "$BIN/$name.new" $srcs $(pkg-config --libs $mods) "$@"; then
        chmod 755 "$BIN/$name.new"
        mv -f "$BIN/$name.new" "$BIN/$name"
        echo "==> Built $name"
    else
        rm -f "$BIN/$name.new"
        echo "[build-helpers] $name: build failed (see above)" >&2
        FAILED="$FAILED $name"
    fi
}

build infynitae-portal "gio-2.0 gio-unix-2.0 wayland-client xkbcommon" \
    "$LIVE/portal/infynitae-portal.c" "$LIVE/portal/remote-desktop.c" \
    "$LIVE/portal/wlr-virtual-pointer-protocol.c" "$LIVE/portal/virtual-keyboard-protocol.c" \
    "$LIVE/portal/xdg-output-protocol.c"
build infynitae-polkit "polkit-agent-1 gtk+-3.0" "$LIVE/polkit/infynitae-polkit.c"
build infynitae-lock "wayland-client cairo pangocairo xkbcommon pam" \
    "$LIVE/lock/infynitae-lock.c" "$LIVE/lock/ext-session-lock-v1-protocol.c" -- -lm -lpthread
build infynitae-wallpaper "wayland-client cairo gdk-pixbuf-2.0" \
    "$LIVE/wallpaper/infynitae-wallpaper.c" "$LIVE/wallpaper/wlr-layer-shell-protocol.c" \
    "$LIVE/wallpaper/xdg-shell-protocol.c" -- -lm
build infynitae-island "wayland-client cairo pangocairo xkbcommon" \
    "$LIVE/island/infynitae-island.c" "$LIVE/wallpaper/wlr-layer-shell-protocol.c" \
    "$LIVE/wallpaper/xdg-shell-protocol.c" -- -lm
build infynitae-record-stream "wayland-client" \
    "$LIVE/record/infynitae-record-stream.c" "$LIVE/record/wlr-screencopy-protocol.c" -- -pthread -lrt

if [ -n "$FAILED" ]; then
    echo "[build-helpers] Not built:$FAILED" >&2
    exit 1
fi
