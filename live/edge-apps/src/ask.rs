//! The `ask` prompt (`infynitae-edge-rs ask [--title-stdin] [--text] [--secret] [--default TEXT] [TITLE]`):
//! a centred card that scripts use to ask the user a choice or a line of text.
//!
//! Copied from live/edge-rs/src/modules/ask.rs, which was ported from `ask_client`, `ask_load`, `draw_ask`, `ask_key` and `ask_answer` in
//! `live/edge/infynitae-edge.c`, drawn in the new module card style.

use crate::card::Rect;

/// What the ask prompt needs of the edge: its state, and a record of what it answered (tests read
/// it). The old edge kept these in its `World`.
#[derive(Default)]
pub struct World {
    pub ask: AskState,
    pub actions: Vec<String>,
}

impl World {
    pub fn now() -> World {
        World::default()
    }
}

fn is_left_button(button: u32) -> bool {
    button == 1 || button == 0x110
}
use crate::draw::{self, Text, PAD_X, PAD_Y};
use crate::theme::{Rgba, Theme};
use cairo::Context;
use std::ffi::CString;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use std::time::Duration;

pub const ASK_FILE: &str = "infynitae-edge-rs.ask";
pub const FEATURES_FILE: &str = "infynitae-edge-rs.features";
pub const ASK_PIPE_PREFIX: &str = "infynitae-edge-rs.ask.";
pub const ASK_CANCEL_FILE: &str = "infynitae-edge-rs.ask-cancel";

pub const ASK_MAX: usize = 12;
pub const MAX_TITLE_BYTES: usize = 159;
pub const MAX_ITEM_BYTES: usize = 159;
pub const MAX_TEXT_BYTES: usize = 255;
pub const MAX_REPLY_BYTES: usize = 255;

pub const CARD_MAX_W: f64 = 460.0;
pub const TITLE_H: f64 = 28.0;
pub const ROW_H: f64 = 32.0;
pub const FIELD_H: f64 = 38.0;
pub const HINT_GAP: f64 = 8.0;
pub const HINT_H: f64 = 18.0;

pub const DEFAULT_RENDER_ASK: &str = "Format Type-C 119.5G (sdf) as|exFAT: every computer, phone and console|FAT32: every device, but no file over 4 GB|ext4: Linux only";

/// Copies a single line from a script up to `\n` or `\r` and at most `max_bytes` bytes, turning
/// ASCII control characters into spaces (matching `ask_copy` in `live/edge/infynitae-edge.c`).
pub fn ask_copy(src: &str, max_bytes: usize) -> String {
    let mut out = String::new();
    for ch in src.chars() {
        if ch == '\n' || ch == '\r' {
            break;
        }
        let mapped = if (ch as u32) < 32 || (ch as u32) == 127 {
            ' '
        } else {
            ch
        };
        if out.len() + mapped.len_utf8() > max_bytes {
            break;
        }
        out.push(mapped);
    }
    out
}

/// Parsed command-line arguments for `infynitae-edge-rs ask`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AskCliArgs {
    pub text: bool,
    pub secret: bool,
    pub title_stdin: bool,
    pub default: String,
    pub title: String,
}

/// Parses the arguments following `ask` on the command line.
pub fn parse_cli_args<S: AsRef<str>>(args: &[S]) -> Result<AskCliArgs, String> {
    let mut text = false;
    let mut secret = false;
    let mut title_stdin = false;
    let mut default = String::new();
    let mut title: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_ref() {
            "--title-stdin" => title_stdin = true,
            "--text" => text = true,
            "--secret" => {
                text = true;
                secret = true;
            }
            "--default" if i + 1 < args.len() => {
                i += 1;
                default = args[i].as_ref().to_string();
            }
            s => title = Some(s.to_string()),
        }
        i += 1;
    }
    if !title_stdin && title.is_none() {
        return Err(
            "usage: infynitae-edge-rs ask [--title-stdin] [--text] [--secret] [--default TEXT] TITLE < choices"
                .to_string(),
        );
    }
    Ok(AskCliArgs {
        text,
        secret,
        title_stdin,
        default,
        title: title.unwrap_or_default(),
    })
}

/// Parses standard input when `--title-stdin` is used.
///
/// The first line is the title (bounded to [`MAX_TITLE_BYTES`] and control characters
/// converted to spaces). Returns `None` if the input has no first line or the title is empty.
/// For choice cards (`!text`), remaining non-empty lines are parsed as choices up to [`ASK_MAX`].
/// For text cards (`text`), no choices are parsed.
pub fn parse_title_stdin(input: &str, text: bool) -> Option<(String, Vec<String>)> {
    let mut lines = input.lines();
    let first = lines.next()?;
    let title = ask_copy(first, MAX_TITLE_BYTES);
    if title.is_empty() {
        return None;
    }
    let mut items = Vec::new();
    if !text {
        for raw in lines {
            let cleaned = ask_copy(raw, MAX_ITEM_BYTES);
            if !cleaned.is_empty() {
                items.push(cleaned);
                if items.len() >= ASK_MAX {
                    break;
                }
            }
        }
    }
    Some((title, items))
}

/// Parses choice lines from `stdin` text, leaving out empty lines and keeping at most [`ASK_MAX`].
pub fn parse_choices(input: &str) -> Vec<String> {
    let mut items = Vec::new();
    for raw in input.lines() {
        let cleaned = ask_copy(raw, MAX_ITEM_BYTES);
        if !cleaned.is_empty() {
            items.push(cleaned);
            if items.len() >= ASK_MAX {
                break;
            }
        }
    }
    items
}

/// Formats the `.ask` request file written by `ask_client`.
pub fn format_request(fifo: &str, cli: &AskCliArgs, items: &[String]) -> String {
    let t = ask_copy(&cli.title, MAX_TITLE_BYTES);
    let st = ask_copy(&cli.default, MAX_TEXT_BYTES);
    let kind = if cli.text { "text" } else { "choose" };
    let sec = if cli.secret { "secret" } else { "plain" };
    let mut out = format!("{fifo}\n{kind}\n{sec}\n{t}\n{st}\n");
    for it in items.iter().take(ASK_MAX) {
        let cleaned = ask_copy(it, MAX_ITEM_BYTES);
        if !cleaned.is_empty() {
            out.push_str(&cleaned);
            out.push('\n');
        }
    }
    out
}

/// Decodes the reply written by the edge to the asker's FIFO (`"1\nANSWER\n"` or `"0\n"`).
/// Returns `Some(Some(answer))` when answered, `Some(None)` when cancelled, or `None` if invalid.
pub fn parse_reply_payload(got: &str) -> Option<Option<String>> {
    if let Some(rest) = got.strip_prefix("1\n") {
        let ans = rest.split(['\n', '\r']).next().unwrap_or("").to_string();
        Some(Some(ans))
    } else if got.starts_with('0') {
        Some(None)
    } else {
        None
    }
}

/// Notifies the edge that this asker is cancelling, writes `ASK_CANCEL_FILE`, and signals the edge.
pub fn tell_edge_cancel(edge_pid: i32, my_pid: u32, fifo_str: &str) {
    let cancel_path = crate::wayland::runtime_path(&format!("{ASK_CANCEL_FILE}.{my_pid}"));
    let target = crate::wayland::runtime_path(ASK_CANCEL_FILE);
    if std::fs::write(&cancel_path, format!("{fifo_str}\n{my_pid}\n")).is_ok() {
        let _ = std::fs::rename(&cancel_path, &target);
    }
    unsafe {
        libc::kill(edge_pid, libc::SIGRTMIN() + 3);
    }
}

/// Runs `infynitae-edge-rs ask ...`: sends the question to the running edge via a FIFO in
/// `$XDG_RUNTIME_DIR` and `SIGRTMIN+3`, prints the answer, and returns the exit code
/// (`0` answered, `1` cancelled, `2` bad question, `3` no edge running).
pub fn run_client<S: AsRef<str>>(args: &[S]) -> u8 {
    let mut cli = match parse_cli_args(args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            return 2;
        }
    };
    let Some(pid) = crate::wayland::running_pid() else {
        return 3;
    };
    let mut items = Vec::new();
    if cli.title_stdin {
        let mut raw = String::new();
        let _ = std::io::stdin().read_to_string(&mut raw);
        let Some((title, parsed_items)) = parse_title_stdin(&raw, cli.text) else {
            return 1;
        };
        cli.title = title;
        items = parsed_items;
        if !cli.text && items.is_empty() {
            return 2;
        }
    } else if !cli.text {
        let mut raw = String::new();
        let _ = std::io::stdin().read_to_string(&mut raw);
        items = parse_choices(&raw);
        if items.is_empty() {
            return 2;
        }
    }

    let my_pid = std::process::id();
    let fifo_path = crate::wayland::runtime_path(&format!("{ASK_PIPE_PREFIX}{my_pid}"));
    let _ = std::fs::remove_file(&fifo_path);
    let Ok(fifo_c) = CString::new(fifo_path.as_os_str().as_bytes()) else {
        return 3;
    };
    if unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) } != 0 {
        return 3;
    }
    let fd = unsafe {
        libc::open(
            fifo_c.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let _ = std::fs::remove_file(&fifo_path);
        return 3;
    }

    // Self-pipe for signals: SIGTERM or SIGINT wakes poll to cancel and notify edge
    let mut sig_pipe = [-1i32; 2];
    let pipe_created =
        unsafe { libc::pipe2(sig_pipe.as_mut_ptr(), libc::O_NONBLOCK | libc::O_CLOEXEC) == 0 };
    let (sig_rd, sig_wr) = if pipe_created {
        (sig_pipe[0], sig_pipe[1])
    } else {
        (-1, -1)
    };

    let term_hook = if sig_wr >= 0 {
        let wr = sig_wr;
        unsafe {
            signal_hook_registry::register(libc::SIGTERM, move || {
                let _ = libc::write(wr, [1u8].as_ptr().cast(), 1);
            })
            .ok()
        }
    } else {
        None
    };
    let int_hook = if sig_wr >= 0 {
        let wr = sig_wr;
        unsafe {
            signal_hook_registry::register(libc::SIGINT, move || {
                let _ = libc::write(wr, [1u8].as_ptr().cast(), 1);
            })
            .ok()
        }
    } else {
        None
    };

    let req_path = crate::wayland::runtime_path(ASK_FILE);
    for _ in 0..40 {
        if !req_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let tmp_path = crate::wayland::runtime_path(&format!("{ASK_FILE}-new.{my_pid}"));
    let fifo_str = fifo_path.to_string_lossy();
    let content = format_request(&fifo_str, &cli, &items);
    if std::fs::write(&tmp_path, content).is_err() {
        if let Some(h) = term_hook {
            signal_hook_registry::unregister(h);
        }
        if let Some(h) = int_hook {
            signal_hook_registry::unregister(h);
        }
        if sig_rd >= 0 {
            unsafe { libc::close(sig_rd) };
        }
        if sig_wr >= 0 {
            unsafe { libc::close(sig_wr) };
        }
        unsafe {
            libc::close(fd);
        }
        let _ = std::fs::remove_file(&fifo_path);
        return 3;
    }
    if std::fs::rename(&tmp_path, &req_path).is_err()
        || unsafe { libc::kill(pid, libc::SIGRTMIN() + 3) } != 0
    {
        let _ = std::fs::remove_file(&tmp_path);
        if let Some(h) = term_hook {
            signal_hook_registry::unregister(h);
        }
        if let Some(h) = int_hook {
            signal_hook_registry::unregister(h);
        }
        if sig_rd >= 0 {
            unsafe { libc::close(sig_rd) };
        }
        if sig_wr >= 0 {
            unsafe { libc::close(sig_wr) };
        }
        unsafe {
            libc::close(fd);
        }
        let _ = std::fs::remove_file(&fifo_path);
        return 3;
    }

    let mut got = [0u8; 400];
    let mut len = 0usize;
    let mut rc = 3u8;
    loop {
        let mut pfds = [
            libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: sig_rd,
                events: if sig_rd >= 0 { libc::POLLIN } else { 0 },
                revents: 0,
            },
        ];
        let nfds = if sig_rd >= 0 { 2 } else { 1 };
        let r = unsafe { libc::poll(pfds.as_mut_ptr(), nfds, 1000) };
        if r < 0 {
            let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if err == libc::EINTR {
                continue;
            }
            break;
        }
        if sig_rd >= 0 && (pfds[1].revents & libc::POLLIN) != 0 {
            tell_edge_cancel(pid, my_pid, &fifo_str);
            rc = 1;
            break;
        }
        if r == 0 {
            if unsafe { libc::kill(pid, 0) } != 0 {
                break;
            }
            continue;
        }
        let max_read = (got.len() - 1).saturating_sub(len);
        let k = unsafe { libc::read(fd, got[len..].as_mut_ptr().cast(), max_read) };
        if k > 0 {
            len += k as usize;
            if len < got.len() - 1 {
                continue;
            }
        }
        if k < 0 {
            let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if err == libc::EAGAIN || err == libc::EWOULDBLOCK || err == libc::EINTR {
                continue;
            }
        }
        let text_got = String::from_utf8_lossy(&got[..len]);
        match parse_reply_payload(&text_got) {
            Some(Some(ans)) => {
                println!("{ans}");
                rc = 0;
            }
            Some(None) => {
                rc = 1;
            }
            None => {
                rc = 3;
            }
        }
        break;
    }

    if let Some(h) = term_hook {
        signal_hook_registry::unregister(h);
    }
    if let Some(h) = int_hook {
        signal_hook_registry::unregister(h);
    }
    if sig_rd >= 0 {
        unsafe { libc::close(sig_rd) };
    }
    if sig_wr >= 0 {
        unsafe { libc::close(sig_wr) };
    }
    unsafe {
        libc::close(fd);
    }
    let _ = std::fs::remove_file(&fifo_path);
    rc
}

/// Parsed contents of `$XDG_RUNTIME_DIR/infynitae-edge-rs.ask`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskRequest {
    pub reply: String,
    pub text: bool,
    pub secret: bool,
    pub title: String,
    pub start: String,
    pub items: Vec<String>,
    pub pid: Option<i32>,
    pub conn_fd: Option<i32>,
}

impl AskRequest {
    pub fn close_fd(&mut self) {
        if let Some(fd) = self.conn_fd.take() {
            if fd >= 0 {
                unsafe { libc::close(fd) };
            }
        }
    }
}

/// Extracts the asker's PID from the reply FIFO path (which ends in `.<pid>`), or `None`.
pub fn parse_pid_from_reply(reply: &str) -> Option<i32> {
    reply.rsplit('.').next()?.parse::<i32>().ok()
}

/// Checks whether the asker is gone: first by polling its connection file descriptor for
/// hangup/error/EOF (e.g. closed reader on FIFO or socket pair), falling back to checking
/// whether the process is still alive with `kill(pid, 0)` if no connection file descriptor is held.
pub fn is_asker_gone(conn_fd: Option<i32>, pid: Option<i32>) -> bool {
    if let Some(fd) = conn_fd {
        if fd >= 0 {
            let mut pfd = libc::pollfd {
                fd,
                events: libc::POLLIN | libc::POLLERR | libc::POLLHUP,
                revents: 0,
            };
            let r = unsafe { libc::poll(&mut pfd, 1, 0) };
            if r > 0 {
                if (pfd.revents & (libc::POLLERR | libc::POLLHUP)) != 0 {
                    return true;
                }
                if (pfd.revents & libc::POLLIN) != 0 {
                    let mut b = [0u8; 1];
                    let k = unsafe {
                        libc::recv(
                            fd,
                            b.as_mut_ptr().cast(),
                            1,
                            libc::MSG_PEEK | libc::MSG_DONTWAIT,
                        )
                    };
                    if k == 0 {
                        return true;
                    }
                }
            }
            return false;
        }
    }
    if let Some(p) = pid {
        if p > 0 && unsafe { libc::kill(p, 0) } != 0 {
            let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if err == libc::ESRCH {
                return true;
            }
        }
    }
    false
}

/// Parses the contents of an `.ask` request file.
pub fn parse_request(raw: &str) -> AskRequest {
    let mut lines = raw.split('\n');
    let reply = ask_copy(lines.next().unwrap_or(""), MAX_REPLY_BYTES);
    let kind = ask_copy(lines.next().unwrap_or(""), 15);
    let secret_s = ask_copy(lines.next().unwrap_or(""), 15);
    let title_raw = ask_copy(lines.next().unwrap_or(""), MAX_TITLE_BYTES);
    let start_raw = ask_copy(lines.next().unwrap_or(""), MAX_TEXT_BYTES);

    let text = kind == "text";
    let secret = text && secret_s == "secret";
    let title = if title_raw.is_empty() {
        "Choose".to_string()
    } else {
        title_raw
    };
    let start = if text { start_raw } else { String::new() };

    let mut items = Vec::new();
    for line in lines {
        if items.len() >= ASK_MAX {
            break;
        }
        let item = ask_copy(line, MAX_ITEM_BYTES);
        if !item.is_empty() {
            items.push(item);
        }
    }

    let pid = parse_pid_from_reply(&reply);

    AskRequest {
        reply,
        text,
        secret,
        title,
        start,
        items,
        pid,
        conn_fd: None,
    }
}

/// Verifies that `reply` is a FIFO or socket owned by the current user under `expected_prefix` without `/..`
/// (matching the checks in `ask_load` in `live/edge/infynitae-edge.c`).
pub fn valid_reply_fifo_with_prefix(reply: &str, expected_prefix: &str, expected_uid: u32) -> bool {
    if expected_prefix.is_empty() || !reply.starts_with(expected_prefix) || reply.contains("/..") {
        return false;
    }
    let Ok(meta) = std::fs::symlink_metadata(reply) else {
        return false;
    };
    (meta.file_type().is_fifo() || meta.file_type().is_socket()) && meta.uid() == expected_uid
}

/// Active question state inside the edge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AskState {
    pub on: bool,
    pub text: bool,
    pub secret: bool,
    pub title: String,
    pub items: Vec<String>,
    pub sel: usize,
    /// where the pointer was last reported over the question (rounded): the selection follows the
    /// pointer only when it moves. The compositor also reports a pointer that did not move, which
    /// put the selection back under a resting mouse right after an arrow key.
    pub hover_at: Option<(i32, i32)>,
    pub buf: String,
    pub reply: String,
    pub conn_fd: Option<i32>,
    pub pid: Option<i32>,
    pub queue: Vec<AskRequest>,
}

impl AskState {
    pub fn clear_active(&mut self) {
        if let Some(fd) = self.conn_fd.take() {
            if fd >= 0 {
                unsafe { libc::close(fd) };
            }
        }
        self.reply.clear();
        self.title.clear();
        self.items.clear();
        self.buf.clear();
        self.sel = 0;
        self.hover_at = None;
        self.pid = None;
    }

    pub fn activate_request(&mut self, mut req: AskRequest, fd: Option<i32>) {
        self.clear_active();
        self.reply = req.reply;
        self.text = req.text;
        self.secret = req.secret;
        self.title = req.title;
        self.buf = req.start;
        self.items = req.items;
        self.sel = 0;
        self.pid = req.pid;
        self.conn_fd = fd.or(req.conn_fd.take());
        self.on = true;
    }

    pub fn advance_queue(&mut self) -> bool {
        self.clear_active();
        while !self.queue.is_empty() {
            let mut next = self.queue.remove(0);
            if is_asker_gone(next.conn_fd, next.pid) {
                next.close_fd();
                continue;
            }
            let fd = next.conn_fd.take();
            self.activate_request(next, fd);
            return true;
        }
        self.on = false;
        false
    }

    pub fn push_request(&mut self, mut req: AskRequest, fd: Option<i32>) -> bool {
        let conn = fd.or(req.conn_fd.take());
        req.conn_fd = conn;
        if is_asker_gone(req.conn_fd, req.pid) {
            req.close_fd();
            return false;
        }
        if !self.on {
            let fd = req.conn_fd.take();
            self.activate_request(req, fd);
            true
        } else {
            self.queue.push(req);
            true
        }
    }
    /// Parses an `INFYNITAE_EDGE_RENDER_ASK` specification (`"Title|choice1|choice2"`,
    /// `"text:Title"`, or `"secret:Title"`), with optional initial text from
    /// `INFYNITAE_EDGE_RENDER_SEARCH` and selected index from `INFYNITAE_EDGE_RENDER_SELECT`.
    pub fn from_render_spec(
        spec: &str,
        search_env: Option<&str>,
        select_env: Option<&str>,
    ) -> Self {
        let (text, secret, rest) = if let Some(r) = spec.strip_prefix("text:") {
            (true, false, r)
        } else if let Some(r) = spec.strip_prefix("secret:") {
            (true, true, r)
        } else {
            (false, false, spec)
        };
        let mut parts = rest.split('|');
        let title = ask_copy(parts.next().unwrap_or(""), MAX_TITLE_BYTES);
        let mut items = Vec::new();
        for part in parts.take(ASK_MAX) {
            items.push(ask_copy(part, MAX_ITEM_BYTES));
        }
        let buf = if let Some(s) = search_env {
            ask_copy(s, MAX_TEXT_BYTES)
        } else if text && !items.is_empty() {
            items.remove(0)
        } else {
            String::new()
        };
        let sel_raw = select_env
            .and_then(|s| s.trim().parse::<isize>().ok())
            .unwrap_or(0)
            .max(0) as usize;
        let sel = if items.is_empty() {
            0
        } else {
            sel_raw.min(items.len() - 1)
        };
        Self {
            on: true,
            text,
            secret,
            title,
            items,
            sel,
            hover_at: None,
            buf,
            reply: String::new(),
            conn_fd: None,
            pid: None,
            queue: Vec::new(),
        }
    }

    /// Populates `self` from `INFYNITAE_EDGE_RENDER_ASK`, `INFYNITAE_EDGE_RENDER_SEARCH`, and
    /// `INFYNITAE_EDGE_RENDER_SELECT`.
    pub fn load_render_env(&mut self) {
        let spec = std::env::var("INFYNITAE_EDGE_RENDER_ASK")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_RENDER_ASK.to_string());
        let search = std::env::var("INFYNITAE_EDGE_RENDER_SEARCH").ok();
        let select = std::env::var("INFYNITAE_EDGE_RENDER_SELECT").ok();
        *self = Self::from_render_spec(&spec, search.as_deref(), select.as_deref());
    }

    /// Returns the card's `(x, y, w, h)` rectangle on a screen of size `w x h`.
    /// Width is `min(460, screen - 32)`, vertically centred near a third of the screen height.
    pub fn geometry(&self, w: f64, h: f64) -> Rect {
        let card_w = (w - 32.0).clamp(0.0, CARD_MAX_W);
        let body_h = if self.text {
            FIELD_H + HINT_GAP + HINT_H
        } else {
            (self.items.len() as f64) * ROW_H
        };
        let card_h = 2.0 * PAD_Y + TITLE_H + body_h;
        let gx = ((w - card_w) / 2.0).round();
        let gy = (h * 0.3 - card_h / 2.0).max(16.0).round();
        (gx, gy, card_w, card_h)
    }

    /// Returns the choice row index at `(x, y)` on a screen of size `w x h`, or `None`.
    pub fn row_at(&self, w: f64, h: f64, x: f64, y: f64) -> Option<usize> {
        if self.text || self.items.is_empty() {
            return None;
        }
        let (gx, gy, gw, _) = self.geometry(w, h);
        let top = gy + PAD_Y + TITLE_H;
        if x < gx + PAD_X - 8.0 || x > gx + gw - (PAD_X - 8.0) || y < top {
            return None;
        }
        let idx = ((y - top) / ROW_H).floor() as isize;
        if idx >= 0 && (idx as usize) < self.items.len() {
            Some(idx as usize)
        } else {
            None
        }
    }
}

pub fn apply(ask: &mut AskState, key: &str, out: &str) -> bool {
    if key != "ask" {
        return false;
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        return false;
    }
    *ask = AskState::from_render_spec(trimmed, None, None);
    true
}

/// Sends `ans` (`None` when cancelled) through `world.ask.reply` or `world.ask.conn_fd`, records the
/// outcome in `world.actions`, and advances to the next queued card if any.
pub fn answer(world: &mut World, ans: Option<&str>) {
    let mut sent = false;
    if let Some(fd) = world.ask.conn_fd.take() {
        if fd >= 0 {
            let buf = match ans {
                Some(a) => format!("1\n{a}\n"),
                None => "0\n".to_string(),
            };
            let bytes = buf.as_bytes();
            let n = bytes.len().min(319);
            unsafe {
                let _ = libc::write(fd, bytes.as_ptr().cast(), n);
                libc::close(fd);
            }
            sent = true;
        }
    }
    if !sent && !world.ask.reply.is_empty() {
        if let Ok(c_path) = CString::new(world.ask.reply.as_bytes()) {
            let fd = unsafe {
                libc::open(
                    c_path.as_ptr(),
                    libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd >= 0 {
                let buf = match ans {
                    Some(a) => format!("1\n{a}\n"),
                    None => "0\n".to_string(),
                };
                let bytes = buf.as_bytes();
                let n = bytes.len().min(319);
                unsafe {
                    let _ = libc::write(fd, bytes.as_ptr().cast(), n);
                    libc::close(fd);
                }
            }
        }
    }
    match ans {
        Some(a) => world.actions.push(format!("ask:answer:{a}")),
        None => world.actions.push("ask:cancel".to_string()),
    }
    world.ask.advance_queue();
}

/// Closes the card if its asker is gone, popping the next queued card if available.
/// Also prunes dead requests from the queue. Returns true if state changed.
pub fn check_asker_gone(world: &mut World) -> bool {
    let mut changed = false;
    world.ask.queue.retain_mut(|req| {
        if is_asker_gone(req.conn_fd, req.pid) {
            req.close_fd();
            changed = true;
            false
        } else {
            true
        }
    });

    if world.ask.on && is_asker_gone(world.ask.conn_fd, world.ask.pid) {
        if let Some(fd) = world.ask.conn_fd.take() {
            if fd >= 0 {
                unsafe { libc::close(fd) };
            }
        }
        world.actions.push("ask:cancel".to_string());
        world.ask.advance_queue();
        return true;
    }
    changed
}

/// Cancels the ask request matching `fifo` or `pid` (either active or in queue).
pub fn cancel_request(world: &mut World, fifo: &str, pid: Option<i32>) -> bool {
    let mut changed = false;
    world.ask.queue.retain_mut(|req| {
        let matches = (!fifo.is_empty() && req.reply == fifo) || (pid.is_some() && req.pid == pid);
        if matches {
            req.close_fd();
            changed = true;
            false
        } else {
            true
        }
    });

    if world.ask.on {
        let matches = (!fifo.is_empty() && world.ask.reply == fifo)
            || (pid.is_some() && world.ask.pid == pid);
        if matches {
            if let Some(fd) = world.ask.conn_fd.take() {
                if fd >= 0 {
                    unsafe { libc::close(fd) };
                }
            }
            world.actions.push("ask:cancel".to_string());
            world.ask.advance_queue();
            return true;
        }
    }
    changed
}

/// Reads a cancel request from `cancel_path` and removes the matching card.
pub fn handle_cancel_from_path(world: &mut World, cancel_path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(cancel_path) else {
        return false;
    };
    let _ = std::fs::remove_file(cancel_path);
    let mut lines = raw.lines();
    let fifo = lines.next().unwrap_or("").trim();
    let pid = lines.next().and_then(|s| s.trim().parse::<i32>().ok());
    cancel_request(world, fifo, pid)
}

/// Loads a waiting `.ask` request file into `world.ask`, validating its reply FIFO with
/// `expected_prefix` and `expected_uid` (matching `ask_load` in `live/edge/infynitae-edge.c`).
pub fn load_from_path_with_prefix(
    world: &mut World,
    req_path: &Path,
    expected_prefix: &str,
    expected_uid: u32,
) -> bool {
    let Ok(raw) = std::fs::read_to_string(req_path) else {
        return false;
    };
    let _ = std::fs::remove_file(req_path);
    let mut req = parse_request(&raw);
    if !valid_reply_fifo_with_prefix(&req.reply, expected_prefix, expected_uid) {
        return false;
    }
    req.pid = parse_pid_from_reply(&req.reply);
    let fd = if let Ok(c_path) = CString::new(req.reply.as_bytes()) {
        let raw_fd = unsafe {
            libc::open(
                c_path.as_ptr(),
                libc::O_WRONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        if raw_fd >= 0 {
            Some(raw_fd)
        } else {
            return false;
        }
    } else {
        return false;
    };
    if !req.text && req.items.is_empty() {
        if let Some(f) = fd {
            unsafe { libc::close(f) };
        }
        return false;
    }
    world.ask.push_request(req, fd)
}

/// Loads `$XDG_RUNTIME_DIR/infynitae-edge-rs.ask` into `world.ask`.
pub fn load_from_path(world: &mut World, req_path: &Path) -> bool {
    let prefix = crate::wayland::runtime_path(ASK_PIPE_PREFIX)
        .to_string_lossy()
        .into_owned();
    let uid = unsafe { libc::getuid() };
    load_from_path_with_prefix(world, req_path, &prefix, uid)
}

/// Handles a key press while `world.ask.on` is true (matching `ask_key` in C).
pub fn key(world: &mut World, keysym: u32, ch: Option<char>) -> bool {
    if !world.ask.on {
        return false;
    }
    if keysym == 0xff1b {
        answer(world, None);
        return true;
    }
    if keysym == 0xff0d || keysym == 0xff8d {
        if world.ask.text {
            if !world.ask.buf.is_empty() {
                let ans = world.ask.buf.clone();
                answer(world, Some(&ans));
                return true;
            }
            return false;
        }
        if let Some(ans) = world.ask.items.get(world.ask.sel).cloned() {
            answer(world, Some(&ans));
            return true;
        }
        return false;
    }
    if !world.ask.text {
        let n = world.ask.items.len();
        if n == 0 {
            return false;
        }
        match keysym {
            0xff52 | 0xff51 | 0xfe20 => {
                world.ask.sel = (world.ask.sel + n - 1) % n;
                return true;
            }
            0xff54 | 0xff53 | 0xff09 => {
                world.ask.sel = (world.ask.sel + 1) % n;
                return true;
            }
            0xff50 => {
                world.ask.sel = 0;
                return true;
            }
            0xff57 => {
                world.ask.sel = n - 1;
                return true;
            }
            _ => return false,
        }
    }
    if keysym == 0xff08 {
        return world.ask.buf.pop().is_some();
    }
    if let Some(c) = ch.filter(|&c| !c.is_control() && (c as u32) >= 32 && (c as u32) != 127) {
        if world.ask.buf.len() + c.len_utf8() <= MAX_TEXT_BYTES {
            world.ask.buf.push(c);
            return true;
        }
    }
    false
}

/// Updates the selected choice row when the pointer moves over it.
pub fn hover(world: &mut World, w: f64, h: f64, x: f64, y: f64) -> bool {
    if !world.ask.on {
        return false;
    }
    let at = (x.round() as i32, y.round() as i32);
    // the first report is where the mouse rests as the question appears, and one that did not
    // move is no choice either: only a pointer that moves picks a row
    match world.ask.hover_at.replace(at) {
        Some(before) if before != at => {}
        _ => return false,
    }
    if let Some(row) = world.ask.row_at(w, h, x, y) {
        if row != world.ask.sel {
            world.ask.sel = row;
            return true;
        }
    }
    false
}

/// Handles a pointer click while `world.ask.on` is true: left-clicking a row answers with that
/// choice; clicking outside the card cancels.
pub fn click(world: &mut World, w: f64, h: f64, x: f64, y: f64, button: u32) -> bool {
    if !world.ask.on {
        return false;
    }
    if let Some(row) = world.ask.row_at(w, h, x, y) {
        if is_left_button(button) {
            let ans = world.ask.items[row].clone();
            answer(world, Some(&ans));
            return true;
        }
        return false;
    }
    let (gx, gy, gw, gh) = world.ask.geometry(w, h);
    if x < gx || x > gx + gw || y < gy || y > gy + gh {
        answer(world, None);
        return true;
    }
    false
}

/// Draws the ask prompt card on a screen of size `screen = (w, h)` and returns its `(x, y, w, h)`
/// rectangle.
pub fn draw(cr: &Context, th: &Theme, ask: &AskState, screen: (f64, f64)) -> Rect {
    let (gx, gy, gw, gh) = ask.geometry(screen.0, screen.1);
    if gw <= 0.0 || gh <= 0.0 {
        return (gx, gy, gw, gh);
    }
    draw::card(cr, th, gx, gy, gw, gh);
    draw::text_max(
        cr,
        gx + PAD_X,
        gy + PAD_Y,
        (gw - 2.0 * PAD_X).max(10.0),
        &Text {
            size: 15.0,
            bold: true,
            color: th.text,
            text: &ask.title,
        },
    );

    let top = gy + PAD_Y + TITLE_H;
    if ask.text {
        let fx = gx + PAD_X;
        let fw = (gw - 2.0 * PAD_X).max(20.0);
        let r = th.small_radius().max(4.0);
        draw::round_rect(cr, fx.round(), top.round(), fw.round(), FIELD_H.round(), r);
        draw::set(cr, th.track);
        let _ = cr.fill();
        draw::round_rect(
            cr,
            fx.round() + 0.5,
            top.round() + 0.5,
            fw.round() - 1.0,
            FIELD_H.round() - 1.0,
            (r - 0.5).max(0.0),
        );
        draw::set(cr, Rgba(th.accent.0, th.accent.1, th.accent.2, 0.65));
        cr.set_line_width(1.0);
        let _ = cr.stroke();

        let shown = if ask.secret {
            "\u{2022}".repeat(ask.buf.chars().count())
        } else {
            ask.buf.clone()
        };
        let cy = top + FIELD_H / 2.0;
        let tx = fx + 12.0;
        let max_w = (fw - 28.0).max(10.0);
        let tw = if !shown.is_empty() {
            draw::text_max(
                cr,
                tx,
                cy - 8.0,
                max_w,
                &Text {
                    size: 13.0,
                    bold: false,
                    color: th.text,
                    text: &shown,
                },
            )
        } else {
            0.0
        };
        let caret_x = (tx
            + if shown.is_empty() {
                0.0
            } else {
                tw.min(max_w) + 2.0
            })
        .round();
        draw::set(cr, th.accent);
        cr.rectangle(caret_x, (cy - 7.0).round(), 1.5, 14.0);
        let _ = cr.fill();

        draw::text_max(
            cr,
            gx + PAD_X,
            top + FIELD_H + HINT_GAP,
            (gw - 2.0 * PAD_X).max(20.0),
            &Text {
                size: 12.0,
                bold: false,
                color: th.muted,
                text: "Enter to confirm, Esc to cancel",
            },
        );
    } else {
        let r = th.small_radius().max(4.0);
        for (i, item) in ask.items.iter().enumerate() {
            let ry = top + (i as f64) * ROW_H;
            let is_sel = i == ask.sel;
            if is_sel {
                draw::round_rect(
                    cr,
                    gx + PAD_X - 8.0,
                    ry + 1.0,
                    (gw - 2.0 * (PAD_X - 8.0)).max(0.0),
                    ROW_H - 2.0,
                    r,
                );
                draw::set(cr, Rgba(th.accent.0, th.accent.1, th.accent.2, 0.18));
                let _ = cr.fill();
            }
            draw::text_max(
                cr,
                gx + PAD_X,
                ry + (ROW_H - 16.0) / 2.0,
                (gw - 2.0 * PAD_X).max(20.0),
                &Text {
                    size: 13.0,
                    bold: is_sel,
                    color: if is_sel { th.text } else { th.text.alpha(0.86) },
                    text: item,
                },
            );
        }
    }

    if let Ok(boxes_path) = std::env::var("INFYNITAE_EDGE_BOXES") {
        if !boxes_path.is_empty() {
            let _ = std::fs::write(boxes_path, format!("ask {gx:.0} {gy:.0} {gw:.0} {gh:.0}\n"));
        }
    }

    (gx, gy, gw, gh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::FromRawFd;

    #[test]
    fn title_stdin_parsing_handles_choices_secret_and_empty_input() {
        // 1. title line + choices
        let input =
            "Format the drive as\nexFAT: every computer\nFAT32: every device\n\next4: Linux only\n";
        let (title, choices) = parse_title_stdin(input, false).unwrap();
        assert_eq!(title, "Format the drive as");
        assert_eq!(
            choices,
            vec![
                "exFAT: every computer",
                "FAT32: every device",
                "ext4: Linux only"
            ]
        );

        // Bounded title length (MAX_TITLE_BYTES = 159)
        let long_title = "T".repeat(200);
        let input_long = format!("{long_title}\nChoice A\nChoice B\n");
        let (title_bounded, choices_bounded) = parse_title_stdin(&input_long, false).unwrap();
        assert_eq!(title_bounded.len(), MAX_TITLE_BYTES);
        assert_eq!(title_bounded, "T".repeat(MAX_TITLE_BYTES));
        assert_eq!(choices_bounded, vec!["Choice A", "Choice B"]);

        // 2. title only for --secret
        let secret_input = "Passphrase for USB\nextra line ignored for text\n";
        let (sec_title, sec_choices) = parse_title_stdin(secret_input, true).unwrap();
        assert_eq!(sec_title, "Passphrase for USB");
        assert!(sec_choices.is_empty());

        let secret_single_line = "Passphrase for USB";
        let (sec_title2, sec_choices2) = parse_title_stdin(secret_single_line, true).unwrap();
        assert_eq!(sec_title2, "Passphrase for USB");
        assert!(sec_choices2.is_empty());

        // 3. empty input
        assert_eq!(parse_title_stdin("", false), None);
        assert_eq!(parse_title_stdin("", true), None);
        assert_eq!(parse_title_stdin("\n", false), None);
        assert_eq!(parse_title_stdin("\r\n", false), None);
        assert_eq!(parse_title_stdin("\n", true), None);
    }

    #[test]
    fn cli_args_parsing_with_title_stdin() {
        let cli = parse_cli_args(&["--title-stdin", "--secret"]).unwrap();
        assert!(cli.title_stdin);
        assert!(cli.text);
        assert!(cli.secret);
        assert_eq!(cli.title, "");

        let cli_choice = parse_cli_args(&["--title-stdin"]).unwrap();
        assert!(cli_choice.title_stdin);
        assert!(!cli_choice.text);
        assert!(!cli_choice.secret);
        assert_eq!(cli_choice.title, "");

        let cli_default = parse_cli_args(&["--title-stdin", "--default", "hint"]).unwrap();
        assert!(cli_default.title_stdin);
        assert_eq!(cli_default.default, "hint");

        // Missing title and no --title-stdin fails
        assert!(parse_cli_args(&["--secret"]).is_err());
        assert!(parse_cli_args::<&str>(&[]).is_err());
    }

    #[test]
    fn cli_args_and_request_formatting_match_c_edge() {
        let cli = parse_cli_args(&["--secret", "--default", "abc", "Passphrase for USB"]).unwrap();
        assert!(cli.text);
        assert!(cli.secret);
        assert_eq!(cli.default, "abc");
        assert_eq!(cli.title, "Passphrase for USB");

        let req_text = format_request("/run/user/1000/infynitae-edge-rs.ask.42", &cli, &[]);
        assert_eq!(
            req_text,
            "/run/user/1000/infynitae-edge-rs.ask.42\ntext\nsecret\nPassphrase for USB\nabc\n"
        );

        let cli_choice = parse_cli_args(&["Format the drive as\nextra line"]).unwrap();
        assert!(!cli_choice.text);
        assert!(!cli_choice.secret);
        let choices = parse_choices(
            "exFAT: every computer, phone and console\nFAT32: every device\n\next4: Linux only\n",
        );
        assert_eq!(
            choices,
            vec![
                "exFAT: every computer, phone and console",
                "FAT32: every device",
                "ext4: Linux only"
            ]
        );
        let req_choice = format_request(
            "/run/user/1000/infynitae-edge-rs.ask.99",
            &cli_choice,
            &choices,
        );
        let parsed = parse_request(&req_choice);
        assert_eq!(parsed.reply, "/run/user/1000/infynitae-edge-rs.ask.99");
        assert!(!parsed.text);
        assert!(!parsed.secret);
        assert_eq!(parsed.title, "Format the drive as");
        assert_eq!(parsed.start, "");
        assert_eq!(parsed.items, choices);
    }

    #[test]
    fn reply_payload_parsing_handles_answer_and_cancel() {
        assert_eq!(
            parse_reply_payload("1\nFAT32: every device\n"),
            Some(Some("FAT32: every device".to_string()))
        );
        assert_eq!(parse_reply_payload("0\n"), Some(None));
        assert_eq!(parse_reply_payload(""), None);
    }

    #[test]
    fn load_from_path_validates_fifo_and_answers_over_pipe() {
        let dir =
            std::env::temp_dir().join(format!("infynitae-edge-rs-ask-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let prefix = dir.join(ASK_PIPE_PREFIX).to_string_lossy().into_owned();
        let fifo_path = dir.join(format!("{ASK_PIPE_PREFIX}1234"));
        let fifo_c = CString::new(fifo_path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        let rfd = unsafe {
            libc::open(
                fifo_c.as_ptr(),
                libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        assert!(rfd >= 0);
        let mut reader = unsafe { std::fs::File::from_raw_fd(rfd) };

        let req_path = dir.join(ASK_FILE);
        let cli = parse_cli_args(&["Erase everything?"]).unwrap();
        let items = vec!["Cancel".to_string(), "Erase everything".to_string()];
        std::fs::write(
            &req_path,
            format_request(&fifo_path.to_string_lossy(), &cli, &items),
        )
        .unwrap();

        let mut world = World::now();
        let uid = unsafe { libc::getuid() };
        assert!(load_from_path_with_prefix(
            &mut world, &req_path, &prefix, uid
        ));
        assert!(!req_path.exists());
        assert!(world.ask.on);
        assert_eq!(world.ask.title, "Erase everything?");
        assert_eq!(world.ask.items, items);

        // Reject a regular file (not a FIFO) or a path with /..
        let bad_file = dir.join(format!("{ASK_PIPE_PREFIX}bad"));
        std::fs::write(&bad_file, "").unwrap();
        assert!(!valid_reply_fifo_with_prefix(
            &bad_file.to_string_lossy(),
            &prefix,
            uid
        ));
        assert!(!valid_reply_fifo_with_prefix(
            &format!("{prefix}../evil"),
            &prefix,
            uid
        ));

        // Select second choice via Down arrow and press Enter -> writes "1\nErase everything\n" to FIFO
        assert!(key(&mut world, 0xff54, None));
        assert_eq!(world.ask.sel, 1);
        assert!(key(&mut world, 0xff0d, None));
        assert!(!world.ask.on);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("ask:answer:Erase everything")
        );

        let mut got = String::new();
        reader.read_to_string(&mut got).unwrap();
        assert_eq!(got, "1\nErase everything\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keyboard_and_mouse_interactions_for_choices_and_text() {
        let mut world = World::now();
        world.ask = AskState::from_render_spec("Format the drive as|exFAT|FAT32|ext4", None, None);
        let (w, h) = (1920.0, 1080.0);
        let (gx, gy, gw, gh) = world.ask.geometry(w, h);
        assert_eq!(gw, 460.0);
        assert_eq!(gx, 730.0);
        assert!(gy < 540.0 && gy + gh < 540.0);

        // End jumps to last item, Down wraps to 0, Up wraps to last, Home jumps to 0, Tab advances
        assert!(key(&mut world, 0xff57, None));
        assert_eq!(world.ask.sel, 2);
        assert!(key(&mut world, 0xff54, None));
        assert_eq!(world.ask.sel, 0);
        assert!(key(&mut world, 0xff52, None));
        assert_eq!(world.ask.sel, 2);
        assert!(key(&mut world, 0xff50, None));
        assert_eq!(world.ask.sel, 0);
        assert!(key(&mut world, 0xff09, None));
        assert_eq!(world.ask.sel, 1);
        assert!(key(&mut world, 0xfe20, None));
        assert_eq!(world.ask.sel, 0);

        // The mouse resting where the question appeared picks nothing, nor does a report of it
        // that did not move; moving onto row 2 selects row 2; left-click on row 1 answers FAT32
        let row2_y = gy + PAD_Y + TITLE_H + ROW_H * 2.5;
        assert!(!hover(&mut world, w, h, gx + gw / 2.0, row2_y));
        assert!(!hover(&mut world, w, h, gx + gw / 2.0, row2_y));
        assert_eq!(world.ask.sel, 0);
        assert!(hover(&mut world, w, h, gx + gw / 2.0 + 4.0, row2_y));
        assert_eq!(world.ask.sel, 2);
        let row1_y = gy + PAD_Y + TITLE_H + ROW_H * 1.5;
        assert!(click(&mut world, w, h, gx + gw / 2.0, row1_y, 1));
        assert!(!world.ask.on);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("ask:answer:FAT32")
        );

        // Text mode: typing, multibyte Backspace by character, Enter confirms, click outside cancels
        world.ask = AskState::from_render_spec("text:Name for the drive", Some("U"), None);
        assert!(key(&mut world, 'S' as u32, Some('S')));
        assert!(key(&mut world, 'é' as u32, Some('é')));
        assert_eq!(world.ask.buf, "USé");
        // Backspace removes the 2-byte 'é' character as one char
        assert!(key(&mut world, 0xff08, None));
        assert_eq!(world.ask.buf, "US");
        assert!(key(&mut world, 'B' as u32, Some('B')));
        assert_eq!(world.ask.buf, "USB");
        assert!(key(&mut world, 0xff0d, None));
        assert!(!world.ask.on);
        assert_eq!(
            world.actions.last().map(String::as_str),
            Some("ask:answer:USB")
        );

        // Click outside cancels
        world.ask = AskState::from_render_spec("secret:Passphrase", Some("secret"), None);
        assert!(click(&mut world, w, h, 20.0, 20.0, 1));
        assert!(!world.ask.on);
        assert_eq!(world.actions.last().map(String::as_str), Some("ask:cancel"));
    }

    fn create_socket_pair() -> (i32, i32) {
        let mut fds = [0i32; 2];
        assert_eq!(
            unsafe {
                libc::socketpair(
                    libc::AF_UNIX,
                    libc::SOCK_STREAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                    0,
                    fds.as_mut_ptr(),
                )
            },
            0
        );
        (fds[0], fds[1])
    }

    #[test]
    fn closing_asker_socket_end_removes_card() {
        let (asker, edge) = create_socket_pair();
        let mut world = World::now();
        let req = AskRequest {
            title: "Question 1".to_string(),
            items: vec!["Yes".to_string(), "No".to_string()],
            ..Default::default()
        };
        assert!(world.ask.push_request(req, Some(edge)));
        assert!(world.ask.on);
        assert_eq!(world.ask.title, "Question 1");

        // Before closing asker end, check_asker_gone reports false
        assert!(!check_asker_gone(&mut world));
        assert!(world.ask.on);

        // Close asker end -> check_asker_gone reports true and removes card
        unsafe { libc::close(asker) };
        assert!(check_asker_gone(&mut world));
        assert!(!world.ask.on);
    }

    #[test]
    fn asker_gone_removes_card_and_shows_next_queued_card() {
        let (asker1, edge1) = create_socket_pair();
        let (asker2, edge2) = create_socket_pair();
        let mut world = World::now();

        let req1 = AskRequest {
            title: "Question 1".to_string(),
            items: vec!["Choice 1A".to_string(), "Choice 1B".to_string()],
            ..Default::default()
        };
        let req2 = AskRequest {
            title: "Question 2".to_string(),
            items: vec!["Choice 2A".to_string(), "Choice 2B".to_string()],
            ..Default::default()
        };

        assert!(world.ask.push_request(req1, Some(edge1)));
        assert!(world.ask.on);
        assert_eq!(world.ask.title, "Question 1");
        assert_eq!(world.ask.queue.len(), 0);

        // Second card queues behind the first
        assert!(world.ask.push_request(req2, Some(edge2)));
        assert!(world.ask.on);
        assert_eq!(world.ask.title, "Question 1");
        assert_eq!(world.ask.queue.len(), 1);
        assert_eq!(world.ask.queue[0].title, "Question 2");

        // Asker 1 closes its end (e.g. killed by SIGTERM or SIGKILL)
        unsafe { libc::close(asker1) };
        assert!(check_asker_gone(&mut world));

        // Card 1 removed, Card 2 now shows!
        assert!(world.ask.on);
        assert_eq!(world.ask.title, "Question 2");
        assert_eq!(world.ask.queue.len(), 0);

        // Asker 2 closes its end
        unsafe { libc::close(asker2) };
        assert!(check_asker_gone(&mut world));

        // Card 2 removed, no cards left
        assert!(!world.ask.on);
    }

    #[test]
    fn queued_ask_files_and_fifo_removal_advances_queue() {
        let dir = std::env::temp_dir().join(format!(
            "infynitae-edge-rs-ask-qtest-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let prefix = dir.join(ASK_PIPE_PREFIX).to_string_lossy().into_owned();
        let uid = unsafe { libc::getuid() };

        let fifo1_path = dir.join(format!("{ASK_PIPE_PREFIX}1001"));
        let fifo1_c = CString::new(fifo1_path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo1_c.as_ptr(), 0o600) }, 0);
        let rfd1 = unsafe {
            libc::open(
                fifo1_c.as_ptr(),
                libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        assert!(rfd1 >= 0);

        let fifo2_path = dir.join(format!("{ASK_PIPE_PREFIX}1002"));
        let fifo2_c = CString::new(fifo2_path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo2_c.as_ptr(), 0o600) }, 0);
        let rfd2 = unsafe {
            libc::open(
                fifo2_c.as_ptr(),
                libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
            )
        };
        assert!(rfd2 >= 0);
        let mut reader2 = unsafe { std::fs::File::from_raw_fd(rfd2) };

        let req1_path = dir.join(format!("{ASK_FILE}.1"));
        let cli1 = parse_cli_args(&["Question 1"]).unwrap();
        let items1 = vec!["A1".to_string(), "B1".to_string()];
        std::fs::write(
            &req1_path,
            format_request(&fifo1_path.to_string_lossy(), &cli1, &items1),
        )
        .unwrap();

        let req2_path = dir.join(format!("{ASK_FILE}.2"));
        let cli2 = parse_cli_args(&["Question 2"]).unwrap();
        let items2 = vec!["A2".to_string(), "B2".to_string()];
        std::fs::write(
            &req2_path,
            format_request(&fifo2_path.to_string_lossy(), &cli2, &items2),
        )
        .unwrap();

        let mut world = World::now();
        assert!(load_from_path_with_prefix(
            &mut world, &req1_path, &prefix, uid
        ));
        assert!(world.ask.on);
        assert_eq!(world.ask.title, "Question 1");

        assert!(load_from_path_with_prefix(
            &mut world, &req2_path, &prefix, uid
        ));
        assert_eq!(world.ask.queue.len(), 1);
        assert_eq!(world.ask.title, "Question 1");

        // Asker 1 killed (close rfd1)
        unsafe { libc::close(rfd1) };
        assert!(check_asker_gone(&mut world));

        // Question 2 shows!
        assert!(world.ask.on);
        assert_eq!(world.ask.title, "Question 2");

        // Answer Question 2
        assert!(key(&mut world, 0xff0d, None));
        assert!(!world.ask.on);

        let mut got = String::new();
        reader2.read_to_string(&mut got).unwrap();
        assert_eq!(got, "1\nA2\n");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cancel_request_removes_active_and_queued_cards() {
        let (asker1, edge1) = create_socket_pair();
        let (asker2, edge2) = create_socket_pair();
        let mut world = World::now();

        let req1 = AskRequest {
            reply: "/tmp/fake.101".to_string(),
            title: "Q1".to_string(),
            pid: Some(101),
            ..Default::default()
        };
        let req2 = AskRequest {
            reply: "/tmp/fake.102".to_string(),
            title: "Q2".to_string(),
            pid: Some(102),
            ..Default::default()
        };

        world.ask.push_request(req1, Some(edge1));
        world.ask.push_request(req2, Some(edge2));
        assert_eq!(world.ask.title, "Q1");
        assert_eq!(world.ask.queue.len(), 1);

        // Cancel Q1 by pid
        assert!(cancel_request(&mut world, "", Some(101)));
        // Q1 removed, Q2 shows
        assert!(world.ask.on);
        assert_eq!(world.ask.title, "Q2");

        // Cancel Q2 by fifo
        assert!(cancel_request(&mut world, "/tmp/fake.102", None));
        assert!(!world.ask.on);

        unsafe {
            libc::close(asker1);
            libc::close(asker2);
        }
    }
}
