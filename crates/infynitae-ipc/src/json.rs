//! A minimal JSON value, a strict parser and a writer (owner decision 2026-10-20, D2: JSON lines,
//! hand-written, no dependency). The parser accepts RFC 8259 JSON only, with a nesting limit; the
//! writer prints integers below 2⁵³ exactly and never prints a non-finite number.

use std::fmt::Write as _;

/// The deepest nesting the parser accepts.
pub const MAX_DEPTH: usize = 32;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    /// Members in order; a key given twice is a parse error.
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// A non-negative integer a JSON number carries exactly (below 2⁵³).
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Num(n) if n.fract() == 0.0 && (0.0..9_007_199_254_740_992.0).contains(n) => {
                Some(*n as u64)
            }
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Arr(a) => Some(a),
            _ => None,
        }
    }

    /// An object from `(key, value)` pairs.
    pub fn obj<const N: usize>(members: [(&str, Value); N]) -> Value {
        Value::Obj(
            members
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
        )
    }

    /// The compact JSON text of this value.
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        write(self, &mut s);
        s
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::Num(v as f64)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Num(v)
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Str(v.to_owned())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(v: Option<T>) -> Self {
        v.map_or(Value::Null, Into::into)
    }
}

fn write(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Num(n) if !n.is_finite() => out.push_str("null"),
        Value::Num(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
            let _ = write!(out, "{}", *n as i64);
        }
        Value::Num(n) => {
            let _ = write!(out, "{n:?}");
        }
        Value::Str(s) => write_str(s, out),
        Value::Arr(a) => {
            out.push('[');
            for (i, v) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(v, out);
            }
            out.push(']');
        }
        Value::Obj(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_str(k, out);
                out.push(':');
                write(v, out);
            }
            out.push('}');
        }
    }
}

fn write_str(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{2028}' || c == '\u{2029}' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Why a text is not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset of the problem.
    pub at: usize,
}

/// Parses exactly one JSON value (surrounding whitespace allowed).
pub fn parse(text: &str) -> Result<Value, ParseError> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err());
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self) -> ParseError {
        ParseError { at: self.i }
    }

    fn ws(&mut self) {
        while self
            .s
            .get(self.i)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.i += 1;
        }
    }

    fn eat(&mut self, lit: &[u8]) -> Result<(), ParseError> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(self.err())
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err());
        }
        match self.s.get(self.i) {
            Some(b'n') => self.eat(b"null").map(|_| Value::Null),
            Some(b't') => self.eat(b"true").map(|_| Value::Bool(true)),
            Some(b'f') => self.eat(b"false").map(|_| Value::Bool(false)),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Value::Arr(a));
                }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Value::Arr(a));
                        }
                        _ => return Err(self.err()),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut m: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Value::Obj(m));
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        return Err(self.err());
                    }
                    let at = self.i;
                    let k = self.string()?;
                    if m.iter().any(|(e, _)| *e == k) {
                        return Err(ParseError { at });
                    }
                    self.ws();
                    self.eat(b":")?;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    m.push((k, v));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Value::Obj(m));
                        }
                        _ => return Err(self.err()),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.err()),
        }
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        let start = self.i;
        let digits = |p: &mut Self| {
            let s = p.i;
            while p.s.get(p.i).is_some_and(u8::is_ascii_digit) {
                p.i += 1;
            }
            p.i > s
        };
        if self.s.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        // no leading zeros (RFC 8259)
        if self.s.get(self.i) == Some(&b'0') {
            self.i += 1;
        } else if !digits(self) {
            return Err(self.err());
        }
        if self.s.get(self.i) == Some(&b'.') {
            self.i += 1;
            if !digits(self) {
                return Err(self.err());
            }
        }
        if matches!(self.s.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.s.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !digits(self) {
                return Err(self.err());
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| self.err())?;
        let n: f64 = text.parse().map_err(|_| self.err())?;
        if !n.is_finite() {
            return Err(ParseError { at: start });
        }
        Ok(Value::Num(n))
    }

    fn hex4(&mut self) -> Result<u32, ParseError> {
        let h = self.s.get(self.i..self.i + 4).ok_or(self.err())?;
        let h = std::str::from_utf8(h).map_err(|_| self.err())?;
        let v = u32::from_str_radix(h, 16).map_err(|_| self.err())?;
        if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(self.err());
        }
        self.i += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, ParseError> {
        self.i += 1; // the opening quote
        let mut out = String::new();
        loop {
            let start = self.i;
            while self
                .s
                .get(self.i)
                .is_some_and(|&b| b != b'"' && b != b'\\' && b >= 0x20)
            {
                self.i += 1;
            }
            out.push_str(std::str::from_utf8(&self.s[start..self.i]).map_err(|_| self.err())?);
            match self.s.get(self.i) {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let e = *self.s.get(self.i).ok_or(self.err())?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let c = if (0xD800..0xDC00).contains(&hi) {
                                self.eat(b"\\u")?;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(self.err());
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            out.push(char::from_u32(c).ok_or(self.err())?);
                        }
                        _ => return Err(self.err()),
                    }
                }
                // a raw control character or the end of the text
                _ => return Err(self.err()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_exact_integers() {
        let text = r#"{"id":7,"cmd":"navigate","to":{"canvas":[-1.5,2e3]},"x":[true,false,null],"s":"a\"b\\c\n\u00e9\ud83d\ude00"}"#;
        let v = parse(text).unwrap();
        assert_eq!(v.get("id").and_then(Value::as_u64), Some(7));
        assert_eq!(
            v.get("s").and_then(Value::as_str),
            Some("a\"b\\c\né\u{1F600}")
        );
        let again = parse(&v.to_json()).unwrap();
        assert_eq!(again, v);
        assert_eq!(
            Value::from(9_007_199_254_740_991u64).to_json(),
            "9007199254740991"
        );
        assert_eq!(Value::from(-0.25).to_json(), "-0.25");
        assert_eq!(Value::from(f64::NAN).to_json(), "null");
        assert_eq!(Value::from("\u{1}\u{2028}").to_json(), r#""\u0001\u2028""#);
    }

    #[test]
    fn rejects_what_is_not_json() {
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\":1,}",
            "01",
            "1.",
            ".5",
            "-",
            "1e",
            "+1",
            "NaN",
            "Infinity",
            "1e999",
            "\"\u{1}\"",
            "\"\\x\"",
            "\"\\ud800\"",
            "\"\\ud800\\u0041\"",
            "{\"a\":1,\"a\":2}",
            "{a:1}",
            "[1] 2",
            "tru",
            "'a'",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 2), "]".repeat(MAX_DEPTH + 2));
        assert!(parse(&deep).is_err(), "nesting beyond the limit");
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(parse(&ok).is_ok());
    }
}
