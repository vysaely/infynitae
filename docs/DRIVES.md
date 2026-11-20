# Drives named by Roman numeral

Drives get names the way Windows gives drive letters (C:, D:, E:), with Roman numerals instead:
I, II, III and so on up to XXXIX. A drive's name is also its folder: drive V opens at `/mnt/V`,
and file managers list it as "V". This is the default (`DRIVE_NAMES="roman"` in the config;
Settings, Apps, More: "Name drives by Roman numeral").

## Which drive gets which numeral

- **Drives named by hand keep their names.** A line you wrote in `/etc/fstab` with a mount folder
  `/mnt/<numeral>` or the option `x-gvfs-name=<numeral>` holds that numeral (for example `/` as I,
  `/mnt/II` as II). Infynitae never changes those lines.
- **A partition inside the PC** (a Windows partition, a second disk) gets a numeral the first time
  the automount watcher sees it unmounted, and keeps it. It is not mounted then: it shows in the
  file manager as that numeral and opens at its folder when you open it.
- **A USB drive or memory card** gets, when plugged in, the numeral it had last time if no drive
  plugged in now uses it; otherwise the lowest numeral that no fixed drive and no plugged-in drive
  has. A drive that is unplugged leaves its numeral free for others; when it comes back while that
  numeral is taken, it gets another one, and it gets its own back the next time that one is free.
- **No numeral** for: swap, the boot and EFI partitions (udisks marks them to be ignored),
  Ventoy's `VTOYEFI` partition, the system's own filesystem and its other subvolumes, a locked
  encrypted drive (the unlocked filesystem inside gets the numeral), and filesystems other than
  FAT, exFAT, NTFS, btrfs, ext4, XFS and F2FS. These mount, when they would before, under
  `/run/media/<you>/<label>`.

The notification says "V mounted", with the drive's label below it, and offers Open and Eject.
`infynitae-automount eject` ejects every USB drive or card that is mounted, named or not.

## Where the names are written

At the end of `/etc/fstab`, between two marker lines, one comment and one line per drive:

```
# infynitae drives: begin
# Drive names given by Infynitae (docs/DRIVES.md): rename a drive by changing its /mnt/ and x-gvfs-name=, forget it by deleting its two lines
# removable: <the drive's label>
UUID=<uuid> /mnt/V exfat noauto,nofail,x-systemd.device-timeout=5s,x-udisks-auth,x-gvfs-show,x-gvfs-name=V,nosuid,nodev,uid=<your uid>,gid=<your gid>,umask=022,iocharset=utf8,errors=remount-ro 0 0
# fixed: <the partition's label>
UUID=<uuid> /mnt/VI ntfs3 noauto,nofail,x-systemd.device-timeout=5s,x-udisks-auth,x-gvfs-show,x-gvfs-name=VI,nosuid,nodev,uid=<your uid>,gid=<your gid>,umask=022 0 0
# infynitae drives: end
```

What the options do: `noauto,nofail` and the 5 second timeout mean the computer never waits for
these drives when it starts; `x-udisks-auth` lets udisks mount them for you; `x-gvfs-show` and
`x-gvfs-name` make file managers list them by numeral; on FAT, exFAT and NTFS, which have no file
owners of their own, `uid` and `gid` make the files yours. The other options are the ones udisks
uses for that filesystem.

The lines are written by `/usr/local/lib/infynitae-drive-name`, a small program that runs as root
(through pkexec) and changes nothing outside the block. Before each change it keeps the file as it
was in `/etc/fstab.infynitae-bak`, and it writes the new file only when `findmnt --verify` finds no
error in it that the old file did not have. The rule
`/etc/polkit-1/rules.d/50-infynitae-drive-names.rules` lets the person at the computer run that
program and mount these drives without a password (SECURITY.md). The installer and
`infynitae-update` put both in place; without them, drives mount under `/run/media` by their
label, as before, and `~/.local/state/infynitae/automount.log` says why.

## Renaming or forgetting a drive

Edit the block as root (`sudoedit /etc/fstab`), then run `sudo systemctl daemon-reload`.

- **Rename:** change the numeral in both places on the drive's line, `/mnt/V` and
  `x-gvfs-name=V`. The new folder is made when the drive next mounts.
- **Keep a USB drive's numeral for it alone:** change its comment from `# removable:` to
  `# fixed:`.
- **Forget a drive:** delete its comment and its line. It gets a numeral again the next time it is
  plugged in. Delete the whole block, markers included, to forget every drive.

`DRIVE_NAMES="label"` stops giving new numerals: new drives mount under `/run/media` by their
label. Lines already in the block still apply (udisks reads `/etc/fstab` for every mount), so
delete the block too to have every drive back under `/run/media`.
