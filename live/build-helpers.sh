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

save_and_install_bin() {
    _staged="$1"
    _target="$2"
    chmod 755 "$_staged" || return 1
    if [ -e "$_target" ]; then
        if cmp -s "$_staged" "$_target"; then
            rm -f "$_staged"
            return 0
        fi
        cp -pf "$_target" "$_target.previous.tmp" \
            && mv -f "$_target.previous.tmp" "$_target.previous" \
            || { rm -f "$_target.previous.tmp"; return 1; }
    fi
    mv -f "$_staged" "$_target"
}

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
    if gcc -O2 -Wall $(pkg-config --cflags $mods) -o "$BIN/$name.new" $srcs $(pkg-config --libs $mods) "$@" \
        && save_and_install_bin "$BIN/$name.new" "$BIN/$name"; then
        echo "==> Built $name"
    else
        rm -f "$BIN/$name.new"
        echo "[build-helpers] $name: build failed (see above)" >&2
        FAILED="$FAILED $name"
    fi
}

build infynitae-portal "gio-2.0 gio-unix-2.0 wayland-client xkbcommon libpipewire-0.3 gbm libdrm" \
    "$LIVE/portal/infynitae-portal.c" "$LIVE/portal/remote-desktop.c" "$LIVE/portal/settings.c" \
    "$LIVE/portal/screencast.c" "$LIVE/portal/screenshot.c" \
    "$LIVE/portal/wlr-virtual-pointer-protocol.c" "$LIVE/portal/virtual-keyboard-protocol.c" \
    "$LIVE/portal/xdg-output-protocol.c" "$LIVE/portal/ext-image-copy-capture-v1-protocol.c" \
    "$LIVE/portal/ext-image-capture-source-v1-protocol.c" \
    "$LIVE/portal/ext-foreign-toplevel-list-v1-protocol.c" "$LIVE/portal/linux-dmabuf-v1-protocol.c"
# drives mounting when plugged in and the lock before sleep, on one watcher of the system bus
build infynitae-bus-watch "gio-2.0 gio-unix-2.0" "$LIVE/bus-watch/infynitae-bus-watch.c"
build infynitae-polkit "polkit-agent-1 gtk+-3.0" "$LIVE/polkit/infynitae-polkit.c"
build infynitae-lock "wayland-client cairo pangocairo xkbcommon pam" \
    "$LIVE/lock/infynitae-lock.c" "$LIVE/lock/greetd.c" "$LIVE/lock/ext-session-lock-v1-protocol.c" -- -lm -lpthread
build infynitae-wallpaper "wayland-client cairo gdk-pixbuf-2.0" \
    "$LIVE/wallpaper/infynitae-wallpaper.c" "$LIVE/wallpaper/wlr-layer-shell-protocol.c" \
    "$LIVE/wallpaper/xdg-shell-protocol.c" "$LIVE/wallpaper/viewporter-protocol.c" -- -lm
# the edge was called the island before 2026-10-31: its old binary goes
[ -x "$BIN/infynitae-edge" ] && rm -f "$BIN/infynitae-island"
build_rust() {
    _rust_name="$1"
    _rust_dir="$2"
    if command -v cargo >/dev/null 2>&1; then
        if cargo build --release --locked --quiet --manifest-path "$_rust_dir/Cargo.toml" \
            --target-dir "$_rust_dir/target" \
            && cp -f "$_rust_dir/target/release/$_rust_name" "$BIN/$_rust_name.new" \
            && save_and_install_bin "$BIN/$_rust_name.new" "$BIN/$_rust_name"; then
            echo "==> Built $_rust_name"
        else
            rm -f "$BIN/$_rust_name.new"
            echo "[build-helpers] $_rust_name: build failed (see above)" >&2
            FAILED="$FAILED $_rust_name"
        fi
    else
        echo "[build-helpers] $_rust_name: cargo is missing" >&2
        FAILED="$FAILED $_rust_name"
    fi
}

# the edge (docs/EDGE_APPS.md), and infynitae-edge pointing at it (live/edge-pick)
build_rust infynitae-edge-apps "$LIVE/edge-apps"
sh "$LIVE/edge-pick" "$BIN" || FAILED="$FAILED infynitae-edge"
build infynitae-record-stream "wayland-client" \
    "$LIVE/record/infynitae-record-stream.c" "$LIVE/record/wlr-screencopy-protocol.c" -- -pthread -lrt
build_rust infynitae-shot "$LIVE/shot-rs"
# the process of an X11 window, from the X server (infynitae-xkill)
build infynitae-xres-pid "xcb xcb-res" "$LIVE/xres/infynitae-xres-pid.c"
# the Wayland socket of a sandboxed Windows program (live/windows-run)
build_rust infynitae-sandbox-socket "$LIVE/sandbox-socket"

if [ -n "$FAILED" ]; then
    echo "[build-helpers] Not built:$FAILED" >&2
    exit 1
fi
