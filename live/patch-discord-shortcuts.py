#!/usr/bin/env python3
"""
Patches Discord's discord_utils.node so lone Right Shift (X11 62 / evdev 54) and
Right Ctrl (X11 105 / evdev 97) keybinds are accepted by Wayland GlobalShortcuts
portal as F15 and F16 triggers instead of being declined as "not representable".
"""
import glob
import os
import struct
import sys

def patch_file(path: str) -> bool:
    try:
        with open(path, "rb") as f:
            data = bytearray(f.read())
    except OSError:
        return False

    orig = bytes(data)

    # 1. Patch kKeyMap evdev codes for F13..F16 (183, 184, 185, 186 -> 42, 29, 54, 97)
    # Each entry is 16 bytes: <uint64 evdev_code, uint64 char_ptr>
    for old_evdev, new_evdev, name in [
        (183, 42, b"F13\x00"),
        (184, 29, b"F14\x00"),
        (185, 54, b"F15\x00"),
        (186, 97, b"F16\x00"),
    ]:
        # Search for 8-byte little-endian old_evdev in .data.rel.ro
        needle = struct.pack("<Q", old_evdev)
        idx = 0
        while True:
            pos = data.find(needle, idx)
            if pos == -1:
                break
            # Verify adjacent entries (183..186 are contiguous at 16-byte stride)
            if old_evdev == 183:
                if (
                    pos + 64 <= len(data)
                    and struct.unpack_from("<Q", data, pos + 16)[0] in (184, 29)
                    and struct.unpack_from("<Q", data, pos + 32)[0] in (185, 54)
                    and struct.unpack_from("<Q", data, pos + 48)[0] in (186, 97)
                ):
                    struct.pack_into("<Q", data, pos, 42)
                    struct.pack_into("<Q", data, pos + 16, 29)
                    struct.pack_into("<Q", data, pos + 32, 54)
                    struct.pack_into("<Q", data, pos + 48, 97)
                    break
            idx = pos + 8

    # 2. Patch IsModifierLinuxCode(int) bitmasks: clear bit 0 of 0x30000009 (evdev 97)
    # and bit 54 of 0x0140040020000000 (evdev 54)
    old_mod_fn = bytes.fromhex(
        "b9090000300fa3f9721531c0c389f948ba0000002000044001"
    )
    new_mod_fn = bytes.fromhex(
        "b9080000300fa3f9721531c0c389f948ba0000002000004001"
    )
    if old_mod_fn in data:
        data = data.replace(old_mod_fn, new_mod_fn)

    # 3. Patch IsPortalEligible and BuildTrigger Loop 1 modifier bitmasks:
    # clear bit 0 of 0x30000009 (X11 105 = Right Ctrl) and bit 25 of 0x0a002001 (X11 62 = Right Shift)
    old_mask = bytes.fromhex("be09000030bf0120000a")
    new_mask = bytes.fromhex("be08000030bf01200008")
    if old_mask in data:
        data = data.replace(old_mask, new_mask)

    # 4. Patch BuildTrigger Loop 2:
    # (a) X11 105 (0x69) check: nop out `je` so kc=105 falls through to kKeyMap lookup
    old_kc105 = bytes.fromhex("4183f96974b34183f96c74a7")
    new_kc105 = bytes.fromhex("4183f96990904183f96c74a7")
    if old_kc105 in data:
        data = data.replace(old_kc105, new_kc105)

    # (b) Jump table entry 25 (kc = 37 + 25 = 62, Right Shift):
    # Find the 28-entry jump table in .rodata where entry[1..12] == entry[14..24]
    # and entry[13] == entry[25] (Left Shift & Right Shift)
    for pos in range(0x100000, len(data) - 28 * 4, 4):
        e0, e1, e2, e13, e25, e26, e27 = (
            struct.unpack_from("<i", data, pos)[0],
            struct.unpack_from("<i", data, pos + 4)[0],
            struct.unpack_from("<i", data, pos + 8)[0],
            struct.unpack_from("<i", data, pos + 13 * 4)[0],
            struct.unpack_from("<i", data, pos + 25 * 4)[0],
            struct.unpack_from("<i", data, pos + 26 * 4)[0],
            struct.unpack_from("<i", data, pos + 27 * 4)[0],
        )
        if (
            e1 == e2 == e26
            and e1 != e0
            and e1 != e13
            and e1 != e27
            and e13 == e25
            and -0x100000 < e1 < 0
        ):
            # Point entry[25] (kc=62) to the default non-modifier target (e1)
            struct.pack_into("<i", data, pos + 25 * 4, e1)
            break

    if bytes(data) == orig:
        return False

    tmp = path + ".tmp"
    with open(tmp, "wb") as f:
        f.write(data)
    os.replace(tmp, path)
    return True

def fix_openh264() -> None:
    """
    Discord 1.0.167's web frontend downloads `libopenh264-2.7.0-linux64.8.so` and runs
    `cleanupUnusedOpenH264Files(["libopenh264-2.7.0-linux64.8.so"])` on startup (which
    deletes `libopenh264-2.5.1-linux64.7.so`), while `core.asar` and `app.asar` still
    hardcode `libopenh264-2.5.1-linux64.7.so`.
    Both strings are 30 bytes: patch `core.asar` and `app.asar` in-place to reference
    the installed `libopenh264-2.7.0-linux64.8.so`, and patch `libffmpeg.so` so a failed
    `dlopen` does not permanently poison `ff_libopenh264_load_library` for the process.
    """
    cache_dir = os.path.expanduser("~/.config/discord/discord_asset_cache/openh264")
    candidates = sorted(
        [
            os.path.basename(p)
            for p in glob.glob(os.path.join(cache_dir, "libopenh264-*.so"))
            if not os.path.islink(p)
        ],
        reverse=True,
    ) if os.path.isdir(cache_dir) else []
    active_so = candidates[0] if candidates else "libopenh264-2.7.0-linux64.8.so"

    old_name = b"libopenh264-2.5.1-linux64.7.so"
    new_name = active_so.encode("ascii")
    if len(old_name) == len(new_name) and old_name != new_name:
        asar_paths = glob.glob(
            os.path.expanduser(
                "~/.config/discord/app-*/modules/discord_desktop_core-*/discord_desktop_core/core.asar"
            )
        ) + glob.glob(
            os.path.expanduser("~/.config/discord/app-*/resources/app.asar")
        )
        for ap in asar_paths:
            try:
                with open(ap, "rb") as f:
                    adata = f.read()
                if old_name in adata:
                    adata = adata.replace(old_name, new_name)
                    tmp = ap + ".tmp"
                    with open(tmp, "wb") as f:
                        f.write(adata)
                    os.replace(tmp, ap)
                    print(f"Patched OpenH264 filename in {ap} -> {active_so}")
            except OSError:
                pass

    # Also patch libffmpeg.so so failed dlopen doesn't set tried_loading=1 permanently
    for ff_path in glob.glob(os.path.expanduser("~/.config/discord/app-*/libffmpeg.so")):
        try:
            with open(ff_path, "rb") as f:
                fdata = bytearray(f.read())
            # movb $0x1, tried_loading(%rip) before dlopen -> movb $0x0, tried_loading(%rip)
            old_ff = bytes.fromhex("c605a843120001be02000000")
            new_ff = bytes.fromhex("c605a843120000be02000000")
            if old_ff in fdata:
                fdata = fdata.replace(old_ff, new_ff)
                tmp = ff_path + ".tmp"
                with open(tmp, "wb") as f:
                    f.write(fdata)
                os.replace(tmp, ff_path)
                print(f"Patched {ff_path}")
        except OSError:
            pass

    if os.path.isdir(cache_dir):
        expected = os.path.join(cache_dir, "libopenh264-2.5.1-linux64.7.so")
        if not os.path.exists(expected) and active_so != "libopenh264-2.5.1-linux64.7.so":
            try:
                if os.path.lexists(expected):
                    os.unlink(expected)
                os.symlink(active_so, expected)
            except OSError:
                pass

def main() -> None:
    fix_openh264()
    paths = sys.argv[1:] or glob.glob(
        os.path.expanduser(
            "~/.config/discord/app-*/modules/discord_utils-*/discord_utils/discord_utils.node"
        )
    )
    for p in paths:
        if patch_file(p):
            print(f"Patched {p}")

if __name__ == "__main__":
    main()
