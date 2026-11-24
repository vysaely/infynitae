# The test VM in several shapes

Infynitae is tried on one AMD PC with one monitor setup. `run` boots the test VM
(`~/.cache/infynitae-vm/run`, the one `vm.sh` and `device.sh` start) once per shape and checks each:

    tools/vm-matrix/run                    all shapes, one after another (each boots twice: once
                                           to write its settings, once to start with them)
    tools/vm-matrix/run scale-2 laptop     only these
    tools/vm-matrix/run --dry-run          the qemu lines and settings, nothing started
    tools/vm-matrix/run --list             the shape names

| Shape | Screen | What it changes |
|---|---|---|
| scale-1, scale-1.25, scale-1.5, scale-2 | 1920x1080 | `OUTPUT_SCALE` |
| two-screens | two 1920x1080 side by side (virtio-gpu `max_outputs=2`) | Virtual-1 at 1, Virtual-2 at 1.5 |
| odd-1366x768 | 1366x768 | adds the everyday test `small_screen` |
| laptop | 1920x1080 | a battery from the kernel's `test_power` module, a touchpad made through uinput |
| no-xwayland | 1920x1080 | `XWAYLAND="false"` |

Each shape runs on a copy-on-write layer over `disk.qcow2`: the VM's own disk is never changed, and
the layer is deleted afterwards (`KEEP_DISKS=1` keeps it). Rendering is software (virtio-vga without
3D), so the host GPU is not used.

In each shape, in this order:

1. `smoke.py` (copied in as `~/vm-matrix-smoke.py`, run as root because it drives uinput): the
   session is up and unlocks with the password; the screens, their modes and scales are the ones the
   shape asks for (two screens: side by side); Super+Q opens a terminal wholly on a screen; Super+A
   draws the control menu and Escape closes it (read from the edge's `INFYNITAE_EDGE_BOXES` dump);
   `grim` takes a screenshot of the screen's size; Super+Shift+L locks and the password unlocks.
   Laptop: a battery appears and the session runs on at 5% without power; a backlight is looked
   for; a touchpad tap outside the open control menu closes it. No XWayland: `xterm` gets no
   window and no X server starts.
2. Tests from `tools/everyday-use-tests` (copied in as `~/g/everyday.py`): `EVERYDAY_TESTS`, by
   default those that leave the session running and need only a terminal, mousepad and xterm.
3. Scripts from `tools/vm-edge` (copied into `~`): `VM_EDGE_TESTS`. The ones built on `~/g/feat.py`
   run only where the VM image has that file (it is not in this repository); otherwise their row
   says SKIP.

Results in `OUT` (default `~/.cache/infynitae-vm/run/matrix/DATE-TIME`): `results.tsv` with one row
per check (shape, check, PASS / FAIL / SKIP, or LOOK where a person must look at the screenshot, and
the screenshot's path), printed as a table at the end; per shape the screenshots in `shots/`, each
test's output, the session log of the checked boot and the serial log.

The VM tests what its disk image has installed: update the image before a round.

## What only real hardware shows

The VM has one virtual GPU drawing in software, a 75 Hz virtual screen and no real devices behind
it. These need another machine:

- NVIDIA with its own driver (`nvidia-drm`): GBM and buffer modifiers, explicit sync, the GL renderer.
- Two GPUs (a laptop's built-in one and a separate one): which one renders, screens on the other,
  the copies between them.
- HDR and 10-bit colour: virtio-gpu offers neither.
- Variable refresh rate, refresh rates above 75 Hz, real vblank timing: the VM's "Latency" and
  "Frame timing" numbers are not those of a real screen.
- Overlay planes, direct scanout of a fullscreen game, tearing: virtio-gpu has one plane and a cursor.
- A real backlight and the brightness keys, the lid switch, sleep and wake with a GPU, the
  battery's charge limit (`BATTERY_LIMIT`). The kernel's test battery is named `test_battery`;
  the installer's checks look for `BAT*` and do not see it.
- A real touchpad (palm detection, pressure, clickpad areas), a touchscreen, a pen, the
  accelerometer that turns the screen.
- Plugging screens in and out over DisplayPort or HDMI, docks with several screens on one cable,
  screens with odd EDIDs, how sharp fractional scales look on a real panel.
- Wi-Fi, Bluetooth and sound hardware behind the control menu's rows.
