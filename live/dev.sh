#!/bin/sh
# Infynitae Live Development & Diagnostic Utility
# Makes it effortless to tweak the desktop live without crashes or lockouts.
set -e
umask 077

ACTION="${1:-help}"
if [ $# -gt 0 ]; then
    shift
fi
STATE_DIR="$HOME/.local/state/infynitae"
mkdir -p "$STATE_DIR"

if [ -f "$HOME/infynitae/infynitae-native/Cargo.toml" ]; then
    REPO_DIR="$HOME/infynitae/infynitae-native"
else
    REPO_DIR="$HOME/infynitae"
fi

case "$ACTION" in
    reload|r)
        # the helpers (island, wallpaper, portal…) restart in place; windows stay open
        exec "$REPO_DIR/live/infynitae-reload" all
        ;;
    logs|log|l)
        TARGET="${1:-all}"
        case "$TARGET" in
            shell|helpers|h)
                tail -n 50 -f "$STATE_DIR/helpers.log"
                ;;
            session|s)
                tail -n 50 -f "$STATE_DIR/session.log"
                ;;
            crash|c)
                if [ -f "$STATE_DIR/crash.log" ]; then
                    tail -n 50 -f "$STATE_DIR/crash.log"
                else
                    echo "No crashes recorded in $STATE_DIR/crash.log"
                fi
                ;;
            *)
                echo "=== Streaming Infynitae Logs (Ctrl+C to stop) ==="
                touch "$STATE_DIR/session.log" "$STATE_DIR/helpers.log"
                tail -n 30 -f "$STATE_DIR/session.log" "$STATE_DIR/helpers.log" "$STATE_DIR/crash.log" 2>/dev/null
                ;;
        esac
        ;;
    test|nested|t)
        if [ -z "$WAYLAND_DISPLAY" ] && [ -z "$DISPLAY" ]; then
            echo "Error: 'infynitae-dev test' runs a nested window inside an existing desktop (e.g. KDE)." >&2
            echo "On a TTY console, run:  infynitae-session" >&2
            exit 1
        fi
        echo "==> Launching Infynitae in a safe nested window..."
        echo "==> You can test your compositor changes inside this window without touching your live desktop."
        CARGO_BIN="${CARGO_TARGET_DIR:-$REPO_DIR/target}/release/infynitae"
        if [ ! -f "$CARGO_BIN" ]; then
            CARGO_BIN="$HOME/.local/bin/infynitae"
        fi
        exec "$CARGO_BIN" --nested --title "Infynitae Live Sandbox" --width 1280 --height 800 "$@"
        ;;
    build|b)
        echo "==> Compiling Infynitae compositor in release mode..."
        cd "$REPO_DIR"
        cargo build --release -p infynitae
        cp -f "${CARGO_TARGET_DIR:-target}/release/infynitae" "$HOME/.local/bin/infynitae"
        echo "==> Successfully installed to ~/.local/bin/infynitae"
        ;;
    check|test-all)
        echo "==> Running architecture verification..."
        cd "$REPO_DIR"
        python3 tools/verify-architecture
        echo "==> Running core tests..."
        cargo test -p infynitae-backend-headless --test t1_window_slots
        echo "==> All checks passed!"
        ;;
    *)
        echo "Infynitae Developer Utility"
        echo "Usage: infynitae-dev <command>"
        echo ""
        echo "Commands:"
        echo "  reload, r       Restart the helpers in place: island, wallpaper, portal… (windows stay)"
        echo "  test, t         Launch a safe nested compositor window to test live changes"
        echo "  logs, l [type]  Stream logs (all, helpers, session, crash)"
        echo "  build, b        Rebuild release binary and deploy to ~/.local/bin/infynitae"
        echo "  check           Verify architecture and run slot tests"
        ;;
esac
