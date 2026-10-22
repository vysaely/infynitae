#!/bin/sh
# Infynitae Live Development & Diagnostic Utility
# Makes it effortless to tweak the desktop live without crashes or lockouts.
set -e
umask 077

ACTION="${1:-help}"
STATE_DIR="$HOME/.local/state/infynitae"
mkdir -p "$STATE_DIR"

case "$ACTION" in
    reload|r)
        echo "==> Reloading Quickshell UI..."
        killall qs 2>/dev/null || true
        echo "==> Quickshell supervisor will respawn immediately."
        ;;
    logs|log|l)
        TARGET="${2:-all}"
        case "$TARGET" in
            shell|qs)
                tail -n 50 -f "$STATE_DIR/quickshell.log"
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
                touch "$STATE_DIR/session.log" "$STATE_DIR/quickshell.log"
                tail -n 30 -f "$STATE_DIR/session.log" "$STATE_DIR/quickshell.log" "$STATE_DIR/crash.log" 2>/dev/null
                ;;
        esac
        ;;
    test|nested|t)
        echo "==> Launching Infynitae in a safe nested window..."
        echo "==> You can test your compositor changes inside this window without touching your live desktop."
        CARGO_BIN="$HOME/infynitae/infynitae-native/target/release/infynitae"
        if [ ! -f "$CARGO_BIN" ]; then
            CARGO_BIN="$HOME/.local/bin/infynitae"
        fi
        exec "$CARGO_BIN" --nested --title "Infynitae Live Sandbox" --width 1280 --height 800 "$@"
        ;;
    build|b)
        echo "==> Compiling Infynitae compositor in release mode..."
        cd "$HOME/infynitae/infynitae-native"
        cargo build --release -p infynitae
        cp -f target/release/infynitae "$HOME/.local/bin/infynitae"
        echo "==> Successfully installed to ~/.local/bin/infynitae"
        ;;
    check|test-all)
        echo "==> Running architecture verification..."
        cd "$HOME/infynitae/infynitae-native"
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
        echo "  reload, r       Reload Quickshell Dynamic Island & Radar HUD (Super+Shift+R)"
        echo "  test, t         Launch a safe nested compositor window to test live changes"
        echo "  logs, l [type]  Stream logs (all, shell, session, crash)"
        echo "  build, b        Rebuild release binary and deploy to ~/.local/bin/infynitae"
        echo "  check           Verify architecture and run slot tests"
        ;;
esac
