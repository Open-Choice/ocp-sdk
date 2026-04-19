//! Wire-format primitive types.
//!
//! These types are the building blocks of every `ocp-json/1` envelope. Their
//! serialized form is **frozen** as of `ocp-types-1.0.0` and is governed by
//! `wire-format-1.md`.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ---------------------------------------------------------------------------
// Identifier
// ---------------------------------------------------------------------------

/// The wire format used by an [`Identifier`].
///
/// `ocp-json/1` defaults to ULID. Future minor releases may add new variants
/// (e.g., `Uuid`); consumers MUST tolerate unknown variants by preserving the
/// raw tag verbatim via the [`Other`](Self::Other) variant. This is the
/// load-bearing forward-compat property for tagged primitives.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IdentifierFormat {
    /// Crockford base32 ULID, 26 characters, time-sortable. The default for `ocp-json/1`.
    Ulid,
    /// Any other format. The original tag string is preserved verbatim so that
    /// it round-trips losslessly.
    Other(String),
}

impl IdentifierFormat {
    /// Return the wire-form tag string (e.g., `"ulid"`, or the preserved
    /// unknown tag).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Ulid => "ulid",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl Serialize for IdentifierFormat {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for IdentifierFormat {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Ok(match s.as_str() {
            "ulid" => Self::Ulid,
            _ => Self::Other(s),
        })
    }
}

/// A wrapped identifier carrying both its format tag and its raw value.
///
/// Wire form (ULID example):
/// ```json
/// { "fmt": "ulid", "value": "01HQRZ8YV9XW6E2K8N9PJ4QF7M" }
/// ```
///
/// Wrapping the format with the value lets `ocp-json/1` migrate to a different
/// identifier scheme without inventing a parallel field. The format tag is part
/// of the freeze; only the set of legal values may grow.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Identifier {
    /// The format tag (e.g., `ulid`).
    pub fmt: IdentifierFormat,
    /// The raw identifier string. For `ulid`, MUST match the ULID grammar after
    /// upper-casing.
    pub value: String,
}

impl Identifier {
    /// Construct a ULID-formatted identifier from a string. The caller is
    /// responsible for ensuring the value is a valid ULID.
    pub fn ulid(value: impl Into<String>) -> Self {
        Self {
            fmt: IdentifierFormat::Ulid,
            value: value.into(),
        }
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

// ---------------------------------------------------------------------------
// Timestamp
// ---------------------------------------------------------------------------

/// An RFC 3339 wall-clock timestamp with mandatory `Z` suffix and microsecond precision.
///
/// Wire form: `"2026-04-07T12:34:56.123456Z"`.
///
/// `Timestamp` is a transparent wrapper around the underlying string. The crate
/// does not parse it into a `chrono::DateTime` to avoid pulling chrono into the
/// frozen surface; downstream crates may convert as needed.
///
/// Producers MUST emit at least microsecond precision (six fractional digits).
/// Consumers MUST tolerate seven, eight, or nine fractional digits (nanoseconds)
/// per `wire-format-1.md` §9.1.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub String);

impl Timestamp {
    /// Wrap a raw RFC 3339 string. The caller is responsible for ensuring it
    /// matches the wire format pattern.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

// ---------------------------------------------------------------------------
// Duration
// ---------------------------------------------------------------------------

/// A protobuf-style duration: non-negative seconds plus a non-negative
/// `[0, 999_999_999]` nanos field.
///
/// Wire form:
/// ```json
/// { "seconds": 12345, "nanos": 678900000 }
/// ```
///
/// `nanos` MAY be omitted if zero; consumers MUST treat absence as zero. The
/// crate's `Default` for [`Duration`] gives zero seconds and zero nanos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Duration {
    /// Whole seconds, non-negative, within the IEEE 754 safe-integer range.
    pub seconds: u64,
    /// Sub-second nanoseconds in `[0, 999_999_999]`.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub nanos: u32,
}

fn is_zero_u32(value: &u32) -> bool {
    *value == 0
}

impl Duration {
    /// Construct a duration from raw `seconds` and `nanos`. The caller is
    /// responsible for ensuring `nanos < 1_000_000_000`.
    pub const fn new(seconds: u64, nanos: u32) -> Self {
        Self { seconds, nanos }
    }

    /// Construct a duration from whole seconds.
    pub const fn from_secs(seconds: u64) -> Self {
        Self { seconds, nanos: 0 }
    }
}

// ---------------------------------------------------------------------------
// ContentDigest
// ---------------------------------------------------------------------------

/// A tagged content digest carrying the hash algorithm alongside the hex digest.
///
/// Wire form (SHA-256 example):
/// ```json
/// { "algo": "sha-256", "digest": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08" }
/// ```
///
/// Wrapping the algorithm with the digest lets `ocp-json/1` add new hash
/// algorithms (e.g., `blake3`) without inventing a parallel field. The set of
/// legal `algo` values is part of `wire-format-1.md` §12.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentDigest {
    /// The hash algorithm tag (e.g., `sha-256`).
    pub algo: DigestAlgorithm,
    /// The lowercase hex digest. For `sha-256`, exactly 64 characters.
    pub digest: String,
}

/// The set of hash algorithms recognized by `ocp-json/1`.
///
/// Future minor releases may add new variants (e.g., `Blake3`); consumers MUST
/// tolerate unknown algorithms by preserving the raw tag verbatim via the
/// [`Other`](Self::Other) variant.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DigestAlgorithm {
    /// SHA-256 — the default. 64-character lowercase hex.
    Sha256,
    /// Any other algorithm. The original tag string is preserved verbatim so
    /// that it round-trips losslessly.
    Other(String),
}

impl DigestAlgorithm {
    /// Return the wire-form tag string (e.g., `"sha-256"`, or the preserved
    /// unknown tag).
    pub fn as_str(&self) -> &str {
        match self {
            Self::Sha256 => "sha-256",
            Self::Other(s) => s.as_str(),
        }
    }
}

impl Serialize for DigestAlgorithm {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DigestAlgorithm {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        Ok(match s.as_str() {
            "sha-256" => Self::Sha256,
            _ => Self::Other(s),
        })
    }
}

impl ContentDigest {
    /// Construct a SHA-256 digest from a 64-character lowercase hex string.
    pub fn sha256(digest: impl Into<String>) -> Self {
        Self {
            algo: DigestAlgorithm::Sha256,
            digest: digest.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// PathRef
// ---------------------------------------------------------------------------

/// A path reference variant. `PathRef` distinguishes between local filesystem
/// paths, URLs, run-relative paths, and content-addressed references.
///
/// Wire form is a tagged enum with a `kind` discriminant:
/// ```json
/// { "kind": "local", "path": "outputs/result.csv" }
/// { "kind": "url", "url": "https://example.com/data.json" }
/// { "kind": "run-relative", "path": "stage_01/result.json" }
/// { "kind": "content-addressed", "digest": { "algo": "sha-256", "digest": "..." } }
/// ```
///
/// The set of variants is part of the freeze. Future additions MUST extend this
/// enum additively and consumers tolerate unknown variants via the `Other`
/// fallthrough.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PathRef {
    /// A local filesystem path. Forward slashes only, even on Windows.
    Local {
        /// The path string. UTF-8, forward-slash separated.
        path: String,
    },
    /// A URL (typically `https://`).
    Url {
        /// The URL string.
        url: String,
    },
    /// A path relative to the current run's output directory.
    RunRelative {
        /// The relative path string. Forward slashes only.
        path: String,
    },
    /// A content-addressed reference resolved via the configured CAS store.
    ContentAddressed {
        /// The content digest identifying the blob.
        digest: ContentDigest,
    },
    /// An unknown variant. Preserved as opaque JSON for forward compatibility.
    #[serde(other)]
    Other,
}

// ---------------------------------------------------------------------------
// ToolRef
// ---------------------------------------------------------------------------

/// A reference to a plugin tool: family + name + version.
///
/// Wire form:
/// ```json
/// { "family": "maxdiff-turf", "name": "maxdiff-turf", "version": "1.2.3" }
/// ```
///
/// `ToolRef` is used inside [`crate::envelope::RunContext`] to identify the
/// plugin that produced an envelope, and inside composition envelopes to
/// describe parent / originating tools.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolRef {
    /// The tool's family identifier (typically the plugin name).
    pub family: String,
    /// The tool's binary name.
    pub name: String,
    /// The semver version string.
    pub version: String,
}

impl ToolRef {
    /// Construct a `ToolRef` from raw fields.
    pub fn new(
        family: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Self {
        Self {
            family: family.into(),
            name: name.into(),
            version: version.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn identifier_round_trips() {
        let id = Identifier::ulid("01HQRZ8YV9XW6E2K8N9PJ4QF7M");
        let json = serde_json::to_value(&id).unwrap();
        assert_eq!(json, json!({ "fmt": "ulid", "value": "01HQRZ8YV9XW6E2K8N9PJ4QF7M" }));
        let back: Identifier = serde_json::from_value(json).unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn identifier_unknown_format_round_trips() {
        let json = json!({ "fmt": "uuid", "value": "550e8400-e29b-41d4-a716-446655440000" });
        let parsed: Identifier = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(parsed.fmt, IdentifierFormat::Other("uuid".into()));
        assert_eq!(parsed.value, "550e8400-e29b-41d4-a716-446655440000");
        // Round-trip preserves the original tag verbatim.
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back, json);
    }

    #[test]
    fn duration_omits_zero_nanos() {
        let d = Duration::from_secs(42);
        let json = serde_json::to_value(d).unwrap();
        assert_eq!(json, json!({ "seconds": 42 }));
    }

    #[test]
    fn duration_round_trips_with_nanos() {
        let d = Duration::new(12345, 678_900_000);
        let json = serde_json::to_value(d).unwrap();
        assert_eq!(json, json!({ "seconds": 12345, "nanos": 678_900_000 }));
        let back: Duration = serde_json::from_value(json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn duration_accepts_missing_nanos() {
        let json = json!({ "seconds": 7 });
        let parsed: Duration = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, Duration::from_secs(7));
    }

    #[test]
    fn timestamp_is_transparent() {
        let ts = Timestamp::new("2026-04-07T12:34:56.123456Z");
        let json = serde_json::to_value(&ts).unwrap();
        assert_eq!(json, json!("2026-04-07T12:34:56.123456Z"));
    }

    #[test]
    fn content_digest_round_trips() {
        let d = ContentDigest::sha256(
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        );
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(
            json,
            json!({
                "algo": "sha-256",
                "digest": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
            })
        );
        let back: ContentDigest = serde_json::from_value(json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn path_ref_local_round_trips() {
        let p = PathRef::Local { path: "outputs/result.csv".into() };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json, json!({ "kind": "local", "path": "outputs/result.csv" }));
        let back: PathRef = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn path_ref_content_addressed_round_trips() {
        let p = PathRef::ContentAddressed {
            digest: ContentDigest::sha256("a".repeat(64)),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: PathRef = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn path_ref_unknown_kind_falls_through() {
        let json = json!({ "kind": "future-variant", "data": 42 });
        let parsed: PathRef = serde_json::from_value(json).unwrap();
        assert_eq!(parsed, PathRef::Other);
    }
}
