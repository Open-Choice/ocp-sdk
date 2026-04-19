//! Payload types for standard `response.*` kinds.
//!
//! Each type corresponds to a kind defined in `kinds-1.md` §4. Response
//! envelopes are emitted exactly once per inspection invocation, followed by
//! the plugin exiting.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{CostEstimate, ValidationIssue};
use crate::wire::Duration;

// ---------------------------------------------------------------------------
// ValidateResponsePayload
// ---------------------------------------------------------------------------

/// Payload for `response.validate`. Result of validating a params object
/// against a command schema.
///
/// `ok` is the top-line decision; the `issues` array carries individual
/// findings (errors, warnings, info) using the unified [`ValidationIssue`]
/// type. A response with `ok: true` MAY still carry warnings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidateResponsePayload {
    /// Whether the input is valid. `false` means at least one issue with
    /// `severity == error` is present.
    pub ok: bool,

    /// All validation issues, ordered by severity (errors first) then by
    /// location.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<ValidationIssue>,

    /// Optional cost estimate. Plugins that advertise the
    /// `validation.cost_estimate` capability SHOULD include this when `ok` is true.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_estimate: Option<CostEstimate>,

    /// Optional normalized parameters. Plugins MAY include the canonicalized
    /// form of the input here for the host to display or persist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_params: Option<Value>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// SelfTestResponsePayload
// ---------------------------------------------------------------------------

/// Status of an individual self-test check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfTestStatus {
    /// The check passed.
    Pass,
    /// The check failed.
    Fail,
    /// The check was skipped (preconditions unmet).
    Skipped,
    /// Unknown status. Preserved verbatim.
    #[serde(other)]
    Other,
}

/// A single self-test check result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelfTestCheck {
    /// Stable identifier for the check.
    pub id: String,

    /// Human-readable label.
    pub label: String,

    /// Status of the check.
    pub status: SelfTestStatus,

    /// Free-form message describing the result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// How long the check took to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<Duration>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `response.self_test`. Result of running the plugin's internal
/// health checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SelfTestResponsePayload {
    /// Whether all checks passed (and at least one ran).
    pub ok: bool,

    /// Individual check results.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<SelfTestCheck>,

    /// Total elapsed time across all checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<Duration>,

    /// Free-form summary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Severity;
    use serde_json::json;

    #[test]
    fn validate_response_round_trips() {
        let p = ValidateResponsePayload {
            ok: false,
            issues: vec![ValidationIssue {
                severity: Severity::Error,
                message: "k must be positive".into(),
                path: Some("/params/k".into()),
                code: Some("VAL.NEG".into()),
                hint: None,
                other: BTreeMap::new(),
            }],
            cost_estimate: None,
            normalized_params: None,
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: ValidateResponsePayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn validate_response_with_cost_estimate() {
        let p = ValidateResponsePayload {
            ok: true,
            issues: vec![],
            cost_estimate: Some(CostEstimate {
                estimated_duration: Some(Duration::from_secs(60)),
                estimated_peak_memory_bytes: Some(512 * 1024 * 1024),
                estimated_cpu_units: None,
                estimated_cost_usd: None,
                confidence: Some(0.9),
                notes: None,
                other: BTreeMap::new(),
            }),
            normalized_params: Some(json!({ "k": 5, "seed": 42 })),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: ValidateResponsePayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn self_test_response_round_trips() {
        let p = SelfTestResponsePayload {
            ok: true,
            checks: vec![SelfTestCheck {
                id: "rng".into(),
                label: "RNG seeding".into(),
                status: SelfTestStatus::Pass,
                message: None,
                elapsed: Some(Duration::new(0, 1_000_000)),
                other: BTreeMap::new(),
            }],
            elapsed: Some(Duration::new(0, 5_000_000)),
            summary: None,
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: SelfTestResponsePayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn self_test_status_unknown_falls_through() {
        let s: SelfTestStatus = serde_json::from_value(json!("flaky")).unwrap();
        assert_eq!(s, SelfTestStatus::Other);
    }
}
