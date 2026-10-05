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
# expected rejection: an error code, or (p2) the exact private-field diagnostic rustc emits when the
# struct has several private fields (that diagnostic carries no error code)
for pr in 'p1_mutate_through_read_accessor|error\[E0624\]' 'p2_forge_input_port|cannot construct `InputPort` with struct literal syntax due to private fields' 'p3_physical_input_without_port|error\[E0061\]' 'p4_call_navigation_internals|error\[E0603\]'; do
  feat="${pr%%|*}"; want="${pr#*|}"
  if cargo build --offline -q -p spike-probes --features "$feat" 2>/tmp/c1p.err; then echo "VIOLATION: $feat compiled"; fail=1
  elif grep -q "$want" /tmp/c1p.err; then echo "OK: $feat rejected: $(grep -m1 "$want" /tmp/c1p.err)"
  else echo "WRONG REASON: $feat failed but not with /$want/:"; grep -m3 "^error" /tmp/c1p.err; fail=1; fi
done
if cargo build --offline -q -p spike-probes --features p5_control_compiles 2>/tmp/c1p.err; then echo "OK: positive control compiles"; else echo "CONTROL BROKEN"; cat /tmp/c1p.err; fail=1; fi
echo "== 2c. storing an authority port in protocol state MUST NOT compile (ports are !Send; protocol state is asserted Send)"
if cargo build --offline -q -p spike-wayland --features probe_store_port_in_state 2>/tmp/c1p.err; then echo "VIOLATION: port stored in protocol state compiled"; fail=1
elif grep -q "error\\[E0277\\]" /tmp/c1p.err; then echo "OK: rejected with E0277"; grep -m1 "cannot be sent" /tmp/c1p.err
else echo "WRONG REASON:"; grep -m3 "^error" /tmp/c1p.err; fail=1; fi
echo "== 2d. PD-F2-01: protocol policy re-fitting a camera on a client commit MUST NOT compile (proof cannot be minted)"
if cargo build --offline -q -p spike-core --features probe_protocol_refits_camera 2>/tmp/c1p.err; then echo "VIOLATION: protocol re-fit compiled"; fail=1
elif grep -q "error\[E0624\]" /tmp/c1p.err && grep -q "mint" /tmp/c1p.err; then echo "OK: rejected: $(grep -m1 'error\[E0624\]' /tmp/c1p.err)"
else echo "WRONG REASON:"; grep -m3 "^error" /tmp/c1p.err; fail=1; fi
echo "== 2e. PD-F2-01: Core retaining an authority proof (sticky fit mode) MUST NOT compile (protocol state Send assertion)"
if cargo build --offline -q -p spike-wayland --features probe_sticky_fit 2>/tmp/c1p.err; then echo "VIOLATION: sticky authority compiled"; fail=1
elif grep -q "error\[E0277\]" /tmp/c1p.err; then echo "OK: rejected with E0277"; grep -m1 "cannot be sent" /tmp/c1p.err
else echo "WRONG REASON:"; grep -m3 "^error" /tmp/c1p.err; fail=1; fi
echo "== 2f. PD-F2-18: output lifecycle authority (each must fail with its expected diagnostic)"
for pr in 'p6_output_connected_without_token|error\[E0061\]' 'p7_forge_output_token|cannot construct `OutputLifecycleToken` with struct literal syntax due to private fields' 'p8_descriptor_with_coordinates|error\[E0560\]' 'p9_public_restore_api|error\[E0599\]'; do
  feat="${pr%%|*}"; want="${pr#*|}"
  if cargo build --offline -q -p spike-probes --features "$feat" 2>/tmp/c1p.err; then echo "VIOLATION: $feat compiled"; fail=1
  elif grep -q "$want" /tmp/c1p.err; then echo "OK: $feat rejected: $(grep -m1 "$want" /tmp/c1p.err)"
  else echo "WRONG REASON: $feat failed but not with /$want/:"; grep -m3 "^error" /tmp/c1p.err; fail=1; fi
done
if cargo build --offline -q -p spike-core --features probe_protocol_creates_camera 2>/tmp/c1p.err; then echo "VIOLATION: protocol camera creation compiled"; fail=1
elif grep -q "error\[E0624\]" /tmp/c1p.err && grep -q "mint" /tmp/c1p.err; then echo "OK: probe_protocol_creates_camera rejected: $(grep -m1 'error\[E0624\]' /tmp/c1p.err)"
else echo "WRONG REASON:"; grep -m3 "^error" /tmp/c1p.err; fail=1; fi
if cargo build --offline -q -p spike-wayland --features probe_store_output_token_in_state 2>/tmp/c1p.err; then echo "VIOLATION: output token stored in protocol state compiled"; fail=1
elif grep -q "error\[E0277\]" /tmp/c1p.err; then echo "OK: probe_store_output_token_in_state rejected with E0277"; grep -m1 "cannot be sent" /tmp/c1p.err
else echo "WRONG REASON:"; grep -m3 "^error" /tmp/c1p.err; fail=1; fi
echo "== 2g. PD-F2-18: OutputLifecycleToken is constructed exactly once, inside Core::new; no other function returns it"
token_check() { # $1 = core source file; prints a verdict; returns 0 if the property holds
  local f="$1" n_lit n_ret
  n_lit=$(grep -c "OutputLifecycleToken { core_id" "$f")
  n_ret=$(grep -E "fn [a-z_]+\(" "$f" | grep -E "\) -> [^{]*OutputLifecycleToken" | grep -vc "pub fn new(persisted")
  echo "   construction sites=$n_lit (want 1) | other functions returning the token=$n_ret (want 0)"
  [ "$n_lit" = 1 ] && [ "$n_ret" = 0 ] && awk '/pub fn new\(persisted/{f=1} f&&/OutputLifecycleToken \{ core_id/{ok=1} f&&/^    }$/{exit} END{exit !ok}' "$f"
}
if token_check core/src/lib.rs; then echo "OK: single construction site inside Core::new"; else echo "VIOLATION"; fail=1; fi
seed=$(mktemp); cp core/src/lib.rs "$seed"
printf '\nimpl Core {\n    pub fn reissue_token(&self) -> OutputLifecycleToken {\n        OutputLifecycleToken { core_id: self.id, _not_send: std::marker::PhantomData }\n    }\n}\n' >> "$seed"
if token_check "$seed" >/tmp/c1seed.out; then echo "NEGATIVE CONTROL BROKEN: a seeded reissue_token() was not detected"; fail=1
else echo "OK: negative control: a seeded reissue_token() is detected:"; cat /tmp/c1seed.out; fi
rm -f "$seed"
echo "== 3. dependency direction: spike-core has no dependencies"
deps=$(cargo tree --offline -p spike-core -e normal --depth 1 2>/dev/null | tail -n +2)
if [ -n "$deps" ]; then echo "VIOLATION: $deps"; fail=1; else echo "OK: spike-core depends on nothing"; fi
echo "== result: $([ $fail = 0 ] && echo PASS || echo FAIL)"
exit $fail
