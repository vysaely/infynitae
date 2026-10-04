#!/usr/bin/env bash
# C1 structural verification for the authority spike. Exits non-zero on any violation.
set -u
cd "$(dirname "$0")"
fail=0
echo "== 1. positive tests + compile_fail doctests"
cargo test --offline -q 2>&1 | tail -15 || fail=1
cargo test --offline -q 2>&1 | grep -q "test result: FAILED" && fail=1
echo "== 2. negative build probe: protocol code moving a camera MUST NOT compile"
if cargo build --offline -q -p spike-core --features probe_protocol_moves_camera 2>/tmp/c1probe.err; then
  echo "VIOLATION: probe compiled"; fail=1
else
  echo "OK: probe rejected by the compiler:"; grep -m2 "^error" /tmp/c1probe.err; grep -q "error\[E0624\]" /tmp/c1probe.err || { echo "WRONG REASON"; fail=1; }
fi
echo "== 2b. protocol-side forbidden paths (each must fail with its expected error code)"
for pr in "p1_mutate_through_read_accessor E0624" "p2_forge_input_port E0451" "p3_physical_input_without_port E0061" "p4_call_navigation_internals E0603"; do
  set -- $pr
  if cargo build --offline -q -p spike-probes --features "$1" 2>/tmp/c1p.err; then echo "VIOLATION: $1 compiled"; fail=1
  elif grep -q "error\[$2\]" /tmp/c1p.err; then echo "OK: $1 rejected with $2"
  else echo "WRONG REASON: $1 failed but not with $2:"; grep -m3 "^error" /tmp/c1p.err; fail=1; fi
done
if cargo build --offline -q -p spike-probes --features p5_control_compiles 2>/tmp/c1p.err; then echo "OK: positive control compiles"; else echo "CONTROL BROKEN"; cat /tmp/c1p.err; fail=1; fi
echo "== 3. dependency direction: spike-core has no dependencies"
deps=$(cargo tree --offline -p spike-core -e normal --depth 1 2>/dev/null | tail -n +2)
if [ -n "$deps" ]; then echo "VIOLATION: $deps"; fail=1; else echo "OK: spike-core depends on nothing"; fi
echo "== result: $([ $fail = 0 ] && echo PASS || echo FAIL)"
exit $fail
