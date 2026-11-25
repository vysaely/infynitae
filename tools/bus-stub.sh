# tools/bus-stub.sh, sourced by tools/test-sleep-lock and tools/test-automount: live/bus-watch on a
# private bus, with tools/bus-stub as logind and UDisks2. DBUS_SYSTEM_BUS_ADDRESS is exported, so
# nothing a test runs reaches the machine's own system bus.
#   bus_build DIR    builds infynitae-bus-watch and bus-stub into DIR ($BUS_WATCH is the first)
#   bus_start        starts the bus and the stub; bus_stop ends them, bus_restart does both
#   bus_send LINE    the stub sends a signal: "PrepareForSleep true|false" or a UDisks2 line
#   inhibitors       how many logind inhibitors are open
bus_build() {
    BUS_DIR="$1"
    BUS_WATCH="$BUS_DIR/infynitae-bus-watch"
    mods="gio-2.0 gio-unix-2.0"
    cc -O1 -Wall -o "$BUS_WATCH" "$REPO_DIR/live/bus-watch/infynitae-bus-watch.c" \
        $(pkg-config --cflags --libs $mods) || return 1
    cc -O1 -Wall -o "$BUS_DIR/bus-stub" "$REPO_DIR/tools/bus-stub.c" $(pkg-config --cflags --libs $mods) || return 1
    cat > "$BUS_DIR/bus.conf" <<CONF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <listen>unix:path=$BUS_DIR/bus</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF
    export DBUS_SYSTEM_BUS_ADDRESS="unix:path=$BUS_DIR/bus"
}

# waits up to 5 s for a file
bus_wait_file() {
    i=0
    while [ ! -e "$1" ] && [ "$i" -lt 50 ]; do sleep 0.1; i=$((i + 1)); done
    [ -e "$1" ]
}

bus_start() {
    rm -f "$BUS_DIR/bus" "$BUS_DIR/ready" "$BUS_DIR/stub-in"
    dbus-daemon --config-file="$BUS_DIR/bus.conf" --nofork &
    BUS_PID=$!
    bus_wait_file "$BUS_DIR/bus" || { echo "FAIL: the private bus did not start" >&2; return 1; }
    mkfifo "$BUS_DIR/stub-in"
    "$BUS_DIR/bus-stub" "$BUS_DIR" < "$BUS_DIR/stub-in" &
    STUB_PID=$!
    exec 4> "$BUS_DIR/stub-in"
    bus_wait_file "$BUS_DIR/ready" || { echo "FAIL: the bus stub did not start" >&2; return 1; }
}

bus_stop() {
    exec 4>&-
    kill "$STUB_PID" "$BUS_PID" 2>/dev/null
    wait "$STUB_PID" "$BUS_PID" 2>/dev/null
    return 0
}

bus_restart() {
    bus_stop
    bus_start
}

bus_send() {
    printf '%s\n' "$1" >&4
}

inhibitors() {
    cat "$BUS_DIR/inhibitors" 2>/dev/null
}
