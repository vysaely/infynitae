# Infynitae health and event log (`infynitae-health`)

`infynitae-health` collects a PC health snapshot, condenses recent system and Infynitae events into one timeline, and records a 60-second JSON Lines history. The JSON shapes below are a stable interface for the edge UI (`monitor/health` and Settings > This PC) and for Aevyternae (`~/aevyternae-mind`).

## Commands

| Command | Output |
|---|---|
| `infynitae-health now [--json]` | One health snapshot (`ok`, `watch`, or `problem` per check, sorted worst first). |
| `infynitae-health log [--since 1h] [--json]` | Condensed recent events, newest first, one line each with local time. `--since` accepts `Ns`, `Nm`, `Nh`, `Nd`, `YYYY-MM-DD`, or an ISO 8601 timestamp. Default is `1h`. |
| `infynitae-health sample [--no-notify] [--json]` | Collects a snapshot, appends one compact line to the daily JSONL file, rotates files older than 7 days, and notifies once for any item that just entered `problem`. |

## States and thresholds

Every check has one of three lowercase states: `ok`, `watch`, or `problem`. The overall snapshot `state` is the worst state across all items.

| Check | `watch` | `problem` | Fix action (`action`) |
|---|---|---|---|
| Disk free space (`disk:<mount>`) | Below 15% free, or below 15 GB free (on volumes >= 20 GB) | Below 5% free, or below 5 GB free (on volumes >= 20 GB) | `disk-fix` on btrfs `/` |
| Btrfs metadata (`btrfs:<mount>`) | Metadata >= 75% used or < 1 GB free while unallocated space is below max(2 chunks, 2 GB) | Metadata >= 90% used or < 256 MB free while unallocated space is below max(2 chunks, 2 GB) (a full metadata chunk froze the PC on Nov 21) | `disk-fix` on `/` |
| NVMe SMART (`nvme:<dev>`) | Wear >= 80% | Wear >= 90%, critical warning > 0, or media errors > 0. Unreadable without root reports `"needs permission"` with state `ok`. | `null` |
| RAM (`memory:ram`) | >= 80% used | >= 92% used, or < 1 GB available (on hosts >= 4 GB RAM) | `null` |
| Swap (`memory:swap`) | >= 60% used | >= 90% used | `null` |
| Memory & I/O PSI (`memory:psi`) | Memory 1 min `full >= 2%` or `some >= 10%`, or I/O `full >= 25%` | Memory 1 min `full >= 10%` or `some >= 40%`, or I/O `full >= 50%` | `null` |
| CPU (`cpu`) | Temperature >= 80°C, or busy >= 95% with 1 min load >= cores | Temperature >= 90°C | `null` |
| GPU (`gpu`) | Edge >= 80°C or hotspot >= 95°C | Edge >= 90°C or hotspot >= 105°C | `null` |
| Systemd units (`systemd:system`, `systemd:user`) | — | Any failed unit (> 0) | `null` |
| Compositor (`compositor`) | >= 5 stalls (>= 16 ms) or (>= 100 missed frames and >= 1% missed) in 1 hour | >= 20 stalls, (>= 500 missed frames and >= 5% missed), any GPU reset, or `shell.sock` unresponsive | `settings:advanced` |
| Updates (`updates`) | Infynitae behind `origin/main` (> 0 commits) or >= 25 pacman packages waiting | — | `updates` |

When a btrfs volume still has enough unallocated raw space on disk to allocate new metadata chunks (`can_allocate_chunk: true`), a high usage percentage inside the currently allocated chunk stays `ok`.

## Stable JSON schema (`schema_version: 1`)

Additive fields may appear inside version 1. Breaking renames or unit changes increment `schema_version`. Optional fields are omitted when unavailable (`null` / absent).

### `infynitae-health now --json`

Top-level object:

| Field | Type | Unit / notes |
|---|---|---|
| `schema_version` | integer | Always `1`. |
| `timestamp_unix` | integer | Unix seconds (UTC). |
| `time` | string | RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`). |
| `state` | string | `"ok"`, `"watch"`, or `"problem"`. |
| `items` | array | Flat list of checks sorted `problem`, then `watch`, then `ok`. |
| `disks` | array | One entry per real mounted filesystem (btrfs subvolumes on the same device are deduplicated to the primary mount). |
| `nvme` | array | One entry per `/sys/class/nvme/nvme*` controller. |
| `memory` | object | RAM, swap (disk and zswap), and `/proc/pressure/*` PSI averages. |
| `cpu` | object | CPU busy %, load averages, core count, and temperature. |
| `gpu` | object or omitted | First DRM GPU card with busy %, VRAM, temperatures, and power vs cap. |
| `systemd` | object | Failed unit names for system and user managers. |
| `compositor` | object | Missed frames, stalls (>= 16 ms), slow turns, GPU resets, and `shell.sock` status. |
| `updates` | object | Infynitae commits behind `origin/main` and pacman update count. |

#### `items[]`

| Field | Type | Notes |
|---|---|---|
| `id` | string | Stable check key (`disk:/`, `btrfs:/`, `nvme:nvme0`, `memory:ram`, `memory:swap`, `memory:psi`, `cpu`, `gpu`, `systemd:system`, `systemd:user`, `compositor`, `updates`). |
| `category` | string | `"disk"`, `"nvme"`, `"memory"`, `"cpu"`, `"gpu"`, `"systemd"`, `"compositor"`, or `"updates"`. |
| `label` | string | Plain display label (e.g. `"Disk / (I)"`, `"Btrfs metadata /"`). |
| `state` | string | `"ok"`, `"watch"`, or `"problem"`. |
| `summary` | string | Short reading for the UI right column. |
| `reason` | string | One plain sentence with the current value and threshold. |
| `threshold` | string | Short rule summary. |
| `action` | string or omitted | Optional fix action key understood by the edge UI: `"disk-fix"`, `"updates"`, `"settings:advanced"`. |

#### `disks[]`

| Field | Type | Unit |
|---|---|---|
| `mount` | string | Mount point path. |
| `device` | string | Block device path. |
| `fs_type` | string | Filesystem type (`btrfs`, `ext4`, `vfat`, etc.). |
| `label` | string or omitted | Label from `/etc/fstab` comment or btrfs sysfs label. |
| `total_bytes` | integer | Bytes (`statvfs`). |
| `available_bytes` | integer | Unprivileged free bytes (`f_bavail * f_frsize`). |
| `used_pct` | number | Percent (0–100, rounded to 0.1). |
| `state` | string | `"ok"`, `"watch"`, or `"problem"`. |
| `reason` | string | Plain sentence. |
| `btrfs` | object or omitted | Present on btrfs mounts when `/sys/fs/btrfs/<uuid>` is readable. |

`disks[].btrfs`:

| Field | Type | Unit |
|---|---|---|
| `uuid` | string | Btrfs filesystem UUID. |
| `metadata_total_bytes` | integer | Bytes allocated to metadata chunks. |
| `metadata_used_bytes` | integer | Bytes used in metadata chunks. |
| `global_reserve_bytes` | integer | Bytes reserved in btrfs global reserve. |
| `metadata_free_bytes` | integer | Remaining free bytes inside allocated metadata chunks after global reserve. |
| `metadata_used_pct` | number | Percent of allocated metadata chunks occupied (`(used + reserve) / total * 100`, clamped to 100). |
| `chunk_size_bytes` | integer | Metadata chunk size in bytes. |
| `unallocated_bytes` | integer | Raw bytes not yet allocated to any chunk on the volume. |
| `can_allocate_chunk` | boolean | True when `unallocated_bytes >= max(2 * chunk_size_bytes, 2 GiB)`. |
| `state` | string | `"ok"`, `"watch"`, or `"problem"`. |
| `reason` | string | Plain sentence. |

#### `nvme[]`

| Field | Type | Unit |
|---|---|---|
| `device` | string | Controller name (`nvme0`, `nvme1`, ...). |
| `model` | string or omitted | Model string from `/sys/class/nvme/<dev>/model`. |
| `readable` | boolean | True if SMART data was read without root failure. |
| `permission_note` | string or omitted | `"needs permission"` when `/dev/<device>` cannot be opened without root. |
| `wear_pct` | integer or omitted | SMART `percentage_used` (0–100+). |
| `data_written_bytes` | integer or omitted | Bytes written (`data_units_written * 512000`). |
| `temp_c` | integer or omitted | Degrees Celsius (from SMART or hwmon `nvme` sensor, readable without root). |
| `critical_warning` | integer or omitted | SMART critical warning bitmask. |
| `media_errors` | integer or omitted | SMART media and data integrity errors count. |
| `state` | string | `"ok"`, `"watch"`, or `"problem"`. |
| `reason` | string | Plain sentence. |

#### `memory`

| Field | Type | Unit |
|---|---|---|
| `ram_total_bytes` | integer | Bytes (`MemTotal`). |
| `ram_available_bytes` | integer | Bytes (`MemAvailable`). |
| `ram_used_bytes` | integer | Bytes (`ram_total_bytes - ram_available_bytes`). |
| `ram_used_pct` | number | Percent (0–100). |
| `swap_total_bytes` | integer | Bytes (`SwapTotal`). |
| `swap_used_bytes` | integer | Bytes (`SwapTotal - SwapFree`, includes zswap-backed pages). |
| `swap_used_pct` | number | Percent (0–100). |
| `swap_disk_used_bytes` | integer | Bytes estimated on disk swap (`swap_used_bytes - zswap_stored_bytes`). |
| `swap_devices` | array | Active `/proc/swaps` entries: `{ path, kind, size_bytes, used_bytes, priority }`. |
| `zswap_enabled` | boolean | From `/sys/module/zswap/parameters/enabled`. |
| `zswap_pool_bytes` | integer | Compressed RAM used by zswap (`Zswap:` in `/proc/meminfo`). |
| `zswap_stored_bytes` | integer | Uncompressed swapped pages held in zswap (`Zswapped:` in `/proc/meminfo`). |
| `pressure` | object or omitted | `{ memory, io, cpu }`, each `{ some, full }` with `{ avg10, avg60, avg300, total_us }` from `/proc/pressure/*` (averages in percent, `total_us` in microseconds). |

#### `cpu`, `gpu`, `systemd`, `compositor`, `updates`

- `cpu`: `busy_pct` (0–100, from `/proc/stat` delta against the saved sample or a 100 ms interval in `now`), `load_1m`, `load_5m`, `load_15m`, `cores`, `temp_c` (°C, `k10temp`/`coretemp`), `state`, `reason`.
- `gpu`: `card`, `name`, `busy_pct` (0–100), `vram_used_bytes`, `vram_total_bytes`, `temp_c` (°C edge), `junction_temp_c` (°C hotspot), `mem_temp_c` (°C memory), `power_watts` (W), `power_cap_watts` (W), `power_pct` (0–100+), `state`, `reason`.
- `systemd`: `system_available`, `system_failed` (array of unit names), `user_available`, `user_failed` (array of unit names), `state`, `reason`.
- `compositor`: `window` (`"1h"` or `"session"`), `latency_stats_enabled`, `missed_frames_1h`, `total_frames_1h`, `missed_pct_1h`, `stalls_1h` (turns >= 16 ms), `slow_turns_1h`, `worst_stall_ms` (ms), `gpu_resets_1h`, `shell_responsive` (boolean from `shell.sock` snapshot ping), `counters` (cumulative counters for delta tracking across restarts), `state`, `reason`.
- `updates`: `infynitae_behind` (commits behind `origin/main`), `pacman_updates` (package count from `update-status` cache or `checkupdates`), `checkupdates_present`, `checked_at_unix`, `state`, `reason`.

### `infynitae-health log --since 1h --json`

Top-level object:

| Field | Type | Notes |
|---|---|---|
| `schema_version` | integer | Always `1`. |
| `since_unix` | integer | Window start in Unix seconds (UTC). |
| `since` | string | RFC 3339 UTC timestamp. |
| `generated_unix` | integer | Collection time in Unix seconds (UTC). |
| `generated` | string | RFC 3339 UTC timestamp. |
| `events` | array | Condensed events, sorted newest first (`timestamp_unix` descending). |

Each `events[]` item:

| Field | Type | Values / notes |
|---|---|---|
| `timestamp_unix` | integer | Unix seconds (UTC). |
| `time` | string | RFC 3339 UTC timestamp. |
| `local_time` | string | Short local wall time (`HH:MM` when same local date, otherwise `YYYY-MM-DD HH:MM`). |
| `kind` | string | One of `"crash"`, `"oom"`, `"unit_failed"`, `"disk_guard"`, `"gpu_reset"`, `"usb_drive"`, `"suspend"`, `"update"`. |
| `severity` | string | `"problem"`, `"watch"`, or `"info"`. |
| `source` | string | Origin such as `"coredumpctl"`, `"infynitae"`, `"kernel"`, `"systemd"`, `"disk-guard"`, `"automount"`, `"sleep-lock"`, `"pacman"`, `"infynitae-update"`, `"health"`. |
| `summary` | string | One plain line describing the event. |

### 60 s JSONL history (`~/.local/state/infynitae/health/YYYY-MM-DD.jsonl`)

The user timer (`infynitae-health.timer`) runs `infynitae-health sample` every 60 seconds (`Nice=19`, `IOSchedulingClass=idle`). Each line is a compact JSON object (`0600` file mode) with:

- `schema_version`, `timestamp_unix`, `time`, `state`
- `alerts`: array of non-`ok` items (`{ id, label, state, summary, reason }`, omitted when empty)
- `disks`: array of `{ mount, available_bytes, total_bytes, used_pct, state, btrfs_used_pct, btrfs_free_bytes, btrfs_unallocated_bytes, btrfs_state }`
- `ram_used_bytes`, `ram_total_bytes`, `swap_disk_used_bytes`, `zswap_pool_bytes`
- `psi_mem_full_60`, `psi_mem_some_60`, `psi_io_full_60`
- `cpu_busy_pct`, `cpu_temp_c`, `load_1m`
- `gpu_busy_pct`, `gpu_temp_c`, `gpu_junction_temp_c`, `gpu_power_watts`, `gpu_power_cap_watts`
- `failed_system_units`, `failed_user_units` (omitted when empty)
- `missed_frames_1h`, `stalls_1h`, `gpu_resets_1h`, `compositor_counters`
- `infynitae_behind`, `pacman_updates`

Daily files older than 7 UTC days are deleted on each sample write.

## Notifications

`infynitae-health sample` stores currently notified problem item IDs in `~/.local/state/infynitae/health/notified.json`. It sends one critical desktop notification (`notify-send -u critical -a infynitae-health`) when an item enters `problem`. It does not notify for `watch`, does not repeat while the item stays `problem`, and clears the ID once the item returns to `watch` or `ok`.

## Measured cost per sample

Measured on this host (8 mounted drives, 2 NVMe controllers, AMD RX 9070 XT, 20 consecutive steady-state runs of `infynitae-health sample --no-notify` with release binary `975 KB`, `resource.getrusage(RUSAGE_CHILDREN)`):

- CPU time per sample: **5.29 ms** (2.07 ms user + 3.23 ms sys), **0.0088%** of one CPU core over a 60 s interval
- Wall time per sample: **11.15 ms**
- JSONL size: **2,486 bytes/sample** (~3.41 MB/day, ~23.9 MB across 7 days with 8 drives and 3 active alerts; ~1.8 KB/sample when all checks are `ok`)
- `sample` uses saved `/proc/stat` counters in `cpu-prev` (no sleep), checks `/dev/nvmeN` readability before spawning SMART tools, and reads only the last 256 KB of the daily JSONL file for 1-hour compositor deltas so runtime stays constant through the day.
