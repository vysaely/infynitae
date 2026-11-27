# Recovery and Rollback

Updates on this machine split into Infynitae itself (`infynitae-update`) and Arch packages (`pacman` + `snapper`).

## 1. Rolling back Infynitae

Before replacing any compiled binary in `~/.local/bin`, `infynitae-update` (along with `live/build-helpers.sh` and `live/build-satellite.sh`) copies the old binary to `~/.local/bin/<name>.previous`. Whenever an update changes at least one binary or script commit, unchanged binaries keep matching `.previous` copies so the whole `.previous` set stays on one generation.

- **Manual rollback:** run `infynitae-update --rollback` (needs no `sudo`). It swaps `~/.local/bin/<name>` and `<name>.previous` back, resets a clean checkout back to the previous commit in `~/.local/state/infynitae/update-rollback` when scripts changed (or prints the `git reset --hard` command if local edits exist), and reloads running helpers. If `infynitae` itself changed, log out (`Ctrl+Alt+Backspace`) or reboot to start the swapped compositor.
- **Automatic fallback at startup:** if a new `~/.local/bin/infynitae` fails twice within 5 s at start, `infynitae-session` starts `~/.local/bin/infynitae.previous` instead and `live/session.sh` posts a critical notification pointing to `infynitae-update --rollback`.

## 2. Rolling back Arch updates (`snap-pac`, `snapper`, Limine)

### How `snap-pac` fits the existing setup

`snapper 0.13.2-2` and `limine 12.9.6-1` are installed; `snap-pac` (`3.0.1-3` in `extra`) is optional in `install.sh --snap-pac` and the installer menu (`snap-pac`, off by default so nothing installs unasked).

Observed settings on this PC:

- `/etc/conf.d/snapper`: `SNAPPER_CONFIGS="root home"` (`/` on btrfs subvolume `/@`, `/home` on `/@home`, 397 GiB partition).
- `/etc/snapper/configs/root`:
  - `QGROUP=""`, `SPACE_LIMIT="0.2"`, `FREE_LIMIT="0.2"`
  - `NUMBER_CLEANUP="yes"`, `NUMBER_MIN_AGE="3600"`, `NUMBER_LIMIT="50"`, `NUMBER_LIMIT_IMPORTANT="10"`, `EMPTY_PRE_POST_CLEANUP="yes"`
  - `TIMELINE_CREATE="yes"`, `TIMELINE_CLEANUP="yes"`, `TIMELINE_LIMIT_HOURLY="3"`, `TIMELINE_LIMIT_DAILY="2"` (weekly/monthly/quarterly/yearly `0`)
  - Enabled timers: `snapper-cleanup.timer`, `snapper-timeline.timer`, `btrfs-balance-light.timer`, `infynitae-disk-guard.timer`.

Compatibility with `live/infynitae-disk-guard`:

- `snap-pac` takes one `pre` and one `post` snapshot per `pacman` transaction (~25 updates under `NUMBER_LIMIT="50"`, after dropping unchanged pairs via `EMPTY_PRE_POST_CLEANUP="yes"`).
- Because `QGROUP=""` is empty, `SPACE_LIMIT="0.2"` has no btrfs quota group accounting; space-based pruning relies on `FREE_LIMIT="0.2"` (20% free on `/`, ~79.4 GiB on this drive). That acts far earlier than `live/infynitae-disk-guard`, which triggers its 4 GiB reserve release and `btrfs balance start -dusage=30 -dlimit=10 /` only below 2 × chunk unallocated and 256 MiB metadata free.
- Because snapshots pin old extents on btrfs, deleting unneeded snapshot pairs (`sudo snapper -c root delete <N>`) frees extents if unallocated space stays tight.

### Which snapshot to choose

List snapshots and find the `pre` snapshot just before the problematic upgrade (`Description` records the `pacman` command):

```sh
sudo snapper -c root list
sudo snapper -c root status <pre>..0
```

Pick `<pre>` (clean state immediately before `pacman`). Your files in `/home` (`/@home`) sit on a separate subvolume, so rolling `/` back leaves `/home` untouched.

### Rolling back when the system boots

Revert changes directly while running (then reboot):

```sh
sudo snapper -c root undochange <pre>..0
sudo reboot
```

### Booting or rolling back from Limine

Two layout details observed in `/etc/fstab` and `/proc/cmdline` matter when recovering from Limine:

1. **`/boot` is `vfat` outside `@`:** `/dev/nvme0n1p3` (UUID `3b60f0cc-bdfb-465a-a98d-95d1e6c1294a`) holds `@` and `@home`, while `/boot` (`UUID=7031-573A`, `fmask=0177,dmask=0077`) is not snapshotted. Also, `/boot/EFI/BOOT/limine.conf` is readable only by `root` and was not read during this build; inspect it on the host with `sudo cat /boot/EFI/BOOT/limine.conf`.
2. **`subvol=@` is pinned explicitly:** `/proc/cmdline` passes `rootflags=subvol=@`, and `/etc/fstab` mounts `subvol=/@`. Changing only btrfs's default subvolume (`snapper rollback` alone) does not switch boots unless you change the subvolume passed to the kernel or replace `@`.

Temporary test boot from the Limine menu:

1. Highlight your Arch entry in Limine and press `E` to edit its boot lines for this boot.
2. Replace `rootflags=subvol=@` on `KERNEL_CMDLINE` with `rootflags=subvol=@/.snapshots/<N>/snapshot` (or verify how your entry spells `rootflags=` in `sudo cat /boot/EFI/BOOT/limine.conf`). Snapper's snapshots are read-only by default; use `undochange` once booted via a live environment or create a writable snapshot below.

Permanent rollback from a live Arch ISO (when the broken `@` does not boot cleanly):

```sh
# 1. Identify your btrfs partition (root UUID 3b60f0cc-bdfb-465a-a98d-95d1e6c1294a)
lsblk -f
mount -o subvolid=5 /dev/<root-part> /mnt

# 2. Keep the broken @ aside and snapshot the chosen good state to @
mv /mnt/@ /mnt/@.broken
btrfs subvolume snapshot /mnt/@.broken/.snapshots/<N>/snapshot /mnt/@
mkdir -p /mnt/@/.snapshots

# 3. If pacman upgraded the linux kernel, chroot and reinstall the matching kernel from cache:
#    mount /dev/<boot-part> /mnt/@/boot
#    arch-chroot /mnt/@ pacman -U /var/cache/pacman/pkg/linux-*.pkg.tar.zst

umount /mnt
reboot
```

After confirming the recovered system works, remove `@.broken` from a root shell (`mount -o subvolid=5 /dev/<root-part> /mnt && btrfs subvolume delete /mnt/@.broken && umount /mnt`).
