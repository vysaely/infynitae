//! Why a fullscreen window's frames do not go to the screen as they are (LATENCY_STATS). Smithay
//! says so only in `trace!` lines, one for each thing it tried, every frame. While a screen shows
//! a fullscreen window, each kind of refusal is printed once; the rest of the time nothing is.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tracing::callsite::Identifier;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

const TARGET: &str = "smithay::backend::drm::compositor";

static WATCHING: AtomicBool = AtomicBool::new(false);
static SEEN: Mutex<Vec<Identifier>> = Mutex::new(Vec::new());
/// Trace lines already known not to be refusals: never formatted again (10-30 a frame otherwise).
static PLAIN: Mutex<Vec<Identifier>> = Mutex::new(Vec::new());

/// Whether the next frames show a fullscreen window; a new fullscreen window reports afresh.
pub(crate) fn watch(on: bool) {
    if on && !WATCHING.swap(true, Ordering::Relaxed) {
        SEEN.lock().unwrap_or_else(|e| e.into_inner()).clear();
    } else if !on {
        WATCHING.store(false, Ordering::Relaxed);
    }
}

/// Starts listening; a later call changes nothing.
pub(crate) fn install() {
    let installed = tracing::subscriber::set_global_default(Refusals).is_ok();
    // a silent log must say why: a build that compiled trace lines out can never explain a refusal
    println!(
        "[infynitae] Direct scanout log: listening {installed}, built with trace lines up to {}",
        tracing::level_filters::STATIC_MAX_LEVEL
    );
}

/// A refusal, as opposed to Smithay's other trace lines.
fn is_refusal(message: &str) -> bool {
    ["skipping", "failed", "could not", "Can't obtain", "too big"]
        .iter()
        .any(|word| message.contains(word))
}

struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        }
    }
}

struct Refusals;

impl Subscriber for Refusals {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.target().starts_with(TARGET)
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        if !WATCHING.load(Ordering::Relaxed) {
            return;
        }
        let callsite = event.metadata().callsite();
        let mut plain = PLAIN.lock().unwrap_or_else(|e| e.into_inner());
        if plain.contains(&callsite)
            || SEEN
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(&callsite)
        {
            return;
        }
        let mut message = Message(String::new());
        event.record(&mut message);
        if !is_refusal(&message.0) {
            // a callsite's words are fixed, only its values change: it never becomes a refusal
            plain.push(callsite);
            return;
        }
        drop(plain);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.contains(&callsite) {
            seen.push(callsite);
            println!("[infynitae] Not shown directly: {}", message.0);
        }
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_refusals_are_reported() {
        assert!(super::is_refusal(
            "skipping direct scan-out on plane, test failed"
        ));
        assert!(super::is_refusal("failed to assign element to primary"));
        assert!(!super::is_refusal("testing direct scan-out for element"));
        assert!(super::is_refusal("failed to add framebuffer: Unsupported"));
        assert!(super::is_refusal(
            "could not import framebuffer for element"
        ));
        assert!(super::is_refusal(
            "Can't obtain cursor's underlying storage"
        ));
        assert!(super::is_refusal(
            "element 3 too big for cursor plane(s), skipping"
        ));
        assert!(!super::is_refusal("assigned element 3 to primary"));
    }
}
