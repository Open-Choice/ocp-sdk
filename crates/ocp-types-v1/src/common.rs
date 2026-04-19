//! Shared types referenced by multiple payload modules.
//!
//! These types appear inside event, response, and control payloads. They
//! are part of the frozen surface and follow the same forward-compat rules
//! as the envelope and primitives: every type carries an `other` slot for
//! preserving unknown fields, and all enums use `#[serde(other)]` fallthrough.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::wire::{ContentDigest, Duration, PathRef, Timestamp};

// ---------------------------------------------------------------------------
// Severity
// ---------------------------------------------------------------------------

/// Severity level used by [`ValidationIssue`] and [`MessagePayload`](crate::events::MessagePayload).
///
/// New severity levels MAY be added in future minor releases. Consumers MUST
/// tolerate unknown variants by treating them as the closest known fallback;
/// the `Other` variant captures any unrecognized severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Informational; no user action required.
    Info,
    /// Non-fatal warning the user should see.
    Warning,
    /// Recoverable error that did not terminate the run.
    Error,
    /// Unknown severity. Preserved verbatim during round-trip; future minor
    /// releases may add new variants.
    #[serde(other)]
    Other,
}

// ---------------------------------------------------------------------------
// ValidationIssue
// ---------------------------------------------------------------------------

/// A single validation issue produced by `api validate` or by inline run-time
/// validation.
///
/// `ValidationIssue` unifies what older drafts called `ValidationError` and
/// `ValidationWarning` into a single type discriminated by `severity`. This
/// is the recommended shape for any validation feedback in `ocp-json/1`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// The severity of the issue.
    pub severity: Severity,

    /// A human-readable message describing the issue.
    pub message: String,

    /// JSON Pointer (RFC 6901) into the input document, identifying the
    /// offending location. Empty string for issues that apply to the whole input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Optional machine-readable error code. Vendors SHOULD namespace their
    /// codes (`<vendor>.<code>`) to avoid collisions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Optional hint for how to fix the issue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,

    /// Forward-compat slot for unknown fields.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ArtifactRecord
// ---------------------------------------------------------------------------

/// A record describing an artifact produced by a run.
///
/// Artifacts are correlated by [`artifact_id`](Self::artifact_id), not by path.
/// An artifact updated mid-run via `event.artifact.updated` MUST reuse the
/// `artifact_id` from the original `event.artifact.created`. See `kinds-1.md` §3.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    /// Stable identifier for this artifact across creation and updates. ULID
    /// recommended; see `wire-format-1.md` §10.
    pub artifact_id: String,

    /// Where the artifact lives.
    pub path: PathRef,

    /// The artifact kind, drawn from `kinds-1.md` §8 (e.g., `result.csv`,
    /// `summary.md`, or a vendor-namespaced kind like `maxdiff-turf.curve_csv`).
    pub kind: String,

    /// The IANA media type (e.g., `text/csv`, `application/json`). Optional;
    /// hosts MAY infer from `kind` when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// The artifact's content digest, when known. Optional during streaming
    /// updates; final `event.run.finished` envelopes SHOULD include digests
    /// for all artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<ContentDigest>,

    /// The artifact's size in bytes, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,

    /// When the artifact was created (or first observed by the producer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<Timestamp>,

    /// When the artifact was last modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<Timestamp>,

    /// Optional human-readable label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Vendor extension bag for artifact-level metadata.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: BTreeMap<String, Value>,

    /// Forward-compat slot for unknown fields.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// OutputDescriptor
// ---------------------------------------------------------------------------

/// Static description of an output that a command may produce. Used inside
/// `outputs/<command>.json` static assets and in some response payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputDescriptor {
    /// The output kind, drawn from `kinds-1.md` §8.
    pub kind: String,

    /// IANA media type, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    /// Whether this output is always produced (`true`) or only conditionally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub always_produced: Option<bool>,

    /// Human-readable label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ProgressMetric
// ---------------------------------------------------------------------------

/// A single named metric value used inside progress and metric payloads.
///
/// `ProgressMetric` is a structured alternative to free-form JSON: it has a
/// name, a numeric value, optional unit, and optional bounds for rendering as
/// a gauge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressMetric {
    /// The metric name. Vendors SHOULD namespace (`<vendor>.<name>`) for
    /// non-standard metrics.
    pub name: String,

    /// The metric's current value.
    pub value: f64,

    /// Optional unit string (e.g., `"ms"`, `"iterations"`, `"%"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,

    /// Optional minimum value for gauge rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,

    /// Optional maximum value for gauge rendering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// CostEstimate
// ---------------------------------------------------------------------------

/// Anticipated cost of a run, attached to validation responses by plugins
/// that advertise the `validation.cost_estimate` capability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostEstimate {
    /// Estimated wall-clock duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_duration: Option<Duration>,

    /// Estimated peak memory in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_peak_memory_bytes: Option<u64>,

    /// Estimated CPU cost in arbitrary units (e.g., FLOPs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cpu_units: Option<f64>,

    /// Estimated cost in monetary units, when applicable (e.g., for cloud-backed plugins).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,

    /// Confidence in the estimate, in `[0.0, 1.0]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,

    /// Free-form notes explaining the basis of the estimate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validation_issue_round_trips() {
        let issue = ValidationIssue {
            severity: Severity::Error,
            message: "field 'k' must be positive".into(),
            path: Some("/params/k".into()),
            code: Some("VAL.NEGATIVE".into()),
            hint: Some("set k >= 1".into()),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&issue).unwrap();
        let back: ValidationIssue = serde_json::from_value(json).unwrap();
        assert_eq!(back, issue);
    }

    #[test]
    fn validation_issue_preserves_unknown_fields() {
        let json = json!({
            "severity": "warning",
            "message": "deprecated option",
            "future_field": { "introduced_in": "1.5.0" }
        });
        let parsed: ValidationIssue = serde_json::from_value(json.clone()).unwrap();
        assert!(parsed.other.contains_key("future_field"));
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(back["future_field"], json["future_field"]);
    }

    #[test]
    fn severity_unknown_falls_through() {
        let json = json!("catastrophic");
        let s: Severity = serde_json::from_value(json).unwrap();
        assert_eq!(s, Severity::Other);
    }

    #[test]
    fn artifact_record_round_trips() {
        let rec = ArtifactRecord {
            artifact_id: "01HQRZ8YV9XW6E2K8N9PJ4QF7M".into(),
            path: PathRef::RunRelative { path: "outputs/result.csv".into() },
            kind: "result.csv".into(),
            media_type: Some("text/csv".into()),
            digest: Some(ContentDigest::sha256("a".repeat(64))),
            size_bytes: Some(2048),
            created_at: Some(Timestamp::new("2026-04-07T12:34:56.123456Z")),
            modified_at: None,
            label: Some("Result".into()),
            description: None,
            ext: BTreeMap::new(),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&rec).unwrap();
        let back: ArtifactRecord = serde_json::from_value(json).unwrap();
        assert_eq!(back, rec);
    }

    #[test]
    fn cost_estimate_round_trips_with_only_some_fields() {
        let est = CostEstimate {
            estimated_duration: Some(Duration::from_secs(120)),
            estimated_peak_memory_bytes: None,
            estimated_cpu_units: None,
            estimated_cost_usd: None,
            confidence: Some(0.8),
            notes: None,
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&est).unwrap();
        // Optional fields should not appear when None.
        assert!(json.get("estimated_peak_memory_bytes").is_none());
        let back: CostEstimate = serde_json::from_value(json).unwrap();
        assert_eq!(back, est);
    }
}
