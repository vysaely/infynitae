#!/bin/sh
# Builds xwayland-satellite with Infynitae's patch into a bin directory (install.sh, update.sh).
#   build-satellite.sh <repo> <bin dir>
# The patch answers a window's request to minimize; without it Proton games stay black after a
# focus change (patches/xwayland-satellite-*.patch). A failed build changes nothing: the session
# keeps the copy it finds, the system's if there is no other.
REPO="$1"
BIN="$2"
TAG="v0.8.3"
COMMIT="b83eab900644e4c7c77982ce3d44cb490f0c5e1d"
PATCH="$REPO/patches/xwayland-satellite-0001-answer-minimize.patch"
WORK="${XDG_CACHE_HOME:-$HOME/.cache}/infynitae-satellite"
STAMP="$BIN/.xwayland-satellite.stamp"

fail() {
    echo "[build-satellite] $1; X11 apps keep the xwayland-satellite already installed" >&2
    exit 0
}

want="$COMMIT $(sha256sum "$PATCH" | cut -d' ' -f1)"
if [ -x "$BIN/xwayland-satellite" ] && [ "$(cat "$STAMP" 2>/dev/null)" = "$want" ]; then
    exit 0
fi
command -v git >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1 || fail "git or cargo is missing"

rm -rf "$WORK/src"
mkdir -p "$WORK" "$BIN"
git clone -q --depth 1 --branch "$TAG" https://github.com/Supreeeme/xwayland-satellite.git "$WORK/src" \
    || fail "could not download it"
[ "$(git -C "$WORK/src" rev-parse HEAD)" = "$COMMIT" ] || fail "$TAG is not the expected commit"
git -C "$WORK/src" apply "$PATCH" || fail "the patch does not apply"
(
    cd "$WORK/src" || exit 1
    RUSTUP_TOOLCHAIN=$(sed -n 's/^channel = "\(.*\)"/\1/p' "$REPO/rust-toolchain.toml") \
        CARGO_TARGET_DIR="$WORK/target" cargo build --release --locked -q
) || fail "the build failed"
cp "$WORK/target/release/xwayland-satellite" "$BIN/.xwayland-satellite.new" || fail "could not copy it"
mv -f "$BIN/.xwayland-satellite.new" "$BIN/xwayland-satellite" || fail "could not install it"
echo "$want" > "$STAMP"
echo "[build-satellite] xwayland-satellite $TAG with Infynitae's patch installed"
