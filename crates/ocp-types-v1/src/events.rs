//! Payload types for standard `event.*` kinds.
//!
//! Each type corresponds to a kind defined in `kinds-1.md` §3. Producers
//! construct one of these and place it in the envelope's `payload` field;
//! consumers deserialize the `payload` based on the envelope's `kind` after
//! confirming the kind is recognized.
//!
//! All payload types carry a `#[serde(flatten)] other` slot for forward-compat
//! preservation of fields added in future minor releases.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::common::{ArtifactRecord, ProgressMetric, Severity};
use crate::wire::{ContentDigest, Duration, PathRef, Timestamp};

// ---------------------------------------------------------------------------
// Run lifecycle
// ---------------------------------------------------------------------------

/// Payload for `event.run.started`. Marks the start of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStartedPayload {
    /// The seed used by the plugin's RNG, when applicable. Strings are used
    /// rather than integers to permit seeds outside the safe-integer range
    /// (per `wire-format-1.md` §4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<String>,

    /// The output directory the plugin will write artifacts into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_dir: Option<PathRef>,

    /// The full command-line invocation, as the plugin observed it. Useful for
    /// audit and reproducibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,

    /// Optional human-readable label for the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `event.run.heartbeat`. Liveness ping with no other data.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunHeartbeatPayload {
    /// Optional time elapsed since `event.run.started`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<Duration>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `event.run.progress`. Iteration / phase progress update.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunProgressPayload {
    /// Number of iterations completed so far. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iter_completed: Option<u64>,

    /// Total iterations targeted, when known. Optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iter_target: Option<u64>,

    /// A short label naming the current phase (e.g., `"warmup"`, `"sampling"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,

    /// Fraction complete in `[0.0, 1.0]`, for progress bars. Optional and
    /// independent of `iter_completed`/`iter_target`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f64>,

    /// Free-form metric samples for this progress tick.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<ProgressMetric>,

    /// Time elapsed since `event.run.started`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<Duration>,

    /// Estimated time remaining.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<Duration>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `event.run.finished`. Final envelope of a successful run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunFinishedPayload {
    /// Total elapsed time of the run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<Duration>,

    /// Final list of artifacts produced by the run. Consumers SHOULD reconcile
    /// this against accumulated `event.artifact.*` envelopes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRecord>,

    /// Free-form summary text rendered to the user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// Final metric values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<ProgressMetric>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `event.run.failed`. Final envelope of a failed run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunFailedPayload {
    /// A human-readable error message.
    pub error: String,

    /// Optional machine-readable error code. Vendors SHOULD namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,

    /// Optional structured cause chain (e.g., underlying I/O errors).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cause_chain: Vec<String>,

    /// Total elapsed time of the run before failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<Duration>,

    /// Any partial artifacts produced before the failure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partial_artifacts: Vec<ArtifactRecord>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `event.run.cancelled`. Final envelope of a cancelled run.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunCancelledPayload {
    /// Optional reason from the host's `control.cancel` envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Total elapsed time of the run before cancellation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<Duration>,

    /// Any partial artifacts produced before cancellation.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partial_artifacts: Vec<ArtifactRecord>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `event.run.paused`. Run is suspended awaiting `control.resume`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunPausedPayload {
    /// Optional reason from the host's `control.pause` envelope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `event.run.resumed`. Run resumed from a checkpoint or pause.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RunResumedPayload {
    /// The checkpoint id that was used for restart, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<String>,

    /// The iteration number at which the run resumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resumed_at_iter: Option<u64>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// Composition (stage events)
// ---------------------------------------------------------------------------

/// Payload for `event.stage.started`. A composition stage begins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageStartedPayload {
    /// The stage label, if not derivable from the envelope's
    /// [`RunContext::stage_id`](crate::envelope::RunContext::stage_id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// A short description of what this stage does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Outcome of a stage. New variants may be added in future minor releases;
/// the `Other` fall-through preserves unknown values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StageOutcome {
    /// Stage completed successfully.
    Success,
    /// Stage terminated with an error.
    Failure,
    /// Stage was skipped (preconditions unmet, etc.).
    Skipped,
    /// Stage was cancelled by the host.
    Cancelled,
    /// Unknown outcome. Preserved verbatim.
    #[serde(other)]
    Other,
}

/// Payload for `event.stage.finished`. A composition stage ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageFinishedPayload {
    /// The outcome of the stage.
    pub outcome: StageOutcome,

    /// Total elapsed time of the stage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed: Option<Duration>,

    /// Stage-level error message, present when `outcome` is `Failure`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Stage-level summary, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// Checkpoints
// ---------------------------------------------------------------------------

/// Payload for `event.checkpoint.committed`. A persistent checkpoint has been written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckpointCommittedPayload {
    /// Stable identifier for the checkpoint.
    pub checkpoint_id: String,

    /// Where the checkpoint was written.
    pub path: PathRef,

    /// The iteration number at which the checkpoint was committed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iter_committed: Option<u64>,

    /// The checkpoint's content digest, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<ContentDigest>,

    /// Whether this checkpoint represents an exact-restart point or an
    /// approximate one (matches the `restart.exact` / `restart.approximate`
    /// capability).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// Artifacts
// ---------------------------------------------------------------------------

/// Payload for `event.artifact.created`. A new artifact has been produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactCreatedPayload {
    /// The artifact record.
    pub artifact: ArtifactRecord,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `event.artifact.updated`. An existing artifact has been modified.
///
/// The `artifact.artifact_id` MUST match the `artifact_id` from the original
/// `event.artifact.created` envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactUpdatedPayload {
    /// The updated artifact record.
    pub artifact: ArtifactRecord,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// Payload for `event.message.warning` and `event.message.error`. The
/// envelope's `kind` discriminates the severity; the payload itself uses
/// [`Severity`] redundantly so that consumers parsing only the payload can
/// still classify it.
///
/// For terminal errors that DO terminate the run, use `event.run.failed`,
/// not `event.message.error`. See `kinds-1.md` §3.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessagePayload {
    /// The severity. Should match the envelope kind.
    pub severity: Severity,

    /// The message text.
    pub message: String,

    /// Optional machine-readable code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Optional path or context locator (e.g., file path, JSON pointer).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// Logs and metrics
// ---------------------------------------------------------------------------

/// Payload for `event.log.line`. A structured log line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLinePayload {
    /// Log severity (info, warning, error, etc.).
    pub severity: Severity,

    /// The log message text.
    pub message: String,

    /// Optional logger name (e.g., `"sampler"`, `"io"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logger: Option<String>,

    /// Optional structured fields attached to the log line.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,

    /// When the log line was produced. Optional; envelope `ts` is the fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<Timestamp>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// Payload for `event.metric`. A standalone metric sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricPayload {
    /// The metric.
    pub metric: ProgressMetric,

    /// When the metric was sampled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<Timestamp>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn run_started_round_trips() {
        let p = RunStartedPayload {
            seed: Some("42".into()),
            output_dir: Some(PathRef::Local { path: "out/".into() }),
            argv: vec!["exe".into(), "run".into(), "task.tmp".into()],
            label: Some("first run".into()),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: RunStartedPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn run_progress_round_trips_with_metrics() {
        let p = RunProgressPayload {
            iter_completed: Some(100),
            iter_target: Some(1000),
            phase: Some("sampling".into()),
            fraction: Some(0.1),
            metrics: vec![ProgressMetric {
                name: "acceptance_rate".into(),
                value: 0.42,
                unit: None,
                min: Some(0.0),
                max: Some(1.0),
                other: BTreeMap::new(),
            }],
            elapsed: Some(Duration::from_secs(30)),
            remaining: Some(Duration::from_secs(270)),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: RunProgressPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn run_progress_preserves_unknown_fields() {
        let json = json!({
            "iter_completed": 50,
            "future_progress_field": { "extra": true }
        });
        let p: RunProgressPayload = serde_json::from_value(json.clone()).unwrap();
        assert!(p.other.contains_key("future_progress_field"));
        let back = serde_json::to_value(&p).unwrap();
        assert_eq!(back["future_progress_field"], json["future_progress_field"]);
    }

    #[test]
    fn run_finished_round_trips() {
        let p = RunFinishedPayload {
            elapsed: Some(Duration::from_secs(120)),
            artifacts: vec![ArtifactRecord {
                artifact_id: "01HQRZ8YV9XW6E2K8N9PJ4QF7M".into(),
                path: PathRef::RunRelative { path: "result.csv".into() },
                kind: "result.csv".into(),
                media_type: Some("text/csv".into()),
                digest: None,
                size_bytes: Some(1024),
                created_at: None,
                modified_at: None,
                label: None,
                description: None,
                ext: BTreeMap::new(),
                other: BTreeMap::new(),
            }],
            summary: Some("done".into()),
            metrics: vec![],
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: RunFinishedPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn stage_outcome_unknown_falls_through() {
        let s: StageOutcome = serde_json::from_value(json!("rolled_back")).unwrap();
        assert_eq!(s, StageOutcome::Other);
    }

    #[test]
    fn checkpoint_committed_round_trips() {
        let p = CheckpointCommittedPayload {
            checkpoint_id: "ckpt_001".into(),
            path: PathRef::RunRelative { path: "checkpoints/ckpt_001.bin".into() },
            iter_committed: Some(500),
            digest: Some(ContentDigest::sha256("b".repeat(64))),
            exact: Some(true),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: CheckpointCommittedPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn message_payload_round_trips() {
        let p = MessagePayload {
            severity: Severity::Warning,
            message: "deprecated option used".into(),
            code: Some("DEPR.001".into()),
            locator: Some("/params/legacy_flag".into()),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: MessagePayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn log_line_round_trips() {
        let mut fields = BTreeMap::new();
        fields.insert("iter".into(), json!(42));
        let p = LogLinePayload {
            severity: Severity::Info,
            message: "iteration complete".into(),
            logger: Some("sampler".into()),
            fields,
            ts: Some(Timestamp::new("2026-04-07T12:34:56.123456Z")),
            other: BTreeMap::new(),
        };
        let json = serde_json::to_value(&p).unwrap();
        let back: LogLinePayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }
}
