#!/bin/sh
# Infynitae Wallpaper & Live Background Manager
# Supports both static images (swaybg/hyprpaper) and animated video backgrounds (mpvpaper/mpv)
# Zero overhead: uses hardware-accelerated video decoding and zero-cost idle for static images.

CONFIG="$HOME/.config/infynitae/config"
if [ -f "$CONFIG" ]; then
    . "$CONFIG"
fi

TARGET="$1"
if [ -z "$TARGET" ]; then
    TARGET="$WALLPAPER"
fi

# Terminate existing wallpaper instances first
pkill -x swaybg 2>/dev/null || true
pkill -x mpvpaper 2>/dev/null || true
pkill -f "infynitae-live-wallpaper" 2>/dev/null || true

if [ -z "$TARGET" ]; then
    exit 0
fi

# Expand tilde safely without eval
case "$TARGET" in
    "~"/*) TARGET="$HOME/${TARGET#\~/}" ;;
    "~")   TARGET="$HOME" ;;
esac

if [ ! -f "$TARGET" ]; then
    echo "[infynitae-wallpaper] Error: File not found: $TARGET" >&2
    exit 1
fi

MODE="${WALLPAPER_MODE:-fill}"

# Check extension
EXT="${TARGET##*.}"
EXT=$(echo "$EXT" | tr '[:upper:]' '[:lower:]')

case "$EXT" in
    mp4|webm|mkv|avi|mov|gif)
        # Animated / Live Video Background (Wallpaper Engine style)
        if command -v mpvpaper >/dev/null 2>&1; then
            exec mpvpaper -o "--loop-file=inf --hwdec=auto --no-audio --vo=gpu" '*' "$TARGET"
        elif command -v mpv >/dev/null 2>&1; then
            # Direct mpv Wayland background (if layer-shell output is supported)
            exec mpv --title="infynitae-live-wallpaper" --loop-file=inf --hwdec=auto --no-audio --vo=gpu "$TARGET"
        else
            echo "[infynitae-wallpaper] For animated video wallpapers, install mpvpaper: sudo pacman -S mpv (or yay -S mpvpaper)" >&2
            exit 1
        fi
        ;;
    *)
        # Static Image Background
        if command -v swaybg >/dev/null 2>&1; then
            exec swaybg -i "$TARGET" -m "$MODE"
        elif command -v hyprpaper >/dev/null 2>&1; then
            hyprctl hyprpaper unload all 2>/dev/null || true
            hyprctl hyprpaper preload "$TARGET" 2>/dev/null || true
            exec hyprpaper
        elif command -v wpaperd >/dev/null 2>&1; then
            exec wpaperd
        else
            echo "[infynitae-wallpaper] Install swaybg for static wallpapers: sudo pacman -S swaybg" >&2
            exit 1
        fi
        ;;
esac
