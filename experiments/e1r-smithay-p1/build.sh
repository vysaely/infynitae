#!/bin/sh
# E1R isolated experiment: materialise smithay 0.7.0 from the local cargo registry (offline), apply
# the single P1 patch, and build the shared xlock_batch source against it. Nothing outside this
# directory is modified; experiments/b8-harness keeps building against the published crate.
set -eu
cd "$(dirname "$0")"
SRC=$(ls -d "$HOME"/.cargo/registry/src/*/smithay-0.7.0 | head -1)
rm -rf vendor && mkdir -p vendor
cp -r "$SRC" vendor/smithay-0.7.0
# registry sources carry a .cargo-ok marker and Cargo.toml.orig; neither affects the build
# apply P1 (no `patch` binary on this host; exact-match replacement, must match once)
python3 - <<'PY'
p = 'vendor/smithay-0.7.0/src/wayland/seat/pointer.rs'
s = open(p).read()
old = """            with_pointer_constraint(self, &pointer, |constraint| {
                if let Some(constraint) = constraint {
                    constraint.deactivate();
                }
            });"""
new = """            with_pointer_constraint(self, &pointer, |constraint| {
                if let Some(constraint) = constraint {
                    if constraint.is_active() {
                        constraint.deactivate();
                    }
                }
            });"""
assert s.count(old) == 1, 'P1 context mismatch'
open(p, 'w').write(s.replace(old, new))
print('P1 applied:', p)
PY
diff -u "$SRC/src/wayland/seat/pointer.rs" vendor/smithay-0.7.0/src/wayland/seat/pointer.rs > vendor/P1.applied.diff || true
cp ../b8-harness/Cargo.lock Cargo.lock
cargo build --offline --release --bins
