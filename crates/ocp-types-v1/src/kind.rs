//! Envelope kind grammar and parser.
//!
//! See `kinds-1.md` for the normative grammar. A `Kind` is a dot-separated
//! namespaced string of the form `<class>.<segment>+`, where `<class>` is one
//! of `event`, `response`, `request`, `control`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// The class portion of a kind. Always matches the envelope's `class` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KindClass {
    /// `event.*` — emitted by plugins during a `run`.
    Event,
    /// `response.*` — emitted exactly once per inspection invocation.
    Response,
    /// `request.*` — reserved for future bidirectional transports. Currently unused.
    Request,
    /// `control.*` — sent by the host to the plugin via stdin.
    Control,
}

impl KindClass {
    /// Return the string form (`"event"`, `"response"`, `"request"`, `"control"`).
    pub fn as_str(self) -> &'static str {
        match self {
            KindClass::Event => "event",
            KindClass::Response => "response",
            KindClass::Request => "request",
            KindClass::Control => "control",
        }
    }
}

impl fmt::Display for KindClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An envelope kind. The `Kind` type is a wrapper around the raw string with
/// accessors for the parsed class and first-segment-after-class.
///
/// Wire form: a JSON string like `"event.run.started"`. The crate stores it as
/// a `String` to preserve unknown kinds verbatim — consumers MUST round-trip
/// vendor and future-standard kinds without dropping or rewriting them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Kind(String);

/// Errors produced when parsing a kind string against the grammar in
/// `kinds-1.md` §1.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KindParseError {
    /// The kind was empty.
    #[error("kind is empty")]
    Empty,
    /// The kind exceeded 256 bytes.
    #[error("kind exceeds 256 bytes")]
    TooLong,
    /// The kind did not start with one of the four valid classes.
    #[error("kind '{0}' does not start with a valid class (event/response/request/control)")]
    UnknownClass(String),
    /// The kind had no segment after the class.
    #[error("kind '{0}' has no segments after the class")]
    MissingSegment(String),
    /// A segment did not match `^[a-z][a-z0-9_]*$`.
    #[error("kind '{kind}' has invalid segment '{segment}'")]
    InvalidSegment {
        /// The full kind string.
        kind: String,
        /// The offending segment.
        segment: String,
    },
}

impl Kind {
    /// Construct a `Kind` from a raw string, validating it against the grammar
    /// in `kinds-1.md` §1.
    pub fn parse(s: impl Into<String>) -> Result<Self, KindParseError> {
        let s = s.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    /// Construct a `Kind` from a raw string **without** validation. Useful for
    /// preserving unknown future kinds during round-trip; do not use for
    /// producer-side construction.
    pub fn from_raw(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The full kind string (e.g., `"event.run.started"`).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The parsed class portion. Returns `None` if the kind does not validate.
    pub fn class(&self) -> Option<KindClass> {
        let (head, _) = self.0.split_once('.')?;
        match head {
            "event" => Some(KindClass::Event),
            "response" => Some(KindClass::Response),
            "request" => Some(KindClass::Request),
            "control" => Some(KindClass::Control),
            _ => None,
        }
    }

    /// The first segment after the class. For dispatch by `(class, first_segment)`
    /// per `kinds-1.md` §9.3.
    pub fn first_segment(&self) -> Option<&str> {
        let mut parts = self.0.splitn(3, '.');
        parts.next()?; // class
        parts.next()
    }

    fn validate(s: &str) -> Result<(), KindParseError> {
        if s.is_empty() {
            return Err(KindParseError::Empty);
        }
        if s.len() > 256 {
            return Err(KindParseError::TooLong);
        }
        let mut iter = s.split('.');
        let class = iter.next().ok_or_else(|| KindParseError::Empty)?;
        match class {
            "event" | "response" | "request" | "control" => {}
            _ => return Err(KindParseError::UnknownClass(s.to_string())),
        }
        let mut saw_segment = false;
        for segment in iter {
            saw_segment = true;
            if !is_valid_segment(segment) {
                return Err(KindParseError::InvalidSegment {
                    kind: s.to_string(),
                    segment: segment.to_string(),
                });
            }
        }
        if !saw_segment {
            return Err(KindParseError::MissingSegment(s.to_string()));
        }
        Ok(())
    }
}

fn is_valid_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    let mut bytes = segment.bytes();
    let first = bytes.next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Kind {
    type Err = KindParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_event_kind() {
        let k = Kind::parse("event.run.started").unwrap();
        assert_eq!(k.class(), Some(KindClass::Event));
        assert_eq!(k.first_segment(), Some("run"));
    }

    #[test]
    fn parses_vendor_kind() {
        let k = Kind::parse("event.maxdiff_pipeline.stage_started").unwrap();
        assert_eq!(k.class(), Some(KindClass::Event));
        assert_eq!(k.first_segment(), Some("maxdiff_pipeline"));
    }

    #[test]
    fn rejects_unknown_class() {
        let err = Kind::parse("status.run.started").unwrap_err();
        assert!(matches!(err, KindParseError::UnknownClass(_)));
    }

    #[test]
    fn rejects_class_only() {
        let err = Kind::parse("event").unwrap_err();
        assert!(matches!(err, KindParseError::MissingSegment(_)));
    }

    #[test]
    fn rejects_uppercase_segment() {
        let err = Kind::parse("event.Run.started").unwrap_err();
        assert!(matches!(err, KindParseError::InvalidSegment { .. }));
    }

    #[test]
    fn rejects_segment_starting_with_digit() {
        let err = Kind::parse("event.run.1started").unwrap_err();
        assert!(matches!(err, KindParseError::InvalidSegment { .. }));
    }

    #[test]
    fn rejects_overlong_kind() {
        let long = format!("event.run.{}", "x".repeat(300));
        let err = Kind::parse(long).unwrap_err();
        assert_eq!(err, KindParseError::TooLong);
    }

    #[test]
    fn from_raw_preserves_unknown_kinds_for_round_trip() {
        // A future kind we don't recognize is still preserved verbatim.
        let k = Kind::from_raw("event.future_namespace.future_kind");
        assert_eq!(k.as_str(), "event.future_namespace.future_kind");
        assert_eq!(k.first_segment(), Some("future_namespace"));
    }

    #[test]
    fn round_trip_via_serde() {
        let k = Kind::parse("event.artifact.created").unwrap();
        let json = serde_json::to_value(&k).unwrap();
        assert_eq!(json, serde_json::json!("event.artifact.created"));
        let back: Kind = serde_json::from_value(json).unwrap();
        assert_eq!(back, k);
    }
}
