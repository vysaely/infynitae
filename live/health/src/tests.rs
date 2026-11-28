use super::*;
use crate::collect::{
    collect_snapshot_with, compute_window_deltas, evaluate_btrfs_metadata, evaluate_cpu,
    evaluate_disk_space, evaluate_gpu, evaluate_nvme, evaluate_pressure, evaluate_ram,
    evaluate_swap, evaluate_updates, parse_fstab_names, parse_hwmon_temp, parse_loadavg,
    parse_meminfo, parse_mountinfo, parse_nvme_cli_json, parse_proc_stat, parse_proc_swaps,
    parse_psi_file, parse_session_log, parse_smartctl_nvme_json, parse_systemctl_failed,
    parse_update_status, render_now_text,
};
use crate::history::{
    append_and_rotate, notify_new_problems, parse_jsonl_date, read_latest_sample,
    read_notified_problems, read_samples_since, rotate_old_files, transition_problems,
    write_cpu_prev, CpuPrev,
};
use crate::log::{
    collect_log, extract_health_disk_warnings, finalize_events, parse_automount_log,
    parse_coredumpctl_json, parse_crash_reports_dir, parse_install_log, parse_journal_jsonl,
    parse_pacman_log, parse_sleep_lock_log, render_log_text,
};
use crate::model::{
    date_utc, format_bytes, iso_utc, parse_since_arg, parse_timestamp_to_unix, CompositorCounters,
    HealthItem, HistorySample, State, SCHEMA_VERSION,
};
use crate::sys::SysEnv;
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "infynitae-health-test-{tag}-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture_env(root: &Path, now_unix: i64, sample_mode: bool) -> SysEnv {
    let proc_dir = root.join("proc");
    let sys_dir = root.join("sys");
    let etc_dir = root.join("etc");
    let dev_dir = root.join("dev");
    let var_log_dir = root.join("var/log");
    let state_dir = root.join("state/infynitae");
    let runtime_dir = root.join("run/infynitae");
    let config_dir = root.join("config/infynitae");
    for d in [
        &proc_dir,
        &proc_dir.join("self"),
        &proc_dir.join("pressure"),
        &sys_dir,
        &etc_dir,
        &dev_dir,
        &var_log_dir,
        &state_dir,
        &runtime_dir,
        &config_dir,
    ] {
        std::fs::create_dir_all(d).unwrap();
    }
    SysEnv {
        proc_dir,
        sys_dir,
        etc_dir,
        dev_dir,
        var_log_dir,
        amdgpu_ids: root.join("amdgpu.ids"),
        state_dir,
        runtime_dir,
        config_dir,
        home_dir: Some(root.join("home")),
        path_env: None,
        now_unix,
        sample_mode,
        run_commands: false,
    }
}

#[test]
fn cli_args_parse_and_reject_unknown_flags() {
    assert_eq!(
        parse_args(&["now".into()]),
        Ok(Command::Now { json: false })
    );
    assert_eq!(
        parse_args(&["now".into(), "--json".into()]),
        Ok(Command::Now { json: true })
    );
    assert_eq!(
        parse_args(&[
            "log".into(),
            "--since".into(),
            "24h".into(),
            "--json".into()
        ]),
        Ok(Command::Log {
            since: "24h".into(),
            json: true
        })
    );
    assert_eq!(
        parse_args(&["log".into(), "--since=30m".into()]),
        Ok(Command::Log {
            since: "30m".into(),
            json: false
        })
    );
    assert_eq!(
        parse_args(&["sample".into(), "--no-notify".into()]),
        Ok(Command::Sample {
            notify: false,
            json: false
        })
    );
    assert!(parse_args(&[]).is_err());
    assert!(parse_args(&["bogus".into()]).is_err());
    assert!(parse_args(&["now".into(), "--bad".into()]).is_err());
    assert!(parse_args(&["log".into(), "--since".into()]).is_err());
}

#[test]
fn timestamps_and_since_arguments_parse_deterministically() {
    // 2026-11-28T09:00:00Z
    let now = 1_795_856_400i64;
    assert_eq!(iso_utc(now), "2026-11-28T09:00:00Z");
    assert_eq!(date_utc(now), "2026-11-28");
    assert_eq!(parse_timestamp_to_unix("2026-11-28T09:00:00Z"), Some(now));
    assert_eq!(
        parse_timestamp_to_unix("2026-11-28T11:00:00+0200"),
        Some(now)
    );
    assert_eq!(
        parse_timestamp_to_unix("2026-11-28T11:00:00+02:00"),
        Some(now)
    );
    assert_eq!(parse_since_arg("1h", now), Some(now - 3600));
    assert_eq!(parse_since_arg("-30m", now), Some(now - 1800));
    assert_eq!(parse_since_arg("7d", now), Some(now - 7 * 86_400));
    assert_eq!(parse_since_arg("90s", now), Some(now - 90));
    assert_eq!(
        parse_since_arg("2026-11-28T08:00:00Z", now),
        Some(now - 3600)
    );
    assert_eq!(parse_since_arg("", now), None);
    assert_eq!(parse_since_arg("nonsense", now), None);

    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(256 * 1024 * 1024), "256 MB");
    assert_eq!(format_bytes(15 * 1024 * 1024 * 1024), "15.0 GB");
    assert_eq!(format_bytes(150 * 1024 * 1024 * 1024), "150 GB");
}

#[test]
fn parses_mountinfo_deduplicating_btrfs_subvolumes_and_skipping_pseudo_fs() {
    let mountinfo = "\
26 32 0:24 / /proc rw,nosuid - proc proc rw
29 32 0:26 / /run rw,nosuid - tmpfs run rw
32 2 0:29 /@ / rw,noatime shared:1 - btrfs /dev/nvme1n1p3 rw,subvol=/@
54 32 0:29 /@home /home rw,noatime shared:109 - btrfs /dev/nvme1n1p3 rw,subvol=/@home
62 32 0:48 / /mnt/II rw,noatime shared:117 - btrfs /dev/nvme0n1p3 rw,subvol=/
66 32 259:5 / /boot rw,relatime shared:121 - vfat /dev/nvme1n1p1 rw
459 32 8:49 / /mnt/My\\040Drive rw,nosuid - exfat /dev/sdd1 rw
446 166 0:77 / /run/user/1000/gvfs rw - fuse.gvfsd-fuse gvfsd-fuse rw
broken line
";
    let mounts = parse_mountinfo(mountinfo);
    assert_eq!(mounts.len(), 4);
    assert_eq!(mounts[0].mount_point, "/");
    assert_eq!(mounts[0].device, "/dev/nvme1n1p3");
    assert_eq!(mounts[1].mount_point, "/boot");
    assert_eq!(mounts[2].mount_point, "/mnt/II");
    assert_eq!(mounts[3].mount_point, "/mnt/My Drive");

    let fstab = "\
# comment
UUID=abc / btrfs defaults,x-gvfs-show,x-gvfs-name=I 0 0
UUID=def /mnt/II btrfs defaults,x-gvfs-show,x-gvfs-name=Data\\040II 0 0
";
    let names = parse_fstab_names(fstab);
    assert_eq!(names.get("/").map(String::as_str), Some("I"));
    assert_eq!(names.get("/mnt/II").map(String::as_str), Some("Data II"));

    assert!(parse_mountinfo("").is_empty());
}

#[test]
fn btrfs_metadata_thresholds_match_nov_21_crash_and_allow_unallocated_growth() {
    let mib = 1024 * 1024;
    let gib = 1024 * mib;

    // Like drive IV: 88.5% of current metadata chunk used, but 279 GB unallocated -> ok.
    let (st, _) = evaluate_btrfs_metadata("/mnt/IV", 88.5, 0, 279 * gib, true);
    assert_eq!(st, State::Ok);

    // Low unallocated space AND 92% full -> problem citing Nov 21.
    let (st, reason) = evaluate_btrfs_metadata("/", 92.0, 180 * mib, 4 * mib, false);
    assert_eq!(st, State::Problem);
    assert!(reason.contains("92%"), "{reason}");
    assert!(reason.contains("Nov 21"), "{reason}");

    // Low unallocated space AND 78% full -> watch.
    let (st, _) = evaluate_btrfs_metadata("/", 78.0, 600 * mib, 100 * mib, false);
    assert_eq!(st, State::Watch);

    // Low unallocated space (like drive II) but 4.2 GB metadata free and 32% used -> ok.
    let (st, _) = evaluate_btrfs_metadata("/mnt/II", 32.3, 4 * gib, 4 * mib, false);
    assert_eq!(st, State::Ok);

    // Disk space thresholds
    assert_eq!(evaluate_disk_space("/", 400 * gib, 120 * gib).0, State::Ok);
    assert_eq!(
        evaluate_disk_space("/", 400 * gib, 40 * gib).0,
        State::Watch
    );
    assert_eq!(
        evaluate_disk_space("/", 400 * gib, 4 * gib).0,
        State::Problem
    );
    // Small /boot partition (1 GB total, 600 MB free = 60% free) must be Ok, not Problem.
    assert_eq!(evaluate_disk_space("/boot", gib, 600 * mib).0, State::Ok);
}

#[test]
fn parses_smartctl_and_nvme_cli_json_and_handles_permission_denied() {
    let denied = r#"{
        "smartctl": {
            "messages": [{"string": "Smartctl open device: /dev/nvme0 failed: Permission denied", "severity": "error"}],
            "exit_status": 2
        }
    }"#;
    let p = parse_smartctl_nvme_json(denied).unwrap();
    assert!(p.permission_denied);

    let healthy = r#"{
        "nvme_smart_health_information_log": {
            "critical_warning": 0,
            "temperature": 46,
            "percentage_used": 14,
            "data_units_written": 62500000,
            "media_errors": 0
        }
    }"#;
    let p = parse_smartctl_nvme_json(healthy).unwrap();
    assert!(!p.permission_denied);
    assert_eq!(p.wear_pct, Some(14));
    assert_eq!(p.data_written_bytes, Some(62_500_000 * 512_000));
    assert_eq!(p.temp_c, Some(46));
    assert_eq!(evaluate_nvme("nvme0", true, Some(&p)).0, State::Ok);

    let watch_nvme = r#"{"percent_used": 84, "data_units_written": 1000000, "temperature": 321, "critical_warning": 0, "media_errors": 0}"#;
    let p2 = parse_nvme_cli_json(watch_nvme).unwrap();
    assert_eq!(p2.wear_pct, Some(84));
    assert_eq!(p2.temp_c, Some(48)); // 321 K -> 48 C
    assert_eq!(evaluate_nvme("nvme1", true, Some(&p2)).0, State::Watch);

    let worn = r#"{"nvme_smart_health_information_log": {"critical_warning": 1, "percentage_used": 95, "media_errors": 0}}"#;
    let p3 = parse_smartctl_nvme_json(worn).unwrap();
    assert_eq!(evaluate_nvme("nvme0", true, Some(&p3)).0, State::Problem);

    assert_eq!(parse_smartctl_nvme_json("{broken"), None);
    assert_eq!(parse_nvme_cli_json("{}"), None);
    let (st, reason) = evaluate_nvme("nvme0", false, None);
    assert_eq!(st, State::Ok);
    assert!(reason.contains("needs permission"), "{reason}");
}

#[test]
fn parses_memory_swap_zswap_and_psi_fixtures() {
    let meminfo = "\
MemTotal:       32453216 kB
MemFree:          887576 kB
MemAvailable:   25152092 kB
SwapTotal:      67108860 kB
SwapFree:       66432816 kB
Zswap:             91260 kB
Zswapped:         352568 kB
";
    let m = parse_meminfo(meminfo);
    assert_eq!(m["MemTotal"], 32_453_216 * 1024);
    assert_eq!(m["Zswap"], 91_260 * 1024);
    assert_eq!(m["Zswapped"], 352_568 * 1024);

    let swaps = "\
Filename\t\t\t\tType\t\tSize\t\tUsed\t\tPriority
/dev/nvme1n1p2                          partition\t67108860\t676044\t\t10
";
    let devs = parse_proc_swaps(swaps);
    assert_eq!(devs.len(), 1);
    assert_eq!(devs[0].path, "/dev/nvme1n1p2");
    assert_eq!(devs[0].used_bytes, 676_044 * 1024);
    assert_eq!(devs[0].priority, 10);

    let psi_raw = "\
some avg10=1.25 avg60=12.50 avg300=4.00 total=13143810
full avg10=0.50 avg60=3.10 avg300=1.00 total=12851585
";
    let psi_res = parse_psi_file(psi_raw).unwrap();
    assert_eq!(psi_res.some.as_ref().unwrap().avg60, 12.5);
    assert_eq!(psi_res.full.as_ref().unwrap().avg60, 3.1);
    assert_eq!(psi_res.full.as_ref().unwrap().total_us, 12_851_585);
    assert_eq!(parse_psi_file("garbage\n"), None);

    let psi_info = model::PressureInfo {
        memory: Some(psi_res),
        cpu: None,
        io: None,
    };
    assert_eq!(evaluate_pressure(&psi_info).0, State::Watch);

    let mut mem = model::MemoryInfo {
        ram_total_bytes: 32 * 1024 * 1024 * 1024,
        ram_used_bytes: 30 * 1024 * 1024 * 1024,
        ram_available_bytes: 2 * 1024 * 1024 * 1024,
        ram_used_pct: 93.8,
        swap_total_bytes: 64 * 1024 * 1024 * 1024,
        swap_used_bytes: 40 * 1024 * 1024 * 1024,
        swap_disk_used_bytes: 39 * 1024 * 1024 * 1024,
        swap_used_pct: 62.5,
        zswap_enabled: true,
        zswap_pool_bytes: 256 * 1024 * 1024,
        ..Default::default()
    };
    assert_eq!(evaluate_ram(&mem).0, State::Problem);
    assert_eq!(evaluate_swap(&mem).0, State::Watch);
    mem.swap_used_pct = 91.0;
    assert_eq!(evaluate_swap(&mem).0, State::Problem);
}

#[test]
fn parses_cpu_gpu_systemd_and_updates_thresholds() {
    let t1 = parse_proc_stat("cpu  100 0 50 850 0 0 0 0\n").unwrap();
    let t2 = parse_proc_stat("cpu  200 0 100 1100 0 0 0 0\n").unwrap();
    assert_eq!(collect::cpu_percent(t1, t2), Some(38)); // 150/400 = 37.5 -> 38
    assert_eq!(
        parse_loadavg("0.42 0.31 0.25 1/512 1234\n"),
        Some((0.42, 0.31, 0.25))
    );
    assert_eq!(parse_hwmon_temp("49375\n"), Some(49));
    assert_eq!(evaluate_cpu(Some(15), Some(49), Some(0.5)).0, State::Ok);
    assert_eq!(evaluate_cpu(Some(15), Some(82), Some(0.5)).0, State::Watch);
    assert_eq!(
        evaluate_cpu(Some(15), Some(91), Some(0.5)).0,
        State::Problem
    );

    // Normal AI training: 238 W / 260 W (91.5%), 66 C edge, 81 C junction -> Ok.
    assert_eq!(
        evaluate_gpu(
            Some(99),
            Some(66),
            Some(81),
            Some(238.0),
            Some(260.0),
            Some(91.5)
        )
        .0,
        State::Ok
    );
    assert_eq!(
        evaluate_gpu(
            Some(99),
            Some(82),
            Some(96),
            Some(250.0),
            Some(260.0),
            Some(96.2)
        )
        .0,
        State::Watch
    );
    assert_eq!(
        evaluate_gpu(
            Some(99),
            Some(92),
            Some(108),
            Some(260.0),
            Some(260.0),
            Some(100.0)
        )
        .0,
        State::Problem
    );

    let failed = parse_systemctl_failed(
        "● broken.service loaded failed failed Broken daemon\n  other.timer loaded failed failed Timer\n",
    );
    assert_eq!(failed, vec!["broken.service", "other.timer"]);
    assert!(parse_systemctl_failed("").is_empty());

    let us = parse_update_status(
        "system=12\ninfynitae=2\ncheckupdates=present\nchecked_at=1795850000\n",
    );
    assert_eq!(us.system, Some(12));
    assert_eq!(us.infynitae, Some(2));
    assert_eq!(us.checkupdates, Some(true));
    assert_eq!(evaluate_updates(Some(0), Some(0), true).0, State::Ok);
    assert_eq!(evaluate_updates(Some(2), Some(0), true).0, State::Watch);
    assert_eq!(evaluate_updates(Some(0), Some(30), true).0, State::Watch);
}

#[test]
fn parses_compositor_session_log_and_computes_1h_deltas_across_restarts() {
    let log = "\
[infynitae-session] 2026-11-28 07:00:00 Starting Infynitae compositor...
[infynitae] Latency OutputId(1): input to screen p50 6.00 ms, p95 8.00 ms, max 9.00 ms (50 frames); budget 2.00 ms, margin 1.50 ms, 999 of 9999 frames in a run missed their refresh (0 of 0 after pause); paused 0 times
[infynitae-session] 2026-11-28 08:15:00 Starting Infynitae compositor...
[infynitae] Latency OutputId(1): input to screen p50 7.50 ms, p95 8.75 ms, max 8.91 ms (235 frames); budget 2.11 ms, margin 2.00 ms (direct 1.00, composited 2.00), 75 of 21383 frames in a run missed their refresh (987 of 5277 after pause); paused 0 times
[infynitae] Slow turn 7.0 ms: client requests 7.0 ms
[infynitae] Slow turn 16.9 ms: libinput 16.9 ms
[infynitae] Latency OutputId(1): input to screen p50 11.56 ms, p95 12.15 ms, max 12.18 ms (144 frames); budget 1.63 ms, margin 1.50 ms (direct 1.00, composited 1.50), 86 of 22596 frames in a run missed their refresh (1025 of 6115 after pause); paused 1 times (now)
[infynitae] Slow turn 92.3 ms: screen off 91.2 ms, auto-lock 1.1 ms
";
    let parsed = parse_session_log(log);
    assert_eq!(parsed.session_start.as_deref(), Some("2026-11-28 08:15:00"));
    assert!(parsed.latency_stats_enabled);
    assert_eq!(parsed.missed_frames_total, 86);
    assert_eq!(parsed.frames_total, 22596);
    assert_eq!(parsed.slow_turns_total, 3);
    assert_eq!(parsed.stalls_total, 2); // 16.9 ms and 92.3 ms >= 16 ms
    assert_eq!(parsed.worst_stall_ms, Some(92.3));
    assert_eq!(parsed.gpu_resets_total, 0);

    // 1h delta across a compositor restart: old session +10 missed / +2 stalls, new session +5 missed / +1 stall.
    let c1 = CompositorCounters {
        session_start: Some("s1".into()),
        missed_frames_total: 100,
        frames_total: 10_000,
        stalls_total: 3,
        slow_turns_total: 4,
        gpu_resets_total: 0,
    };
    let c2 = CompositorCounters {
        session_start: Some("s1".into()),
        missed_frames_total: 110,
        frames_total: 12_000,
        stalls_total: 5,
        slow_turns_total: 6,
        gpu_resets_total: 0,
    };
    let cur = CompositorCounters {
        session_start: Some("s2".into()),
        missed_frames_total: 5,
        frames_total: 1_000,
        stalls_total: 1,
        slow_turns_total: 2,
        gpu_resets_total: 0,
    };
    assert_eq!(
        compute_window_deltas(&[&c1, &c2], &cur),
        (15, 3_000, 3, 4, 0)
    );
}

#[test]
fn full_snapshot_on_fixture_tree_orders_problems_first_and_serializes_stable_json() {
    let dir = TestDir::new("full-snap");
    let now = 1_795_856_400i64; // 2026-11-28T09:00:00Z
    let env = fixture_env(dir.path(), now, false);

    // Mountinfo + fstab + btrfs root with critical metadata (92% full, 4 MB unallocated)
    std::fs::write(
        env.proc_dir.join("self/mountinfo"),
        "32 2 0:29 /@ / rw,noatime - btrfs /dev/nvme1n1p3 rw\n",
    )
    .unwrap();
    std::fs::write(
        env.etc_dir.join("fstab"),
        "UUID=1 / btrfs defaults,x-gvfs-show,x-gvfs-name=I 0 0\n",
    )
    .unwrap();
    let btrfs = env.sys_dir.join("fs/btrfs/uuid-root");
    std::fs::create_dir_all(btrfs.join("devices/nvme1n1p3")).unwrap();
    std::fs::create_dir_all(btrfs.join("allocation/data")).unwrap();
    std::fs::create_dir_all(btrfs.join("allocation/metadata")).unwrap();
    std::fs::create_dir_all(btrfs.join("allocation/system")).unwrap();
    // 100 GiB disk, 98 GiB data, ~2 GiB metadata -> ~4 MiB unallocated; metadata 92% used
    let gib = 1024u64 * 1024 * 1024;
    let mib = 1024u64 * 1024;
    std::fs::write(btrfs.join("label"), "I\n").unwrap();
    std::fs::write(
        btrfs.join("devices/nvme1n1p3/size"),
        format!("{}\n", (100 * gib) / 512),
    )
    .unwrap();
    std::fs::write(
        btrfs.join("allocation/data/disk_total"),
        format!("{}\n", 97 * gib + 1000 * mib),
    )
    .unwrap();
    std::fs::write(
        btrfs.join("allocation/metadata/disk_total"),
        format!("{}\n", 2 * gib),
    )
    .unwrap();
    std::fs::write(
        btrfs.join("allocation/system/disk_total"),
        format!("{}\n", 20 * mib),
    )
    .unwrap();
    std::fs::write(
        btrfs.join("allocation/metadata/chunk_size"),
        format!("{gib}\n"),
    )
    .unwrap();
    std::fs::write(
        btrfs.join("allocation/metadata/total_bytes"),
        format!("{}\n", 2 * gib),
    )
    .unwrap();
    std::fs::write(
        btrfs.join("allocation/metadata/bytes_used"),
        format!("{}\n", (2 * gib * 92) / 100),
    )
    .unwrap();
    std::fs::write(
        btrfs.join("allocation/global_rsv_size"),
        format!("{}\n", 100 * mib),
    )
    .unwrap();

    // NVMe unreadable without permission
    let nvme0 = env.sys_dir.join("class/nvme/nvme0");
    std::fs::create_dir_all(&nvme0).unwrap();
    std::fs::write(nvme0.join("model"), "KINGSTON SNV3S1000G\n").unwrap();

    // Memory + PSI + zswap
    std::fs::write(
        env.proc_dir.join("meminfo"),
        "MemTotal: 32000000 kB\nMemAvailable: 20000000 kB\nSwapTotal: 16000000 kB\nSwapFree: 15000000 kB\nZswap: 100000 kB\nZswapped: 400000 kB\n",
    )
    .unwrap();
    std::fs::create_dir_all(env.sys_dir.join("module/zswap/parameters")).unwrap();
    std::fs::write(env.sys_dir.join("module/zswap/parameters/enabled"), "Y\n").unwrap();
    std::fs::write(
        env.sys_dir.join("module/zswap/parameters/compressor"),
        "zstd\n",
    )
    .unwrap();
    std::fs::write(
        env.proc_dir.join("pressure/memory"),
        "some avg10=0.00 avg60=0.10 avg300=0.05 total=100\nfull avg10=0.00 avg60=0.02 avg300=0.01 total=50\n",
    )
    .unwrap();

    // CPU + GPU
    std::fs::write(env.proc_dir.join("stat"), "cpu  500 0 100 1400 0 0 0 0\n").unwrap();
    std::fs::write(env.proc_dir.join("loadavg"), "0.50 0.40 0.30 1/200 99\n").unwrap();
    write_cpu_prev(
        &env.health_dir(),
        CpuPrev {
            timestamp_unix: now - 60,
            idle: 1000,
            total: 1500,
        },
    )
    .unwrap();
    let cpu_hw = env.sys_dir.join("class/hwmon/hwmon0");
    std::fs::create_dir_all(&cpu_hw).unwrap();
    std::fs::write(cpu_hw.join("name"), "k10temp\n").unwrap();
    std::fs::write(cpu_hw.join("temp1_input"), "54000\n").unwrap();

    let gpu_dev = env.sys_dir.join("class/drm/card1/device");
    let gpu_hw = gpu_dev.join("hwmon/hwmon2");
    std::fs::create_dir_all(&gpu_hw).unwrap();
    std::fs::write(gpu_dev.join("gpu_busy_percent"), "80\n").unwrap();
    std::fs::write(gpu_dev.join("mem_info_vram_used"), format!("{}\n", 8 * gib)).unwrap();
    std::fs::write(
        gpu_dev.join("mem_info_vram_total"),
        format!("{}\n", 16 * gib),
    )
    .unwrap();
    std::fs::write(gpu_hw.join("temp1_input"), "66000\n").unwrap();
    std::fs::write(gpu_hw.join("temp1_label"), "edge\n").unwrap();
    std::fs::write(gpu_hw.join("temp2_input"), "82000\n").unwrap();
    std::fs::write(gpu_hw.join("temp2_label"), "junction\n").unwrap();
    std::fs::write(gpu_hw.join("power1_average"), "240000000\n").unwrap();
    std::fs::write(gpu_hw.join("power1_cap"), "260000000\n").unwrap();

    // Systemd fixture: 0 failed system, 0 failed user
    std::fs::write(env.state_dir.join("systemd-failed-system.txt"), "").unwrap();
    std::fs::write(env.state_dir.join("systemd-failed-user.txt"), "").unwrap();

    // Updates: 3 Infynitae commits behind -> Watch
    std::fs::write(
        env.state_dir.join("update-status"),
        format!("system=5\ninfynitae=3\ncheckupdates=present\nchecked_at={now}\n"),
    )
    .unwrap();

    let snap = collect_snapshot_with(&env, &[], |p| {
        if p == Path::new("/") {
            Some((100 * gib, 40 * gib))
        } else {
            None
        }
    });

    assert_eq!(snap.schema_version, SCHEMA_VERSION);
    assert_eq!(snap.state, State::Problem);
    // Problem item must be first, followed by Watch, then Ok.
    assert_eq!(snap.items[0].id, "btrfs:/");
    assert_eq!(snap.items[0].state, State::Problem);
    assert_eq!(snap.items[0].action.as_deref(), Some("disk-fix"));
    assert_eq!(snap.items[1].id, "updates");
    assert_eq!(snap.items[1].state, State::Watch);
    assert_eq!(snap.items[1].action.as_deref(), Some("updates"));

    assert_eq!(snap.nvme.len(), 1);
    assert!(!snap.nvme[0].readable);
    assert_eq!(
        snap.nvme[0].permission_note.as_deref(),
        Some("needs permission")
    );

    let gpu = snap.gpu.as_ref().unwrap();
    assert_eq!(gpu.power_watts, Some(240.0));
    assert_eq!(gpu.power_cap_watts, Some(260.0));
    assert_eq!(gpu.temp_c, Some(66));
    assert_eq!(gpu.junction_temp_c, Some(82));
    assert_eq!(snap.cpu.busy_pct, Some(20)); // +100 busy / +500 total

    // JSON round-trip and plain text output check
    let json = serde_json::to_string(&snap).unwrap();
    let decoded: model::HealthSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.state, State::Problem);
    assert_eq!(decoded.items.len(), snap.items.len());
    let text = render_now_text(&snap);
    assert!(text.contains("Health: 1 problem, 1 to watch"), "{text}");
    assert!(text.contains("PROBLEM"), "{text}");
    assert!(text.contains("Nov 21"), "{text}");
}

#[test]
fn parses_all_log_sources_condensed_newest_first() {
    let dir = TestDir::new("log-sources");
    let now = 1_795_856_400i64; // 2026-11-28T09:00:00Z
    let since = now - 7200;

    // 1. Coredumpctl
    let cd = format!(
        r#"[{{"time":{},"pid":100,"sig":11,"exe":"/usr/bin/firefox"}}]"#,
        (now - 300) * 1_000_000
    );
    let cd_ev = parse_coredumpctl_json(&cd, since, now);
    assert_eq!(cd_ev.len(), 1);
    assert_eq!(cd_ev[0].kind, "crash");
    assert_eq!(cd_ev[0].summary, "firefox crashed (SIGSEGV)");

    // 2. Crash report dir
    let crashes = dir.path().join("crashes");
    std::fs::create_dir_all(&crashes).unwrap();
    std::fs::write(
        crashes.join("2026-11-28_08-30-00.log"),
        "Infynitae crashed (signal ABRT) after 42s\nDate:    2026-11-28T08:30:00Z\nPanic:   assertion failed\n",
    )
    .unwrap();
    let cr_ev = parse_crash_reports_dir(&crashes, since, now);
    assert_eq!(cr_ev.len(), 1);
    assert!(cr_ev[0].summary.contains("assertion failed"));

    // 3. Journal JSONL: OOM, failed unit, disk-guard, GPU reset, USB plug, suspend
    let j_lines = [
        format!(
            r#"{{"__REALTIME_TIMESTAMP":"{}","_SYSTEMD_UNIT":"systemd-oomd.service","MESSAGE":"Killed /user.slice/app-firefox-123.scope due to memory pressure"}}"#,
            (now - 100) * 1_000_000
        ),
        format!(
            r#"{{"__REALTIME_TIMESTAMP":"{}","SYSLOG_IDENTIFIER":"systemd","UNIT":"backup.service","MESSAGE":"Failed with result 'exit-code'."}}"#,
            (now - 200) * 1_000_000
        ),
        format!(
            r#"{{"__REALTIME_TIMESTAMP":"{}","_SYSTEMD_USER_UNIT":"infynitae-disk-guard.service","MESSAGE":"Root drive metadata is nearly full"}}"#,
            (now - 250) * 1_000_000
        ),
        format!(
            r#"{{"__REALTIME_TIMESTAMP":"{}","_TRANSPORT":"kernel","MESSAGE":"amdgpu 0000:03:00.0: amdgpu: GPU reset begin!"}}"#,
            (now - 400) * 1_000_000
        ),
        format!(
            r#"{{"__REALTIME_TIMESTAMP":"{}","_TRANSPORT":"kernel","MESSAGE":"usb 1-2: Product: Kingston DataTraveler"}}"#,
            (now - 500) * 1_000_000
        ),
        format!(
            r#"{{"__REALTIME_TIMESTAMP":"{}","_TRANSPORT":"kernel","MESSAGE":"PM: suspend entry (deep)"}}"#,
            (now - 600) * 1_000_000
        ),
    ]
    .join("\n");
    let j_ev = parse_journal_jsonl(&j_lines, since, now);
    let kinds: Vec<&str> = j_ev.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "oom",
            "unit_failed",
            "disk_guard",
            "gpu_reset",
            "usb_drive",
            "suspend"
        ]
    );

    // 4. Automount + sleep-lock + pacman + install logs
    let auto_ev = parse_automount_log(
        "2026-11-28 08:50:00.100 ignoring /dev/sda1 (already mounted)\n2026-11-28 08:51:00.200 mounted /dev/sdd1 at /mnt/V\n",
        since - 86_400,
        now + 86_400,
    );
    assert_eq!(auto_ev.len(), 1);
    assert_eq!(auto_ev[0].kind, "usb_drive");

    let sl_ev = parse_sleep_lock_log(
        "2026-11-28 08:00:00.000 started (WAYLAND_DISPLAY=wayland-1)\n2026-11-28 08:52:00.000 locking before sleep\n",
        since - 86_400,
        now + 86_400,
    );
    assert_eq!(sl_ev.len(), 1);
    assert_eq!(sl_ev[0].kind, "suspend");

    let pacman = "\
[2026-11-28T08:40:00+0000] [ALPM] transaction started
[2026-11-28T08:40:01+0000] [ALPM] upgraded linux (7.2.14-1 -> 7.2.15-1)
[2026-11-28T08:40:02+0000] [ALPM] upgraded mesa (26.0.1-1 -> 26.0.2-1)
[2026-11-28T08:40:03+0000] [ALPM] transaction completed
";
    let pac_ev = parse_pacman_log(pacman, since, now);
    assert_eq!(pac_ev.len(), 1);
    assert_eq!(pac_ev[0].kind, "update");
    assert_eq!(pac_ev[0].summary, "Upgraded 2 packages (linux, mesa)");

    let inst_ev = parse_install_log(
        "==> 2026-11-28 08:45:00 Updating Infynitae in /home/u/infynitae\n",
        since - 86_400,
        now + 86_400,
    );
    assert_eq!(inst_ev.len(), 1);
    assert_eq!(inst_ev[0].summary, "Infynitae updated");

    // Combined ordering is strictly newest first and deduplicated
    let mut all = Vec::new();
    all.extend(cd_ev);
    all.extend(j_ev.clone());
    all.extend(j_ev); // duplicate batch must collapse
    all.extend(pac_ev);
    let final_ev = finalize_events(all);
    for w in final_ev.windows(2) {
        assert!(w[0].timestamp_unix >= w[1].timestamp_unix);
    }
    assert_eq!(final_ev[0].kind, "oom");

    // End-to-end fixture collect_log
    let env = fixture_env(dir.path(), now, false);
    std::fs::write(env.state_dir.join("fixture-coredumpctl.json"), &cd).unwrap();
    std::fs::write(env.state_dir.join("fixture-journal.jsonl"), &j_lines).unwrap();
    std::fs::write(env.var_log_dir.join("pacman.log"), pacman).unwrap();
    let report = collect_log(&env, since);
    assert_eq!(report.schema_version, SCHEMA_VERSION);
    assert!(report.events.len() >= 8);
    let text = render_log_text(&report);
    assert!(text.contains("OOM killed"), "{text}");
    assert!(text.contains("Upgraded 2 packages"), "{text}");
}

#[test]
fn history_appends_rotates_at_7_days_and_notifies_once_per_problem_transition() {
    let dir = TestDir::new("history-notify");
    let now = 1_795_856_400i64; // 2026-11-28T09:00:00Z
    let env = fixture_env(dir.path(), now, true);
    let hdir = env.health_dir();
    std::fs::create_dir_all(&hdir).unwrap();

    // Seed old JSONL files (8 and 9 days old) + recent file (6 days old) + unrelated file.
    let d_old8 = date_utc(now - 8 * 86_400);
    let d_old7 = date_utc(now - 7 * 86_400);
    let d_keep6 = date_utc(now - 6 * 86_400);
    std::fs::write(hdir.join(format!("{d_old8}.jsonl")), "{}\n").unwrap();
    std::fs::write(hdir.join(format!("{d_old7}.jsonl")), "{}\n").unwrap();
    std::fs::write(hdir.join(format!("{d_keep6}.jsonl")), "{}\n").unwrap();
    std::fs::write(hdir.join("keep-me.txt"), "hi\n").unwrap();

    assert!(parse_jsonl_date(&format!("{d_old8}.jsonl")).is_some());
    assert_eq!(parse_jsonl_date("keep-me.txt"), None);

    let snap = collect_snapshot_with(&env, &[], |_| None);
    let written = append_and_rotate(&hdir, &snap, 7).unwrap();
    assert!(written.is_file());
    assert!(!hdir.join(format!("{d_old8}.jsonl")).exists());
    assert!(!hdir.join(format!("{d_old7}.jsonl")).exists());
    assert!(hdir.join(format!("{d_keep6}.jsonl")).exists());
    assert!(hdir.join("keep-me.txt").exists());
    assert_eq!(rotate_old_files(&hdir, now, 7), 0);

    let loaded = read_samples_since(&hdir, now - 600, now);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].timestamp_unix, now);
    assert_eq!(read_latest_sample(&hdir).unwrap().timestamp_unix, now);

    // Disk warning extraction from history only fires on transition into non-ok.
    let mut bad_snap = snap.clone();
    bad_snap.items.insert(
        0,
        HealthItem {
            id: "btrfs:/".into(),
            category: "disk".into(),
            label: "Btrfs metadata /".into(),
            state: State::Problem,
            summary: "92% used".into(),
            reason: "btrfs metadata on / is 92% full".into(),
            threshold: "".into(),
            action: Some("disk-fix".into()),
        },
    );
    let mut bad_snap2 = bad_snap.clone();
    bad_snap2.timestamp_unix = now + 60;
    let warn_ev = extract_health_disk_warnings(
        &[
            HistorySample::from_snapshot(&snap),
            HistorySample::from_snapshot(&bad_snap),
            HistorySample::from_snapshot(&bad_snap2),
        ],
        now - 60,
        now + 60,
    );
    assert_eq!(warn_ev.len(), 1);
    assert_eq!(warn_ev[0].kind, "disk_guard");

    // Notification transition logic (pure + with stub notify-send binary)
    let mut notified = BTreeSet::new();
    let item_prob = bad_snap.items[0].clone();
    let item_watch = HealthItem {
        id: "updates".into(),
        category: "updates".into(),
        label: "Updates".into(),
        state: State::Watch,
        summary: "1 behind".into(),
        reason: "behind".into(),
        threshold: "".into(),
        action: None,
    };
    let item_ok = HealthItem {
        state: State::Ok,
        ..item_prob.clone()
    };

    // Watch never notifies; Problem notifies once.
    assert_eq!(
        transition_problems(&mut notified, &[item_prob.clone(), item_watch.clone()]).len(),
        1
    );
    // Staying Problem notifies 0 times.
    assert_eq!(
        transition_problems(&mut notified, &[item_prob.clone(), item_watch.clone()]).len(),
        0
    );
    // Returning to Ok clears it; re-entering Problem notifies once again.
    assert_eq!(
        transition_problems(&mut notified, std::slice::from_ref(&item_ok)).len(),
        0
    );
    assert!(notified.is_empty());
    assert_eq!(
        transition_problems(&mut notified, std::slice::from_ref(&item_prob)).len(),
        1
    );

    // End-to-end notify_new_problems using a stub notify-send in temp PATH.
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let stub_log = dir.path().join("notify.log");
    let stub_notify = bin_dir.join("notify-send");
    std::fs::write(
        &stub_notify,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
            stub_log.display()
        ),
    )
    .unwrap();
    let mut perms = std::fs::metadata(&stub_notify).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&stub_notify, perms).unwrap();

    let mut env_n = env.clone();
    env_n.path_env = Some(bin_dir.into_os_string());
    let s1 = notify_new_problems(&env_n, &[item_prob.clone(), item_watch.clone()]);
    assert_eq!(s1, vec!["btrfs:/"]);
    assert!(read_notified_problems(&hdir).contains("btrfs:/"));

    let s2 = notify_new_problems(&env_n, std::slice::from_ref(&item_prob));
    assert!(
        s2.is_empty(),
        "must not repeat notification while still problem"
    );

    let s3 = notify_new_problems(&env_n, &[item_ok]);
    assert!(s3.is_empty());
    assert!(read_notified_problems(&hdir).is_empty());

    let logged = std::fs::read_to_string(&stub_log).unwrap();
    assert_eq!(logged.lines().count(), 1, "{logged}");
    assert!(logged.contains("-u critical"), "{logged}");
    assert!(logged.contains("Btrfs metadata /"), "{logged}");
}
