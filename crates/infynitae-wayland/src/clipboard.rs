//! The compositor's own clipboard (owner 2026-11-22: our own in place of wl-clip-persist and
//! cliphist, four processes fewer).
//!
//! When an app copies, the text types and one image type it offers are read into memory at once:
//! a source can no longer be read once its app has closed. When that source goes, the compositor
//! offers the copy itself (`CLIPBOARD_PERSIST`). Each copy also enters a history kept in memory
//! only (`CLIPBOARD_HISTORY`): newest first, at most [`HISTORY_ENTRIES`] entries and
//! [`HISTORY_BYTES`]; it outlives an edge restart and ends with the compositor. A copy the app
//! marks for a password manager ([`SECRET_HINT`]) or one made in a private window
//! (`PRIVACY_APPS`, `PRIVACY_TITLES`, Super+Shift+P) is never read: not kept, not in the history.
//!
//! Pipes are read and written without blocking, after each client dispatch ([`Clipboard::pump`])
//! and whenever the session's loop sees one ready ([`Clipboard::watch`]), so a slow or stuck app
//! never holds up the compositor.

use std::collections::VecDeque;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex, MutexGuard};

use smithay::reexports::rustix;
use smithay::reexports::rustix::io::Errno;

use crate::WaylandState;
use crate::log_line;

/// The most one copy may hold, all its types together; a larger copy is not kept.
pub const COPY_BYTES: usize = 32 << 20;
/// The most the history holds, all entries together.
pub const HISTORY_BYTES: usize = 64 << 20;
/// The most entries the history holds.
pub const HISTORY_ENTRIES: usize = 300;
/// The type KeePassXC and other password managers offer beside a password (KDE's convention):
/// such a copy stays out of the compositor's memory.
pub const SECRET_HINT: &str = "x-kde-passwordManagerHint";
/// The characters of a text entry the history listing gives.
const PREVIEW_CHARS: usize = 200;

/// One copy: the bytes of each type kept, in the order the app offered them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contents(pub Vec<(String, Arc<[u8]>)>);

impl Contents {
    pub fn mime_types(&self) -> Vec<String> {
        self.0.iter().map(|(m, _)| m.clone()).collect()
    }

    pub fn get(&self, mime: &str) -> Option<Arc<[u8]>> {
        self.0
            .iter()
            .find(|(m, _)| m == mime)
            .map(|(_, b)| b.clone())
    }

    fn bytes(&self) -> usize {
        self.0.iter().map(|(_, b)| b.len()).sum()
    }

    /// What the history shows and compares: the image if there is one, else the text.
    fn main(&self) -> Option<(&str, &[u8])> {
        let find = |f: &dyn Fn(&str) -> bool| {
            self.0
                .iter()
                .find(|(m, _)| f(m))
                .map(|(m, b)| (m.as_str(), &b[..]))
        };
        find(&|m| m.starts_with("image/"))
            .or_else(|| find(&|m| m == "text/plain;charset=utf-8"))
            .or_else(|| find(&|m| m == "UTF8_STRING"))
            .or_else(|| find(&|m| m == "text/plain"))
            .or_else(|| find(&is_text))
    }
}

fn is_text(mime: &str) -> bool {
    mime.starts_with("text/") || matches!(mime, "UTF8_STRING" | "STRING" | "TEXT")
}

/// The types of an offer to read: every text type and one image type (PNG when offered; an app
/// converting one picture to a dozen formats is read once). `None` for a password manager's copy.
pub fn types_to_read(offered: &[String]) -> Option<Vec<String>> {
    if offered.iter().any(|m| m == SECRET_HINT) {
        return None;
    }
    let image = offered
        .iter()
        .find(|m| *m == "image/png")
        .or_else(|| offered.iter().find(|m| m.starts_with("image/")));
    Some(
        offered
            .iter()
            .filter(|m| is_text(m) || Some(*m) == image)
            .cloned()
            .collect(),
    )
}

/// A history entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u64,
    pub pinned: bool,
    pub contents: Contents,
}

/// What the history listing says of an entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preview {
    /// Up to [`PREVIEW_CHARS`] characters of the text.
    Text(String),
    /// The image's type, its size in bytes and, for a PNG, its width and height.
    Image {
        mime: String,
        bytes: usize,
        size: Option<(u32, u32)>,
    },
}

/// One line of the history listing, newest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub id: u64,
    pub pinned: bool,
    pub preview: Preview,
}

/// A PNG's width and height from its header.
fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[..8] != b"\x89PNG\r\n\x1a\n" || &b[12..16] != b"IHDR" {
        return None;
    }
    let be = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    Some((be(16), be(20)))
}

/// The copies, newest first, within [`HISTORY_ENTRIES`] and [`HISTORY_BYTES`].
#[derive(Debug, Default)]
pub struct History {
    entries: VecDeque<Entry>,
    next_id: u64,
}

impl History {
    /// A new copy goes first; one with the same text or image as an older entry replaces it (and
    /// keeps its pin). Then the oldest unpinned entries go until the bounds hold, and if pinned
    /// ones alone are over them, the oldest pinned too.
    pub fn add(&mut self, contents: Contents) {
        let Some(main) = contents.main() else {
            return;
        };
        let pinned = match self
            .entries
            .iter()
            .position(|e| e.contents.main() == Some(main))
        {
            Some(i) => self.entries.remove(i).is_some_and(|e| e.pinned),
            None => false,
        };
        self.next_id += 1;
        self.entries.push_front(Entry {
            id: self.next_id,
            pinned,
            contents,
        });
        while self.entries.len() > HISTORY_ENTRIES || self.bytes() > HISTORY_BYTES {
            let i = self
                .entries
                .iter()
                .rposition(|e| !e.pinned)
                .unwrap_or(self.entries.len() - 1);
            self.entries.remove(i);
        }
    }

    pub fn bytes(&self) -> usize {
        self.entries.iter().map(|e| e.contents.bytes()).sum()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The entry `id` moved to the front (it is the clipboard again), and its contents.
    pub fn take_to_front(&mut self, id: u64) -> Option<Contents> {
        let i = self.entries.iter().position(|e| e.id == id)?;
        let e = self.entries.remove(i)?;
        let contents = e.contents.clone();
        self.entries.push_front(e);
        Some(contents)
    }

    pub fn pin(&mut self, id: u64, on: bool) -> bool {
        match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) => {
                e.pinned = on;
                true
            }
            None => false,
        }
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    /// Newest first, pinned entries before the others.
    pub fn listing(&self) -> Vec<Listed> {
        let pinned = self.entries.iter().filter(|e| e.pinned);
        let rest = self.entries.iter().filter(|e| !e.pinned);
        pinned
            .chain(rest)
            .filter_map(|e| {
                let (mime, bytes) = e.contents.main()?;
                let preview = if mime.starts_with("image/") {
                    Preview::Image {
                        mime: mime.to_string(),
                        bytes: bytes.len(),
                        size: png_size(bytes),
                    }
                } else {
                    Preview::Text(
                        String::from_utf8_lossy(bytes)
                            .chars()
                            .take(PREVIEW_CHARS)
                            .collect(),
                    )
                };
                Some(Listed {
                    id: e.id,
                    pinned: e.pinned,
                    preview,
                })
            })
            .collect()
    }
}

/// One pipe the compositor reads from an app or writes to one, without blocking.
#[derive(Debug)]
pub enum Transfer {
    /// A copy's type, read from the app that copied. `over`: it passed [`COPY_BYTES`] or failed.
    Read {
        fd: Option<OwnedFd>,
        bytes: Vec<u8>,
        over: bool,
    },
    /// A paste, written to the app that pastes.
    Write {
        fd: Option<OwnedFd>,
        data: Arc<[u8]>,
        at: usize,
    },
}

/// A pipe the state and the session's loop both move (the state must be `Send`, C7.4).
pub type Shared = Arc<Mutex<Transfer>>;

/// The transfer of `t`; a panic elsewhere while it was held leaves bytes that are still bytes.
pub fn lock(t: &Shared) -> MutexGuard<'_, Transfer> {
    t.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Transfer {
    /// Moves what the pipe takes or gives now. True once finished: the end of the pipe, an
    /// error, everything written, or the limit passed; the pipe is then closed.
    pub fn pump(&mut self) -> bool {
        let mut buf = [0u8; 16 * 1024];
        match self {
            Transfer::Read { fd, bytes, over } => loop {
                let Some(f) = fd.as_ref() else {
                    return true;
                };
                match rustix::io::read(f, &mut buf) {
                    Ok(0) => *fd = None,
                    Ok(n) if bytes.len() + n > COPY_BYTES => {
                        *over = true;
                        *fd = None;
                    }
                    Ok(n) => bytes.extend_from_slice(&buf[..n]),
                    Err(Errno::INTR) => {}
                    Err(Errno::AGAIN) => return false,
                    Err(_) => {
                        *over = true;
                        *fd = None;
                    }
                }
            },
            Transfer::Write { fd, data, at } => loop {
                let Some(f) = fd.as_ref() else {
                    return true;
                };
                if *at >= data.len() {
                    *fd = None;
                    continue;
                }
                match rustix::io::write(f, &data[*at..]) {
                    Ok(n) => *at += n,
                    Err(Errno::INTR) => {}
                    Err(Errno::AGAIN) => return false,
                    // the app closed its end: it wants no more
                    Err(_) => *fd = None,
                }
            },
        }
    }

    pub fn finished(&self) -> bool {
        match self {
            Transfer::Read { fd, .. } | Transfer::Write { fd, .. } => fd.is_none(),
        }
    }
}

/// What a shell asks of the clipboard history (`infynitae msg clipboard …`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardAsk {
    /// The entry becomes the clipboard again.
    Use(u64),
    Pin(u64, bool),
    Delete(u64),
}

/// The clipboard's state in the compositor.
#[derive(Debug)]
pub struct Clipboard {
    /// `CLIPBOARD_PERSIST`: the copy stays offered after its app closed.
    pub persist: bool,
    /// `CLIPBOARD_HISTORY`: copies enter the history.
    pub history_on: bool,
    /// The types of a new selection to read at the end of the dispatch: Smithay calls
    /// `new_selection` before the selection is the seat's, so it cannot be read there.
    due: Option<Vec<String>>,
    /// The current selection's types being read.
    reading: Vec<(String, Shared)>,
    /// Its app's source went before the reading finished: the copy is offered once read.
    orphaned: bool,
    /// An app set the clipboard to nothing (the X11 bridge, when the X11 app that copied quits):
    /// handled at the end of the dispatch, since Smithay clears the seat's selection only after
    /// `new_selection` returns.
    cleared: bool,
    /// The current selection's copy, read.
    held: Option<Contents>,
    pub history: History,
    /// Pastes being written.
    sends: Vec<Shared>,
    /// Pipes for the session's loop to watch (a copy of the fd each; `true`: for writing).
    pub watch: Vec<(OwnedFd, Shared, bool)>,
    /// The history changed since the shell was last given its listing.
    listing_changed: bool,
}

impl Default for Clipboard {
    fn default() -> Self {
        Clipboard {
            persist: true,
            history_on: true,
            due: None,
            reading: Vec::new(),
            orphaned: false,
            cleared: false,
            held: None,
            history: History::default(),
            sends: Vec::new(),
            watch: Vec::new(),
            // the shell's first listing (an empty one) goes out at the first turn
            listing_changed: true,
        }
    }
}

impl Clipboard {
    /// Reads and writes what the pipes allow now; drops the finished ones.
    pub fn pump(&mut self) {
        for (_, t) in &self.reading {
            lock(t).pump();
        }
        self.sends.retain(|t| !lock(t).pump());
    }

    /// The history listing when it changed since the last call.
    pub fn take_listing(&mut self) -> Option<Vec<Listed>> {
        std::mem::take(&mut self.listing_changed).then(|| self.history.listing())
    }

    /// `CLIPBOARD_PERSIST` and `CLIPBOARD_HISTORY` as the config says now; the history goes
    /// when it is turned off.
    pub fn configure(&mut self, persist: bool, history_on: bool) {
        self.persist = persist;
        self.history_on = history_on;
        if !history_on && !self.history.is_empty() {
            self.history.clear();
            self.listing_changed = true;
        }
    }

    fn watch_pipe(&mut self, fd: &OwnedFd, t: &Shared, write: bool) {
        match rustix::io::dup(fd) {
            Ok(copy) => self.watch.push((copy, t.clone(), write)),
            Err(e) => log_line!("[infynitae] Clipboard pipe not watched: {e}"),
        }
    }
}

/// `fd` without blocking.
fn nonblocking(fd: &OwnedFd) -> rustix::io::Result<()> {
    let flags = rustix::fs::fcntl_getfl(fd)?;
    rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)
}

impl WaylandState {
    /// A client set the clipboard (`offered`: its types) or cleared it (`None`). `private`: it
    /// was copied in a private window.
    pub(crate) fn clipboard_new_selection(&mut self, offered: Option<Vec<String>>, private: bool) {
        // an app set the clipboard to nothing: the X11 bridge does so when the X11 app that copied
        // quits. Kept like a copy whose app went (clipboard_orphaned): the compositor offers it.
        if offered.is_none() && self.clipboard.persist {
            self.clipboard.cleared = true;
            return;
        }
        self.clipboard.cleared = false;
        let c = &mut self.clipboard;
        c.reading.clear();
        c.orphaned = false;
        c.held = None;
        c.due = offered
            .filter(|_| !private && (c.persist || c.history_on))
            .and_then(|o| types_to_read(&o))
            .filter(|t| !t.is_empty());
    }

    /// After each client dispatch: the reads of a new selection start, the pipes move, and a
    /// finished copy is kept (and offered, when its source went meanwhile).
    pub(crate) fn clipboard_turn(&mut self) {
        // a cleared clipboard is kept like one whose app went (the copy read so far is offered)
        if std::mem::take(&mut self.clipboard.cleared) {
            self.clipboard_orphaned();
        }
        if let Some(types) = self.clipboard.due.take() {
            for mime in types {
                let pipe = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC)
                    .map_err(std::io::Error::from)
                    .and_then(|(r, w)| nonblocking(&r).map(|_| (r, w)).map_err(Into::into));
                let (read, write) = match pipe {
                    Ok(p) => p,
                    Err(e) => {
                        log_line!("[infynitae] Clipboard not read: {e}");
                        self.clipboard.reading.clear();
                        break;
                    }
                };
                if smithay::wayland::selection::data_device::request_data_device_client_selection(
                    &self.seat,
                    mime.clone(),
                    write,
                )
                .is_err()
                {
                    continue;
                }
                let t: Shared = Arc::new(Mutex::new(Transfer::Read {
                    fd: None,
                    bytes: Vec::new(),
                    over: false,
                }));
                self.clipboard.watch_pipe(&read, &t, false);
                if let Transfer::Read { fd, .. } = &mut *lock(&t) {
                    *fd = Some(read);
                }
                self.clipboard.reading.push((mime, t));
            }
        }
        self.clipboard_finish_reads();
    }

    /// Pumps active read pipes and, once every requested MIME type has finished, commits the copy
    /// to history / `held` (and serves it immediately when the source already disappeared).
    fn clipboard_finish_reads(&mut self) {
        self.clipboard.pump();
        let c = &mut self.clipboard;
        if c.reading.is_empty() || c.reading.iter().any(|(_, t)| !lock(t).finished()) {
            return;
        }
        let mut contents = Contents::default();
        let mut over = false;
        for (mime, t) in std::mem::take(&mut c.reading) {
            if let Transfer::Read { bytes, over: o, .. } = &mut *lock(&t) {
                over |= *o;
                if !bytes.is_empty() {
                    contents.0.push((mime, Arc::from(std::mem::take(bytes))));
                }
            }
        }
        if over || contents.bytes() > COPY_BYTES || contents.0.is_empty() {
            if std::mem::take(&mut c.orphaned) {
                self.clipboard_clear();
            }
            return;
        }
        if c.history_on {
            c.history.add(contents.clone());
            c.listing_changed = true;
        }
        c.held = Some(contents.clone());
        if std::mem::take(&mut c.orphaned) && c.persist {
            self.clipboard_serve(contents);
        }
    }

    /// The clipboard's app went: its copy is offered by the compositor when it is kept (or once
    /// it is read); otherwise the clipboard is cleared.
    pub(crate) fn clipboard_orphaned(&mut self) {
        if self.clipboard.persist
            && self.clipboard.held.is_none()
            && !self.clipboard.reading.is_empty()
        {
            // Drain pipes whose writer just closed so the compositor takes over the selection
            // synchronously before a pasting client sees the dead source.
            self.clipboard.orphaned = true;
            self.clipboard_finish_reads();
            if self.clipboard.held.is_some() || self.clipboard.reading.is_empty() {
                return;
            }
        }
        let c = &mut self.clipboard;
        if !c.persist {
            self.clipboard_clear();
        } else if let Some(contents) = c.held.clone() {
            self.clipboard_serve(contents);
        } else if !c.reading.is_empty() || c.due.is_some() {
            c.orphaned = true;
        } else {
            self.clipboard_clear();
        }
    }

    fn clipboard_clear(&mut self) {
        self.clipboard.held = None;
        smithay::wayland::selection::data_device::clear_data_device_selection(&self.dh, &self.seat);
    }

    /// The compositor offers `contents` as the clipboard.
    fn clipboard_serve(&mut self, contents: Contents) {
        self.clipboard.held = Some(contents.clone());
        smithay::wayland::selection::data_device::set_data_device_selection(
            &self.dh,
            &self.seat,
            contents.mime_types(),
            contents,
        );
    }

    /// An app pastes the compositor's copy as `mime`: written to `fd` as the app reads it.
    pub(crate) fn clipboard_send(&mut self, contents: &Contents, mime: &str, fd: OwnedFd) {
        let Some(data) = contents.get(mime) else {
            return;
        };
        if let Err(e) = nonblocking(&fd) {
            log_line!("[infynitae] Clipboard paste not written: {e}");
            return;
        }
        let mut write = Transfer::Write {
            fd: Some(fd),
            data,
            at: 0,
        };
        // most pastes fit the pipe at once; a longer one is watched, and the watch's copy of the
        // fd closes once it sees the paste finished
        if write.pump() {
            return;
        }
        let watched = match &write {
            Transfer::Write { fd: Some(fd), .. } => rustix::io::dup(fd),
            _ => return,
        };
        let t: Shared = Arc::new(Mutex::new(write));
        match watched {
            Ok(copy) => self.clipboard.watch.push((copy, t.clone(), true)),
            Err(e) => log_line!("[infynitae] Clipboard pipe not watched: {e}"),
        }
        self.clipboard.sends.push(t);
    }

    /// A shell's ask of the history (`infynitae msg clipboard use|pin|unpin|delete`).
    pub fn clipboard_ask(&mut self, ask: ClipboardAsk) {
        let c = &mut self.clipboard;
        let changed = match ask {
            ClipboardAsk::Use(id) => match c.history.take_to_front(id) {
                Some(contents) => {
                    // the copy the compositor offers is no app's: nothing of it is read again
                    c.due = None;
                    c.reading.clear();
                    c.orphaned = false;
                    self.selection_owners[0] = None;
                    self.selection_sources[0] = None;
                    self.clipboard_serve(contents);
                    true
                }
                None => false,
            },
            ClipboardAsk::Pin(id, on) => c.history.pin(id, on),
            ClipboardAsk::Delete(id) => c.history.delete(id),
        };
        self.clipboard.listing_changed |= changed;
    }

    /// The history listing for the shell, when it changed since the last call.
    pub fn clipboard_listing(&mut self) -> Option<Vec<Listed>> {
        self.clipboard.take_listing()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &str) -> Contents {
        Contents(vec![(
            "text/plain;charset=utf-8".to_string(),
            Arc::from(s.as_bytes()),
        )])
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn text_types_and_one_image_type_are_read_and_a_secret_none() {
        let offered = strings(&[
            "text/html",
            "image/bmp",
            "image/png",
            "UTF8_STRING",
            "application/x-qt-image",
            "text/plain",
        ]);
        assert_eq!(
            types_to_read(&offered),
            Some(strings(&[
                "text/html",
                "image/png",
                "UTF8_STRING",
                "text/plain"
            ]))
        );
        assert_eq!(
            types_to_read(&strings(&["image/jpeg", "image/bmp"])),
            Some(strings(&["image/jpeg"]))
        );
        assert_eq!(
            types_to_read(&strings(&["text/plain", SECRET_HINT])),
            None,
            "a password manager's copy is not read"
        );
    }

    #[test]
    fn the_history_is_newest_first_and_a_repeat_moves_up_with_its_pin() {
        let mut h = History::default();
        h.add(text("one"));
        h.add(text("two"));
        let one = h.listing()[1].id;
        assert!(h.pin(one, true));
        h.add(text("three"));
        h.add(text("one"));
        let l = h.listing();
        assert_eq!(l.len(), 3, "the repeat replaced the older entry");
        assert_eq!(l[0].preview, Preview::Text("one".into()));
        assert!(l[0].pinned, "and kept its pin");
        assert_eq!(l[1].preview, Preview::Text("three".into()));
        assert_eq!(l[2].preview, Preview::Text("two".into()));
        assert!(h.delete(l[2].id));
        assert!(!h.delete(l[2].id));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn the_bounds_hold_and_pinned_entries_go_last() {
        let mut h = History::default();
        h.add(text("kept"));
        let kept = h.listing()[0].id;
        h.pin(kept, true);
        for i in 0..HISTORY_ENTRIES + 50 {
            h.add(text(&format!("copy {i}")));
        }
        assert_eq!(h.len(), HISTORY_ENTRIES);
        let l = h.listing();
        assert_eq!(l[0].id, kept, "the pinned entry outlived 349 newer ones");
        assert_eq!(
            l[1].preview,
            Preview::Text(format!("copy {}", HISTORY_ENTRIES + 49))
        );

        let mut h = History::default();
        let big = |b: u8| {
            Contents(vec![(
                "image/png".to_string(),
                Arc::from(vec![b; 10 << 20]),
            )])
        };
        for b in 0..10 {
            h.add(big(b));
        }
        assert!(h.bytes() <= HISTORY_BYTES, "{} bytes", h.bytes());
        assert_eq!(h.len(), HISTORY_BYTES / (10 << 20));
        // pinned entries alone over the bound: the oldest pinned go too
        for e in h.listing() {
            h.pin(e.id, true);
        }
        h.add(big(200));
        assert!(h.bytes() <= HISTORY_BYTES);
    }

    #[test]
    fn a_png_is_listed_with_its_size_and_a_text_cut_to_the_preview() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        let mut h = History::default();
        h.add(Contents(vec![
            ("text/html".to_string(), Arc::from(&b"<img>"[..])),
            ("image/png".to_string(), Arc::from(png.as_slice())),
        ]));
        h.add(text(&"x".repeat(1000)));
        let l = h.listing();
        assert_eq!(l[0].preview, Preview::Text("x".repeat(PREVIEW_CHARS)));
        assert_eq!(
            l[1].preview,
            Preview::Image {
                mime: "image/png".into(),
                bytes: png.len(),
                size: Some((1920, 1080)),
            }
        );
    }

    #[test]
    fn a_read_ends_with_its_writer_and_a_write_with_its_reader() {
        let (r, w) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC).unwrap();
        nonblocking(&r).unwrap();
        let mut t = Transfer::Read {
            fd: Some(r),
            bytes: Vec::new(),
            over: false,
        };
        assert!(!t.pump(), "nothing written yet: not finished");
        rustix::io::write(&w, b"abc").unwrap();
        assert!(!t.pump());
        drop(w);
        assert!(t.pump(), "the writer closed: finished");
        assert!(matches!(&t, Transfer::Read { bytes, over: false, .. } if bytes == b"abc"));

        let (r, w) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC).unwrap();
        nonblocking(&w).unwrap();
        let mut t = Transfer::Write {
            fd: Some(w),
            data: Arc::from(vec![7u8; 1 << 20]),
            at: 0,
        };
        assert!(!t.pump(), "the pipe is full long before 1 MiB");
        drop(r);
        assert!(t.pump(), "the reader closed: finished, not stuck");
    }
}
