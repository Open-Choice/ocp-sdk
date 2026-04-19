//! The universal `ocp-json/1` envelope.
//!
//! Every NDJSON line emitted on the wire is exactly one [`Envelope`]. The
//! envelope shape is **frozen** as of `ocp-types-1.0.0` and is governed by
//! `envelope-1.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kind::Kind;
use crate::wire::{Identifier, Timestamp, ToolRef};

// ---------------------------------------------------------------------------
// EnvelopeClass
// ---------------------------------------------------------------------------

/// The four envelope classes defined by `ocp-json/1`. The class field MUST
/// match the kind's leading segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EnvelopeClass {
    /// `event.*` envelopes — emitted by plugins during a `run`.
    Event,
    /// `response.*` envelopes — emitted exactly once per inspection invocation.
    Response,
    /// `request.*` envelopes — reserved for future use.
    Request,
    /// `control.*` envelopes — sent by the host to the plugin via stdin.
    Control,
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// The universal envelope. Every NDJSON line on the wire is one of these.
///
/// Wire form (event example):
/// ```json
/// {
///   "ocp": "1",
///   "class": "event",
///   "id": { "fmt": "ulid", "value": "01HQRZ8YV9XW6E2K8N9PJ4QF7M" },
///   "ts": "2026-04-07T12:34:56.123456Z",
///   "kind": "event.run.started",
///   "run": { "run_id": { "fmt": "ulid", "value": "01HQRZ..." }, ... },
///   "payload": { ... },
///   "ext": { ... }
/// }
/// ```
///
/// # Forward compatibility
///
/// The `payload` field is intentionally typed as `serde_json::Value` rather than
/// a strongly-typed union of payload types. This is the load-bearing forward-compat
/// property: a consumer that doesn't recognize a kind can still parse the envelope,
/// inspect its `class`, `kind`, and `id`, and round-trip it without loss. Strongly
/// typed payloads live in higher layers (`events`, `responses`, `controls` modules,
/// added in subsequent releases) and are accessed by deserializing the `payload`
/// field into the appropriate type once the kind is recognized.
///
/// The `other` field captures any unknown top-level fields via `#[serde(flatten)]`.
/// This is the per-envelope forward-compat slot for fields added in future minor
/// releases that this consumer's version of `ocp-types-v1` doesn't yet know about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    /// The wire format version. MUST be the literal string `"1"` for the entire
    /// lifetime of `ocp-json/1`. Future incompatible wire formats will use `"2"`.
    pub ocp: String,

    /// The envelope class. Always matches `kind`'s leading segment.
    pub class: EnvelopeClass,

    /// A unique identifier for this envelope. Used for deduplication and
    /// correlation. ULID by default.
    pub id: Identifier,

    /// The wall-clock timestamp at which this envelope was produced.
    pub ts: Timestamp,

    /// The kind. The leading segment matches `class`.
    pub kind: Kind,

    /// The run context — present on every event envelope, optional on response
    /// and control envelopes that are not associated with a specific run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunContext>,

    /// The payload, kind-specific. Higher layers parse this into a typed value
    /// once the kind is recognized.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,

    /// The vendor extension bag. See [`Ext`] and `envelope-1.md` §6.
    #[serde(default, skip_serializing_if = "Ext::is_empty")]
    pub ext: Ext,

    /// Forward-compatibility slot. Captures any top-level field that this
    /// version of `ocp-types-v1` doesn't recognize, so it round-trips losslessly.
    /// Future minor releases of `ocp-json/1` may add new top-level fields that
    /// older consumers preserve here without understanding them.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

impl Envelope {
    /// Construct an envelope with the minimum required fields. Use builder
    /// methods (added later) for typical producer paths.
    pub fn new(class: EnvelopeClass, id: Identifier, ts: Timestamp, kind: Kind) -> Self {
        Self {
            ocp: "1".to_string(),
            class,
            id,
            ts,
            kind,
            run: None,
            payload: None,
            ext: Ext::default(),
            other: BTreeMap::new(),
        }
    }

    /// Returns true if this envelope's `ocp` field matches the expected `"1"`
    /// value. Consumers SHOULD reject envelopes that fail this check.
    pub fn is_ocp_v1(&self) -> bool {
        self.ocp == "1"
    }
}

// ---------------------------------------------------------------------------
// RunContext
// ---------------------------------------------------------------------------

/// The composition envelope sub-object. Carries everything a consumer needs to
/// place an envelope in its run, parent run, and stage hierarchy.
///
/// `RunContext` is the load-bearing structure for composition wrappers. A
/// wrapper plugin emits envelopes whose `RunContext.parent_run_id` points to
/// its caller, whose `run_chain` lists every ancestor, and whose `stage_id`
/// identifies which internal phase produced the envelope. See `envelope-1.md`
/// §5 and `kinds-1.md` §3.2.
///
/// # Forward compatibility
///
/// New optional fields will be added in future minor releases. Consumers MUST
/// tolerate their absence (handled by `#[serde(default)]`) and MUST preserve
/// unknown fields via `other`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunContext {
    /// The current run's identifier. Required on every event envelope.
    pub run_id: Identifier,

    /// The task identifier from the `.scaffolding` task file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    /// The parent run's identifier, when this run was launched by another run
    /// (composition wrapper case). `None` for top-level runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_run_id: Option<Identifier>,

    /// The full chain of ancestor runs, ordered from outermost to immediate
    /// parent. Empty for top-level runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub run_chain: Vec<RunChainEntry>,

    /// The stage identifier within the current wrapper, when applicable. See
    /// `wire-format-1.md` §10.3 for the grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<String>,

    /// The originating tool — the tool that produced the *content* of this
    /// envelope, which may differ from the current run's tool when a wrapper
    /// relays a child's events upstream (`composition.relay` capability).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub originating_tool: Option<ToolRef>,

    /// The current run's tool. Required.
    pub tool: ToolRef,

    /// Free-form metadata attached to the run. Vendors SHOULD use [`Ext`] for
    /// extensions; this field is reserved for host-injected metadata such as
    /// user labels, scheduling tags, and provenance hints.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub run_metadata: BTreeMap<String, Value>,

    /// Forward-compatibility slot. Captures any unknown fields added by future
    /// minor releases.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// A single entry in the [`RunContext::run_chain`]. Identifies an ancestor run
/// by its id and tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunChainEntry {
    /// The ancestor's run identifier.
    pub run_id: Identifier,
    /// The ancestor's tool reference.
    pub tool: ToolRef,
    /// The ancestor's stage id within its own parent, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Ext
// ---------------------------------------------------------------------------

/// The vendor extension bag.
///
/// Wire form: a JSON object whose keys are vendor-namespaced (`<vendor>.<field>`)
/// and whose values are arbitrary JSON. See `envelope-1.md` §6 for the namespacing
/// grammar.
///
/// `Ext` lives at the top of the envelope and inside `RunContext`, payloads, and
/// any other type that needs vendor extensibility. Consumers MUST round-trip the
/// full `ext` object even when they don't recognize any of its keys.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Ext(pub BTreeMap<String, Value>);

impl Ext {
    /// Returns true if there are no extension keys.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Borrow the underlying map.
    pub fn as_map(&self) -> &BTreeMap<String, Value> {
        &self.0
    }

    /// Mutably borrow the underlying map.
    pub fn as_map_mut(&mut self) -> &mut BTreeMap<String, Value> {
        &mut self.0
    }

    /// Insert a vendor-namespaced key. Caller is responsible for ensuring the
    /// key matches the `<vendor>.<field>` grammar.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        self.0.insert(key.into(), value);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::IdentifierFormat;
    use serde_json::json;

    fn sample_envelope() -> Envelope {
        let mut env = Envelope::new(
            EnvelopeClass::Event,
            Identifier::ulid("01HQRZ8YV9XW6E2K8N9PJ4QF7M"),
            Timestamp::new("2026-04-07T12:34:56.123456Z"),
            Kind::parse("event.run.started").unwrap(),
        );
        env.run = Some(RunContext {
            run_id: Identifier::ulid("01HQRZ8YV9XW6E2K8N9PJ4QF8N"),
            task_id: Some("calc_001".into()),
            parent_run_id: None,
            run_chain: Vec::new(),
            stage_id: None,
            originating_tool: None,
            tool: ToolRef::new("toy-calculator", "toy-calculator", "1.0.0"),
            run_metadata: BTreeMap::new(),
            other: BTreeMap::new(),
        });
        env.payload = Some(json!({ "seed": 42 }));
        env
    }

    #[test]
    fn envelope_round_trips() {
        let env = sample_envelope();
        let json = serde_json::to_value(&env).unwrap();
        let back: Envelope = serde_json::from_value(json).unwrap();
        assert_eq!(back, env);
    }

    #[test]
    fn envelope_preserves_unknown_top_level_fields() {
        let json = json!({
            "ocp": "1",
            "class": "event",
            "id": { "fmt": "ulid", "value": "01HQRZ8YV9XW6E2K8N9PJ4QF7M" },
            "ts": "2026-04-07T12:34:56.123456Z",
            "kind": "event.run.started",
            "run": {
                "run_id": { "fmt": "ulid", "value": "01HQRZ8YV9XW6E2K8N9PJ4QF8N" },
                "tool": { "family": "x", "name": "x", "version": "1.0.0" }
            },
            "future_field": { "version": 7, "data": [1, 2, 3] }
        });

        let env: Envelope = serde_json::from_value(json.clone()).unwrap();
        assert!(env.other.contains_key("future_field"));

        // Round-trip preserves the unknown field exactly.
        let back = serde_json::to_value(&env).unwrap();
        assert_eq!(back["future_field"], json["future_field"]);
    }

    #[test]
    fn run_context_preserves_unknown_fields() {
        let json = json!({
            "run_id": { "fmt": "ulid", "value": "01HQRZ8YV9XW6E2K8N9PJ4QF8N" },
            "tool": { "family": "x", "name": "x", "version": "1.0.0" },
            "future_run_field": "preserved"
        });
        let ctx: RunContext = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(ctx.other.get("future_run_field"), Some(&json!("preserved")));
        let back = serde_json::to_value(&ctx).unwrap();
        assert_eq!(back["future_run_field"], json!("preserved"));
    }

    #[test]
    fn ext_round_trips_vendor_keys() {
        let mut ext = Ext::default();
        ext.insert("maxdiff-pipeline.iter_count", json!(150));
        ext.insert("numerious-hb.acceptance_rate", json!(0.42));
        let serialized = serde_json::to_value(&ext).unwrap();
        assert_eq!(
            serialized,
            json!({
                "maxdiff-pipeline.iter_count": 150,
                "numerious-hb.acceptance_rate": 0.42
            })
        );
        let back: Ext = serde_json::from_value(serialized).unwrap();
        assert_eq!(back, ext);
    }

    #[test]
    fn empty_ext_is_skipped_in_serialization() {
        let env = sample_envelope();
        let json = serde_json::to_value(&env).unwrap();
        assert!(json.get("ext").is_none(), "empty ext should not be serialized");
    }

    #[test]
    fn identifier_format_is_lowercase_in_envelope() {
        let env = sample_envelope();
        let json = serde_json::to_value(&env).unwrap();
        assert_eq!(json["id"]["fmt"], json!("ulid"));
        assert_eq!(env.id.fmt, IdentifierFormat::Ulid);
    }
}
