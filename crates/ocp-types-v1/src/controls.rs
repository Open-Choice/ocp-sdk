//! Payload types for standard `control.*` kinds.
//!
//! Each type corresponds to a kind defined in `kinds-1.md` §6. Control
//! envelopes are sent by the host to the plugin via stdin during a `run`,
//! gated by capability flags from `capabilities-1.md` §5.1.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::wire::Duration;

// ---------------------------------------------------------------------------
// ControlCancelPayload
// ---------------------------------------------------------------------------

/// Payload for `control.cancel`. Stop the current run cleanly.
///
/// Gated by the `control.cancel` capability. See `kinds-1.md` §6.1 for the
/// cancellation semantics: plugins MUST emit `event.run.cancelled` as the
/// terminal event and exit with status 0.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ControlCancelPayload {
    /// Optional human-readable reason for the cancellation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Soft deadline for cancellation. The plugin SHOULD complete its
    /// cancellation within this duration; after which the host MAY send
    /// SIGTERM/SIGKILL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<Duration>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ControlPausePayload
// ---------------------------------------------------------------------------

/// Payload for `control.pause`. Suspend the current run.
///
/// Gated by the `control.pause` capability. The plugin MUST emit
/// `event.run.paused` once it has actually suspended its work loop.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ControlPausePayload {
    /// Optional human-readable reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ControlResumePayload
// ---------------------------------------------------------------------------

/// Payload for `control.resume`. Resume a paused run.
///
/// Gated by the `control.pause` capability (pause and resume are bundled).
/// The plugin MUST emit `event.run.resumed` once it has actually resumed.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ControlResumePayload {
    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ControlHeartbeatPayload
// ---------------------------------------------------------------------------

/// Payload for `control.heartbeat`. Liveness check from host.
///
/// Gated by the `stdin.control_channel` capability. The plugin is not required
/// to acknowledge heartbeats; the host uses them only to verify the plugin is
/// still reading from stdin.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ControlHeartbeatPayload {
    /// Optional sequence number for heartbeat correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ControlDeadlinePayload
// ---------------------------------------------------------------------------

/// Payload for `control.deadline.extend`. Push back a soft deadline.
///
/// Gated by the `control.deadline` capability. The plugin SHOULD treat the
/// new deadline as advisory; failure to meet it does not require any specific
/// terminal event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlDeadlinePayload {
    /// The new deadline relative to now.
    pub new_deadline: Duration,

    /// Optional human-readable reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_round_trips() {
        let p = ControlCancelPayload {
            reason: Some("user requested".into()),
            deadline: Some(Duration::from_secs(5)),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: ControlCancelPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn cancel_minimal_round_trips() {
        let p = ControlCancelPayload::default();
        let json = serde_json::to_value(&p).unwrap();
        // Both optional fields should be skipped when None.
        assert_eq!(json, serde_json::json!({}));
        let back: ControlCancelPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn deadline_extend_round_trips() {
        let p = ControlDeadlinePayload {
            new_deadline: Duration::from_secs(300),
            reason: Some("user extended".into()),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: ControlDeadlinePayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn heartbeat_preserves_unknown_fields() {
        let json = serde_json::json!({ "seq": 42, "future": "preserved" });
        let p: ControlHeartbeatPayload = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(p.seq, Some(42));
        assert!(p.other.contains_key("future"));
    }
}
