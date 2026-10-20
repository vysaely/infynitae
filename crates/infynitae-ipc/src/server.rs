//! The shell server (C7.1, C7.3, OBJ-31): connections, their requests in order, replies, and the
//! event stream of subscribers.
//!
//! Threads do the socket I/O only: one acceptor, and per connection a reader (lines in) and a
//! writer (lines out). Every request is executed on the compositor's thread, in
//! [`ShellServer::service`], which is the only code that holds the `ShellPort` (C7.4: `!Send`, it
//! never leaves that thread). A blocked shell never blocks the compositor: replies and events go
//! through a bounded queue, and a shell that lets it fill is disconnected (it re-syncs with a
//! Snapshot when it reconnects, C7.3). The threads wake the compositor's loop through `wake`;
//! nothing runs while no shell sends anything (INV-IDLE-1).

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

use infynitae_core::protocol::Effect;
use infynitae_core::{Core, ShellCommand, ShellPort, ShellStream};

use crate::model::Model;
use crate::protocol::{self, MAX_LINE, Request, RequestError};

/// How many lines may wait for one shell before it is disconnected.
pub const OUTBOX: usize = 1024;

/// Wakes the compositor's loop (a loop ping); called from the I/O threads.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Decides whether a connection is admitted (C7.1: the peer's uid); runs on the acceptor thread.
pub type Admit = Box<dyn Fn(&UnixStream) -> bool + Send>;

enum Inbound {
    Opened {
        id: u64,
        out: SyncSender<String>,
        stream: UnixStream,
    },
    Line(u64, String),
    Closed(u64),
}

struct Conn {
    id: u64,
    out: SyncSender<String>,
    stream: UnixStream,
    subscribed: bool,
    /// This connection opened a shell stream that it has not ended.
    streaming: bool,
}

struct Acceptor {
    path: PathBuf,
    stop: Arc<AtomicBool>,
}

/// What one service round did, for the loop.
#[derive(Debug, Default)]
pub struct Serviced {
    /// Core's effects of the commands executed, in order, for the adapter to apply.
    pub effects: Vec<Effect>,
    /// A command changed state (what is shown may have changed: the loop asks for a frame).
    pub changed: bool,
}

/// The shell server: owner of the `ShellPort` (C7.1) and of every connection's state (OBJ-31).
pub struct ShellServer {
    port: ShellPort,
    wake: Wake,
    tx: Sender<Inbound>,
    rx: Receiver<Inbound>,
    next_id: Arc<Mutex<u64>>,
    conns: Vec<Conn>,
    /// The last copy the shells were given (or `None` before the first): events are its diff.
    model: Option<Model>,
    generation: u64,
    acceptor: Option<Acceptor>,
}

impl ShellServer {
    /// `ipc::serve(ShellPort)` (C7.4): the port is consumed; no connection exists yet.
    pub fn new(port: ShellPort, wake: Wake) -> ShellServer {
        let (tx, rx) = mpsc::channel();
        ShellServer {
            port,
            wake,
            tx,
            rx,
            next_id: Arc::new(Mutex::new(1)),
            conns: Vec::new(),
            model: None,
            generation: 0,
            acceptor: None,
        }
    }

    /// Accepts connections on `listener` (bound at `path`), each admitted by `admit`.
    pub fn listen(&mut self, listener: UnixListener, path: PathBuf, admit: Admit) {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, wake, ids, stop_t) = (
            self.tx.clone(),
            self.wake.clone(),
            self.next_id.clone(),
            stop.clone(),
        );
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                let Ok(stream) = stream else { continue };
                if !admit(&stream) {
                    continue; // dropped: closed without a word
                }
                open(stream, &tx, &wake, &ids);
            }
        });
        self.acceptor = Some(Acceptor { path, stop: stop_t });
    }

    /// Adds a connection already admitted (a test's socket pair, or a launcher's).
    pub fn connect(&mut self, stream: UnixStream) {
        open(stream, &self.tx, &self.wake, &self.next_id);
    }

    /// How many connections are open.
    pub fn connections(&self) -> usize {
        self.conns.len()
    }

    /// The current generation.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Runs, on the compositor's thread, everything the shells sent since the last call, in
    /// arrival order (per connection: the order sent, C7.3), then brings the subscribers up to
    /// date. Call it after every loop dispatch; it does nothing when nothing arrived and nobody
    /// subscribes.
    pub fn service(&mut self, core: &mut Core) -> Serviced {
        let mut done = Serviced::default();
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Inbound::Opened { id, out, stream } => self.conns.push(Conn {
                    id,
                    out,
                    stream,
                    subscribed: false,
                    streaming: false,
                }),
                Inbound::Closed(id) => self.close(id, core, &mut done),
                Inbound::Line(id, line) => self.request(id, &line, core, &mut done),
            }
        }
        if self.conns.iter().any(|c| c.subscribed) {
            self.refresh(core, &mut done);
        }
        done
    }

    fn request(&mut self, id: u64, line: &str, core: &mut Core, done: &mut Serviced) {
        let (rid, req) = protocol::parse_request(line, core);
        let reply = match req {
            Err(e) => protocol::error_reply(rid, e),
            Ok(Request::Snapshot) => {
                let g = self.refresh(core, done);
                protocol::ok_reply(rid, g, self.model.as_ref().map(Model::to_value))
            }
            Ok(Request::Subscribe) => {
                // the events up to this generation go to the subscribers there were; this one
                // starts from the snapshot it is given
                let g = self.refresh(core, done);
                if let Some(c) = self.conns.iter_mut().find(|c| c.id == id) {
                    c.subscribed = true;
                }
                protocol::ok_reply(rid, g, self.model.as_ref().map(Model::to_value))
            }
            Ok(Request::Unsubscribe) => {
                if let Some(c) = self.conns.iter_mut().find(|c| c.id == id) {
                    c.subscribed = false;
                }
                protocol::ok_reply(rid, self.generation, None)
            }
            Ok(Request::Command(cmd)) => match core.shell_command(&mut self.port, cmd) {
                Ok(fx) => {
                    done.effects.extend(fx);
                    done.changed = true;
                    if let Some(c) = self.conns.iter_mut().find(|c| c.id == id) {
                        match stream_of(&cmd) {
                            ShellStream::Begin | ShellStream::Step => c.streaming = true,
                            ShellStream::End => c.streaming = false,
                            ShellStream::Atomic => {}
                        }
                    }
                    let g = self.refresh(core, done);
                    protocol::ok_reply(rid, g, None)
                }
                Err(e) => protocol::error_reply(rid, RequestError::Shell(e)),
            },
        };
        self.send(id, reply, core, done);
    }

    /// Compares Core's state with the last copy; when it changed, the generation advances and the
    /// subscribers get the events, stamped with it. Returns the current generation.
    fn refresh(&mut self, core: &mut Core, done: &mut Serviced) -> u64 {
        let next = Model::of(core);
        match &self.model {
            None => {}
            Some(old) => {
                let events = old.events_to(&next);
                if !events.is_empty() {
                    self.generation += 1;
                    let lines: Vec<String> = events
                        .into_iter()
                        .map(|e| protocol::event_line(e, self.generation))
                        .collect();
                    let subscribers: Vec<u64> = self
                        .conns
                        .iter()
                        .filter(|c| c.subscribed)
                        .map(|c| c.id)
                        .collect();
                    for id in subscribers {
                        for line in &lines {
                            self.send(id, line.clone(), core, done);
                        }
                    }
                }
            }
        }
        self.model = Some(next);
        self.generation
    }

    /// Queues a line for a connection; one whose queue is full (a shell that stopped reading) or
    /// gone is closed.
    fn send(&mut self, id: u64, line: String, core: &mut Core, done: &mut Serviced) {
        let Some(c) = self.conns.iter().find(|c| c.id == id) else {
            return;
        };
        match c.out.try_send(line) {
            Ok(()) => {}
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => {
                self.close(id, core, done);
            }
        }
    }

    /// The connection is gone (or is dropped): its threads end, and a stream it left open ends
    /// (C7.3: nothing to roll back; the epoch settles).
    fn close(&mut self, id: u64, core: &mut Core, done: &mut Serviced) {
        let Some(i) = self.conns.iter().position(|c| c.id == id) else {
            return;
        };
        let c = self.conns.remove(i);
        let _ = c.stream.shutdown(std::net::Shutdown::Both);
        if c.streaming
            && let Ok(fx) = core.end_shell_stream(&mut self.port)
        {
            done.effects.extend(fx);
            done.changed = true;
        }
    }
}

impl Drop for ShellServer {
    fn drop(&mut self) {
        for c in &self.conns {
            let _ = c.stream.shutdown(std::net::Shutdown::Both);
        }
        if let Some(a) = self.acceptor.take() {
            a.stop.store(true, Ordering::Release);
            // the acceptor blocks in accept: one connection lets it see the flag
            let _ = UnixStream::connect(&a.path);
        }
    }
}

fn stream_of(cmd: &ShellCommand) -> ShellStream {
    match cmd {
        ShellCommand::Navigate { stream, .. } | ShellCommand::MoveElement { stream, .. } => *stream,
        _ => ShellStream::Atomic,
    }
}

/// Starts a connection's reader and writer threads and announces it.
fn open(stream: UnixStream, tx: &Sender<Inbound>, wake: &Wake, ids: &Mutex<u64>) {
    let (Ok(reader), Ok(writer), Ok(handle)) =
        (stream.try_clone(), stream.try_clone(), stream.try_clone())
    else {
        return;
    };
    let id = {
        let mut n = ids.lock().unwrap();
        let id = *n;
        *n += 1;
        id
    };
    let (out, outbox) = mpsc::sync_channel::<String>(OUTBOX);
    if tx
        .send(Inbound::Opened {
            id,
            out,
            stream: handle,
        })
        .is_err()
    {
        return;
    }
    std::thread::spawn(move || write_lines(writer, outbox));
    let (tx, wake_r) = (tx.clone(), wake.clone());
    std::thread::spawn(move || read_lines(id, reader, &tx, &wake_r));
    wake();
}

fn write_lines(mut w: UnixStream, outbox: Receiver<String>) {
    for mut line in outbox {
        line.push('\n');
        if w.write_all(line.as_bytes()).is_err() {
            return;
        }
    }
}

/// Reads lines until the end or an error; a line longer than [`MAX_LINE`] or not UTF-8 ends the
/// connection (the shell is broken; it reconnects).
fn read_lines(id: u64, mut r: UnixStream, tx: &Sender<Inbound>, wake: &Wake) {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    'read: loop {
        let n = match r.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=nl).collect();
            let Ok(text) = String::from_utf8(line) else {
                break 'read;
            };
            let text = text.trim_end_matches(['\n', '\r']);
            if text.is_empty() {
                continue;
            }
            if tx.send(Inbound::Line(id, text.to_owned())).is_err() {
                return;
            }
            wake();
        }
        if buf.len() > MAX_LINE {
            break;
        }
    }
    let _ = tx.send(Inbound::Closed(id));
    wake();
}
