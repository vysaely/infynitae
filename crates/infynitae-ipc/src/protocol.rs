//! The shell protocol (C7.3; owner decision 2026-10-20, D2/D3/D8): one JSON object per line in each
//! direction.
//!
//! Requests: `{"id": <number|string>, "cmd": <name>, ...}`. `snapshot`, `subscribe` and
//! `unsubscribe` are queries; `navigate`, `viewport_fit`, `viewport_restore`, `focus`,
//! `acknowledge_attention` and `move_element` are commands Core validates and executes. Elements are
//! named `[instance, element]` (INV-STAGE-3), outputs by their id. `layers` is a diagnostic
//! query: each window's surfaces and the physical rects they are drawn at.
//!
//! Replies: `{"id": …, "ok": true, "generation": g}` (plus `"snapshot"` for the queries that return
//! one) or `{"id": …, "ok": false, "error": <code>}`; nothing was changed by a failed request.
//! Events (subscribers only): `{"event": <name>, "generation": g, …}`, totally ordered per
//! connection; a subscriber that missed some re-syncs with a Snapshot and the events after it.

use infynitae_core::output::OutputId;
use infynitae_core::stage::ElementRef;
use infynitae_core::{Core, NavigateTo, ShellCommand, ShellError, ShellStream};

use crate::json::{self, Value};

/// The longest request line accepted, in bytes; a longer one closes the connection.
pub const MAX_LINE: usize = 64 * 1024;

/// What a request asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Snapshot,
    /// A diagnostic: every window's surfaces as Core holds them and the rects it draws them at.
    Layers,
    Subscribe,
    Unsubscribe,
    Command(ShellCommand),
}

/// Why a request was not executed, as its reply names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestError {
    /// Not one JSON object.
    Parse,
    /// `cmd` names no request.
    UnknownCommand,
    /// A field is missing or has the wrong type.
    Invalid,
    /// Core refused the command.
    Shell(ShellError),
}

impl RequestError {
    pub fn code(self) -> &'static str {
        match self {
            RequestError::Parse => "parse_error",
            RequestError::UnknownCommand => "unknown_command",
            RequestError::Invalid => "invalid_request",
            RequestError::Shell(e) => match e {
                ShellError::ForeignPort => "foreign_port",
                ShellError::StaleElement => "stale_element",
                ShellError::UnknownOutput => "unknown_output",
                ShellError::InvalidArgument => "invalid_argument",
                ShellError::NotMapped => "not_mapped",
                ShellError::Presenting => "presenting",
                ShellError::Busy => "busy",
                ShellError::Refused => "refused",
            },
        }
    }
}

/// The request id to echo: a number or a string as given, `null` otherwise.
pub fn request_id(v: Option<&Value>) -> Value {
    match v {
        Some(Value::Num(n)) => Value::Num(*n),
        Some(Value::Str(s)) => Value::Str(s.clone()),
        _ => Value::Null,
    }
}

/// Parses one request line into its id and what it asks. Outputs are resolved against `core`'s
/// connected outputs (an id of a gone output is `unknown_output`); element references are resolved
/// by Core when the command runs.
pub fn parse_request(line: &str, core: &Core) -> (Value, Result<Request, RequestError>) {
    let Ok(v) = json::parse(line) else {
        return (Value::Null, Err(RequestError::Parse));
    };
    if !matches!(v, Value::Obj(_)) {
        return (Value::Null, Err(RequestError::Parse));
    }
    let id = request_id(v.get("id"));
    (id, request(&v, core))
}

fn request(v: &Value, core: &Core) -> Result<Request, RequestError> {
    use RequestError::Invalid;
    let cmd = v.get("cmd").and_then(Value::as_str).ok_or(Invalid)?;
    let output = || -> Result<OutputId, RequestError> {
        let raw = v.get("output").and_then(Value::as_u64).ok_or(Invalid)?;
        core.outputs()
            .iter()
            .map(|o| o.id)
            .find(|o| o.get() == raw)
            .ok_or(RequestError::Shell(ShellError::UnknownOutput))
    };
    let element = |key: &str| element_of(v.get(key)).ok_or(Invalid);
    let stream = || match v.get("stream") {
        None => Ok(ShellStream::Atomic),
        Some(s) => match s.as_str() {
            Some("begin") => Ok(ShellStream::Begin),
            Some("step") => Ok(ShellStream::Step),
            Some("end") => Ok(ShellStream::End),
            _ => Err(Invalid),
        },
    };
    Ok(match cmd {
        "snapshot" => Request::Snapshot,
        "layers" => Request::Layers,
        "subscribe" => Request::Subscribe,
        "unsubscribe" => Request::Unsubscribe,
        "navigate" => {
            let to = v.get("to").ok_or(Invalid)?;
            let to = if let Some(e) = to.get("element") {
                NavigateTo::Element(element_of(Some(e)).ok_or(Invalid)?)
            } else if let Some(p) = to.get("canvas") {
                let (x, y) = point(Some(p)).ok_or(Invalid)?;
                NavigateTo::Canvas(x, y)
            } else if let Some(p) = to.get("delta") {
                let (x, y) = point(Some(p)).ok_or(Invalid)?;
                NavigateTo::Delta(x, y)
            } else {
                return Err(Invalid);
            };
            Request::Command(ShellCommand::Navigate {
                output: output()?,
                to,
                stream: stream()?,
            })
        }
        "viewport_fit" => Request::Command(ShellCommand::ViewportFit {
            output: output()?,
            element: element("element")?,
        }),
        "viewport_restore" => Request::Command(ShellCommand::ViewportRestore { output: output()? }),
        "focus" => Request::Command(ShellCommand::Focus {
            element: element("element")?,
        }),
        "acknowledge_attention" => Request::Command(ShellCommand::AcknowledgeAttention {
            element: element("element")?,
        }),
        "move_element" => Request::Command(ShellCommand::MoveElement {
            element: element("element")?,
            to: point(v.get("to")).ok_or(Invalid)?,
            stream: stream()?,
        }),
        _ => return Err(RequestError::UnknownCommand),
    })
}

/// `[instance, element]`.
fn element_of(v: Option<&Value>) -> Option<ElementRef> {
    match v?.as_array()? {
        [i, e] => ElementRef::from_raw(i.as_u64()?, e.as_u64()?),
        _ => None,
    }
}

/// `[x, y]`, two finite numbers (Core checks the Canvas bound).
fn point(v: Option<&Value>) -> Option<(f64, f64)> {
    match v?.as_array()? {
        [x, y] => Some((x.as_f64()?, y.as_f64()?)),
        _ => None,
    }
}

/// `{"id", "ok": true, "generation"}`, with a snapshot when given.
pub fn ok_reply(id: Value, generation: u64, snapshot: Option<Value>) -> String {
    let mut m = vec![
        ("id".to_owned(), id),
        ("ok".to_owned(), true.into()),
        ("generation".to_owned(), generation.into()),
    ];
    if let Some(s) = snapshot {
        m.push(("snapshot".to_owned(), s));
    }
    Value::Obj(m).to_json()
}

/// `{"id", "ok": false, "error"}`.
pub fn error_reply(id: Value, e: RequestError) -> String {
    Value::obj([("id", id), ("ok", false.into()), ("error", e.code().into())]).to_json()
}

/// An event stamped with its generation.
pub fn event_line(mut event: Value, generation: u64) -> String {
    if let Value::Obj(m) = &mut event {
        m.insert(1, ("generation".to_owned(), generation.into()));
    }
    event.to_json()
}
