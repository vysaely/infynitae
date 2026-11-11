//! Non-blocking logger for the compositor session log.
//!
//! Log calls format the line and send it over a bounded channel to a dedicated writer thread
//! that writes to stdout and flushes. If the writer thread is delayed (e.g. a slow write to disk)
//! and the channel buffer fills up, log lines are dropped instead of blocking the main thread.
//! Once the writer can write again, the number of dropped lines is reported.

use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, OnceLock};

/// Default capacity of the bounded log channel buffer.
pub const DEFAULT_CAPACITY: usize = 4096;

/// A single message sent to the background writer thread.
struct LogEntry {
    line: String,
    dropped_before: usize,
}

enum LogCommand {
    Line(LogEntry),
    Flush(SyncSender<()>),
}

/// An asynchronous non-blocking logger.
pub struct Logger {
    sender: SyncSender<LogCommand>,
    dropped: Arc<AtomicUsize>,
}

impl Logger {
    /// Creates a new logger writing to `out` with a bounded queue of `capacity`.
    pub fn new<W: Write + Send + 'static>(
        out: W,
        capacity: usize,
    ) -> (Self, std::thread::JoinHandle<()>) {
        let (sender, receiver) = sync_channel(capacity);
        let dropped = Arc::new(AtomicUsize::new(0));
        let dropped_writer = Arc::clone(&dropped);
        let handle = std::thread::Builder::new()
            .name("infynitae-log".into())
            .spawn(move || {
                writer_loop(out, receiver, dropped_writer);
            })
            .expect("failed to spawn log writer thread");
        (Self { sender, dropped }, handle)
    }

    /// Sends a line to be logged. Never blocks: if the channel is full, the line is dropped
    /// and the dropped count is incremented.
    pub fn log(&self, line: String) {
        let dropped_before = self.dropped.swap(0, Ordering::Relaxed);
        match self.sender.try_send(LogCommand::Line(LogEntry {
            line,
            dropped_before,
        })) {
            Ok(()) => {}
            Err(TrySendError::Full(LogCommand::Line(entry))) => {
                self.dropped
                    .fetch_add(entry.dropped_before + 1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(LogCommand::Line(entry))) => {
                self.dropped
                    .fetch_add(entry.dropped_before + 1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// Waits for all currently queued messages to be written and flushed.
    pub fn flush(&self) {
        let (ack_tx, ack_rx) = sync_channel(0);
        if self.sender.send(LogCommand::Flush(ack_tx)).is_ok() {
            let _ = ack_rx.recv();
        }
    }
}

fn write_dropped<W: Write>(out: &mut W, count: usize) {
    if count > 0 {
        let _ = writeln!(out, "[infynitae] {count} log lines dropped");
        let _ = out.flush();
    }
}

fn write_line<W: Write>(out: &mut W, line: &str) {
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn writer_loop<W: Write>(mut out: W, rx: Receiver<LogCommand>, dropped: Arc<AtomicUsize>) {
    // waits for a line (no timer: an idle compositor wakes nothing here); the lines dropped while the queue was
    // full are counted by the sender and travel with the next line that fits
    while let Ok(cmd) = rx.recv() {
        match cmd {
            LogCommand::Line(entry) => {
                if entry.dropped_before > 0 {
                    write_dropped(&mut out, entry.dropped_before);
                }
                write_line(&mut out, &entry.line);
            }
            LogCommand::Flush(ack) => {
                let n = dropped.swap(0, Ordering::Relaxed);
                write_dropped(&mut out, n);
                let _ = out.flush();
                let _ = ack.send(());
            }
        }
    }
    let n = dropped.swap(0, Ordering::Relaxed);
    write_dropped(&mut out, n);
}

static GLOBAL_LOGGER: OnceLock<Logger> = OnceLock::new();

/// Returns a reference to the global compositor logger.
pub fn global_logger() -> &'static Logger {
    GLOBAL_LOGGER.get_or_init(|| {
        let (logger, _) = Logger::new(std::io::stdout(), DEFAULT_CAPACITY);
        logger
    })
}

/// Logs a line to the global compositor logger.
pub fn log(line: String) {
    global_logger().log(line);
}

/// Flushes the global compositor logger.
pub fn flush() {
    if let Some(logger) = GLOBAL_LOGGER.get() {
        logger.flush();
    }
}

/// Formats a line and sends it to the non-blocking log writer thread.
#[macro_export]
macro_rules! log_line {
    () => {
        $crate::logger::log(String::new())
    };
    ($($arg:tt)*) => {
        $crate::logger::log(format!($($arg)*))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier, Mutex};
    use std::time::Duration;
    use std::time::Instant;

    struct TestBuffer {
        data: Arc<Mutex<Vec<u8>>>,
        entered: Option<std::sync::mpsc::Sender<()>>,
        gate: Option<Arc<Barrier>>,
        drained_one: Option<std::sync::mpsc::Sender<()>>,
    }

    impl Write for TestBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                let _ = entered.send(());
                if let Some(gate) = self.gate.as_ref() {
                    gate.wait();
                }
            } else if let Some(drained) = self.drained_one.take() {
                let _ = drained.send(());
            }
            self.data.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn order_is_kept() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let out = TestBuffer {
            data: Arc::clone(&buffer),
            entered: None,
            gate: None,
            drained_one: None,
        };
        let (logger, handle) = Logger::new(out, 64);
        for i in 1..=5 {
            logger.log(format!("line {i}"));
        }
        logger.flush();
        drop(logger);
        handle.join().unwrap();
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "line 1\nline 2\nline 3\nline 4\nline 5\n");
    }

    #[test]
    fn drops_instead_of_blocking_when_writer_is_stuck() {
        let gate = Arc::new(Barrier::new(2));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (drained_tx, drained_rx) = std::sync::mpsc::channel();
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let out = TestBuffer {
            data: Arc::clone(&buffer),
            entered: Some(entered_tx),
            gate: Some(Arc::clone(&gate)),
            drained_one: Some(drained_tx),
        };
        // Channel capacity of 4.
        let (logger, handle) = Logger::new(out, 4);

        // First message causes the writer to block on the gate.
        logger.log("first".to_string());
        entered_rx.recv().unwrap();

        // While the writer is stuck, queue 4 messages (fills the channel).
        for i in 1..=4 {
            logger.log(format!("queued {i}"));
        }

        // Send 3 more messages that must be dropped because channel is full.
        let start = Instant::now();
        for i in 1..=3 {
            logger.log(format!("overflow {i}"));
        }
        // Verify sender did not block while writer is stuck.
        assert!(start.elapsed() < Duration::from_millis(100));

        // Release the writer and wait until it consumes at least one queued message.
        gate.wait();
        drained_rx.recv().unwrap();

        // Send a message after unblocking.
        logger.log("after unblock".to_string());

        logger.flush();
        drop(logger);
        handle.join().unwrap();

        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(
            output,
            "first\nqueued 1\nqueued 2\nqueued 3\nqueued 4\n[infynitae] 3 log lines dropped\nafter unblock\n"
        );
    }

    #[test]
    fn dropped_count_is_reported_when_no_further_lines_sent() {
        let gate = Arc::new(Barrier::new(2));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let out = TestBuffer {
            data: Arc::clone(&buffer),
            entered: Some(entered_tx),
            gate: Some(Arc::clone(&gate)),
            drained_one: None,
        };
        let (logger, handle) = Logger::new(out, 4);

        logger.log("first".to_string());
        entered_rx.recv().unwrap();

        for i in 1..=4 {
            logger.log(format!("queued {i}"));
        }
        for i in 1..=2 {
            logger.log(format!("overflow {i}"));
        }

        gate.wait();
        // Do not send any further line; flush and drop.
        logger.flush();
        drop(logger);
        handle.join().unwrap();

        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(
            output,
            "first\nqueued 1\nqueued 2\nqueued 3\nqueued 4\n[infynitae] 2 log lines dropped\n"
        );
    }
}
