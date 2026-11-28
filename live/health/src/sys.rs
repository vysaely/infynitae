//! Environment paths and bounded subprocess execution so tests point at fixture folders without
//! reading real `/proc`, `/sys`, or host logs.

use std::ffi::{CString, OsStr, OsString};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const MAX_CMD_OUTPUT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct SysEnv {
    pub proc_dir: PathBuf,
    pub sys_dir: PathBuf,
    pub etc_dir: PathBuf,
    pub dev_dir: PathBuf,
    pub var_log_dir: PathBuf,
    pub amdgpu_ids: PathBuf,
    pub state_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub config_dir: PathBuf,
    pub home_dir: Option<PathBuf>,
    pub path_env: Option<OsString>,
    pub now_unix: i64,
    pub sample_mode: bool,
    pub run_commands: bool,
}

impl SysEnv {
    pub fn system(sample_mode: bool) -> Self {
        let home_dir = std::env::var_os("HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let state_base = std::env::var_os("XDG_STATE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir.as_ref().map(|h| h.join(".local/state")))
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let config_base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| home_dir.as_ref().map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let runtime_base = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        let now_unix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);

        Self {
            proc_dir: PathBuf::from("/proc"),
            sys_dir: PathBuf::from("/sys"),
            etc_dir: PathBuf::from("/etc"),
            dev_dir: PathBuf::from("/dev"),
            var_log_dir: PathBuf::from("/var/log"),
            amdgpu_ids: PathBuf::from("/usr/share/libdrm/amdgpu.ids"),
            state_dir: state_base.join("infynitae"),
            runtime_dir: runtime_base.join("infynitae"),
            config_dir: config_base.join("infynitae"),
            home_dir,
            path_env: std::env::var_os("PATH"),
            now_unix,
            sample_mode,
            run_commands: true,
        }
    }

    pub fn health_dir(&self) -> PathBuf {
        self.state_dir.join("health")
    }

    pub fn resolve_checkout(&self) -> Option<PathBuf> {
        let checkout_file = self.config_dir.join("checkout");
        if let Ok(raw) = std::fs::read_to_string(checkout_file) {
            if let Some(line) = raw
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with('#'))
            {
                return Some(expand_home(line, self.home_dir.as_deref()));
            }
        }
        self.home_dir.as_ref().map(|h| h.join("infynitae"))
    }

    pub fn program_on_path(&self, name: &str) -> Option<PathBuf> {
        let path_env = self.path_env.as_deref()?;
        find_program_in_path(name, path_env)
    }
}

pub fn expand_home(raw: &str, home: Option<&Path>) -> PathBuf {
    let trimmed = raw.trim();
    let Some(base) = home.filter(|h| !h.as_os_str().is_empty()) else {
        return PathBuf::from(trimmed);
    };
    for prefix in ["$HOME", "${HOME}", "~"] {
        if trimmed == prefix {
            return base.to_path_buf();
        }
        if let Some(rest) = trimmed
            .strip_prefix(prefix)
            .and_then(|s| s.strip_prefix('/'))
        {
            return base.join(rest);
        }
    }
    PathBuf::from(trimmed)
}

pub fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
}

pub fn find_program_in_path(name: &str, path_env: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_env)
        .filter(|d| !d.as_os_str().is_empty())
        .map(|d| d.join(name))
        .find(|p| is_executable(p))
}

/// Calls `statvfs` on `path`, returning `(total_bytes, available_bytes)`.
pub fn statvfs_sizes(path: &Path) -> Option<(u64, u64)> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let res = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if res != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    let frsize = if stat.f_frsize > 0 {
        stat.f_frsize
    } else {
        stat.f_bsize
    };
    Some((stat.f_blocks * frsize, stat.f_bavail * frsize))
}

/// Reads at most the trailing `max_bytes` of `path`, skipping a partial leading line when cut.
pub fn read_file_tail(path: &Path, max_bytes: usize) -> std::io::Result<String> {
    use std::io::{Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let skip_first = len > max_bytes as u64;
    if skip_first {
        file.seek(SeekFrom::End(-(max_bytes as i64)))?;
    }
    let mut buf = Vec::with_capacity(len.min(max_bytes as u64) as usize);
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    if skip_first {
        if let Some((_, rest)) = text.split_once('\n') {
            return Ok(rest.to_string());
        }
    }
    Ok(text.into_owned())
}

#[derive(Clone, Debug)]
pub struct CmdOutput {
    pub status: Option<i32>,
    pub stdout: String,
}

fn kill_process_group(child: &mut Child) {
    let pgid = child.id() as libc::pid_t;
    if pgid > 0 {
        unsafe {
            libc::kill(-pgid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs `prog` directly with `args` (`LC_ALL=C`, no shell) bounded by `limit`.
pub fn run_cmd(prog: &OsStr, args: &[&str], limit: Duration) -> Option<CmdOutput> {
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd.process_group(0);

    let mut child = cmd.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout
            .by_ref()
            .take(MAX_CMD_OUTPUT as u64)
            .read_to_end(&mut buf);
        let _ = tx.send(buf);
    });

    let deadline = Instant::now() + limit;
    match rx.recv_timeout(limit) {
        Ok(buf) => loop {
            match child.try_wait() {
                Ok(Some(st)) => {
                    return Some(CmdOutput {
                        status: st.code(),
                        stdout: String::from_utf8_lossy(&buf).into_owned(),
                    });
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2));
                }
                _ => {
                    kill_process_group(&mut child);
                    return None;
                }
            }
        },
        Err(_) => {
            kill_process_group(&mut child);
            None
        }
    }
}
