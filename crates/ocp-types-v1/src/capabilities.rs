//! Capability registry types and standard capability constants.
//!
//! See `capabilities-1.md` for the full registry. This module exposes:
//!
//! - [`Capability`] — a wrapper type around the raw capability identifier
//!   string with grammar validation.
//! - The [`standard`] submodule — string constants for every standard
//!   capability defined in `capabilities-1.md` §5.
//! - [`validate_capability_set`] — checks the dependency closure rules in
//!   `capabilities-1.md` §6.

use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Capability
// ---------------------------------------------------------------------------

/// A capability identifier.
///
/// Wire form: a JSON string like `"control.cancel"` or `"events.progress"`.
/// The crate stores it as a `String` to round-trip unknown future capabilities
/// verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Capability(String);

/// Errors produced when parsing a capability identifier against the grammar in
/// `capabilities-1.md` §3.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CapabilityParseError {
    /// The identifier was empty.
    #[error("capability is empty")]
    Empty,
    /// The identifier exceeded 128 bytes.
    #[error("capability exceeds 128 bytes")]
    TooLong,
    /// The identifier had no segment after the namespace.
    #[error("capability '{0}' has no feature segment after the namespace")]
    MissingFeature(String),
    /// A segment did not match `^[a-z][a-z0-9_]*$`.
    #[error("capability '{cap}' has invalid segment '{segment}'")]
    InvalidSegment {
        /// The full capability string.
        cap: String,
        /// The offending segment.
        segment: String,
    },
}

impl Capability {
    /// Construct a capability identifier from a raw string, validating it
    /// against the grammar in `capabilities-1.md` §3.
    pub fn parse(s: impl Into<String>) -> Result<Self, CapabilityParseError> {
        let s = s.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    /// Construct a capability identifier without validation. Useful for
    /// preserving unknown future identifiers during round-trip.
    pub fn from_raw(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// The full capability string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The namespace (first segment).
    pub fn namespace(&self) -> Option<&str> {
        self.0.split_once('.').map(|(ns, _)| ns)
    }

    /// Whether this capability is one of the reserved standard namespaces.
    pub fn is_standard_namespace(&self) -> bool {
        matches!(
            self.namespace(),
            Some(
                "control"
                    | "events"
                    | "composition"
                    | "restart"
                    | "bundled"
                    | "stdin"
                    | "outputs"
                    | "validation"
                    | "params"
            )
        )
    }

    fn validate(s: &str) -> Result<(), CapabilityParseError> {
        if s.is_empty() {
            return Err(CapabilityParseError::Empty);
        }
        if s.len() > 128 {
            return Err(CapabilityParseError::TooLong);
        }
        let mut iter = s.split('.');
        let first = iter.next().ok_or(CapabilityParseError::Empty)?;
        if !is_valid_segment(first) {
            return Err(CapabilityParseError::InvalidSegment {
                cap: s.to_string(),
                segment: first.to_string(),
            });
        }
        let mut saw_feature = false;
        for segment in iter {
            saw_feature = true;
            if !is_valid_segment(segment) {
                return Err(CapabilityParseError::InvalidSegment {
                    cap: s.to_string(),
                    segment: segment.to_string(),
                });
            }
        }
        if !saw_feature {
            return Err(CapabilityParseError::MissingFeature(s.to_string()));
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

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Capability {
    type Err = CapabilityParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s.to_string())
    }
}

// ---------------------------------------------------------------------------
// Standard capability constants
// ---------------------------------------------------------------------------

/// String constants for every standard capability in `capabilities-1.md` §5.
///
/// Use these instead of inline string literals when constructing manifests
/// or matching against advertised capabilities. Renaming any constant in this
/// module is forbidden by the freeze rule.
pub mod standard {
    // control.*
    /// `control.cancel` — accepts `control.cancel` envelopes.
    pub const CONTROL_CANCEL: &str = "control.cancel";
    /// `control.pause` — accepts `control.pause` and `control.resume` envelopes.
    pub const CONTROL_PAUSE: &str = "control.pause";
    /// `control.deadline` — honors `control.deadline.extend` envelopes.
    pub const CONTROL_DEADLINE: &str = "control.deadline";

    // events.*
    /// `events.heartbeat` — emits `event.run.heartbeat`.
    pub const EVENTS_HEARTBEAT: &str = "events.heartbeat";
    /// `events.progress` — emits `event.run.progress`.
    pub const EVENTS_PROGRESS: &str = "events.progress";
    /// `events.metric` — emits standalone `event.metric`.
    pub const EVENTS_METRIC: &str = "events.metric";
    /// `events.log_line` — emits `event.log.line` for structured logging.
    pub const EVENTS_LOG_LINE: &str = "events.log_line";
    /// `events.artifact_updates` — may emit `event.artifact.updated`.
    pub const EVENTS_ARTIFACT_UPDATES: &str = "events.artifact_updates";
    /// `events.stage` — emits `event.stage.*` (composition wrappers only).
    pub const EVENTS_STAGE: &str = "events.stage";

    // composition.*
    /// `composition.wrapper` — internally invokes other plugins.
    pub const COMPOSITION_WRAPPER: &str = "composition.wrapper";
    /// `composition.nested` — children may themselves be composition wrappers.
    pub const COMPOSITION_NESTED: &str = "composition.nested";
    /// `composition.parallel` — runs multiple child plugins concurrently.
    pub const COMPOSITION_PARALLEL: &str = "composition.parallel";
    /// `composition.relay` — forwards child events upstream rather than aggregating.
    pub const COMPOSITION_RELAY: &str = "composition.relay";

    // restart.*
    /// `restart.exact` — bit-identical resume from checkpoint.
    pub const RESTART_EXACT: &str = "restart.exact";
    /// `restart.approximate` — statistically equivalent resume from checkpoint.
    pub const RESTART_APPROXIMATE: &str = "restart.approximate";

    // bundled.*
    /// `bundled.components` — ships and invokes other plugin binaries internally.
    pub const BUNDLED_COMPONENTS: &str = "bundled.components";
    /// `bundled.components.attested` — bundled components have per-component SHA-256.
    pub const BUNDLED_COMPONENTS_ATTESTED: &str = "bundled.components.attested";

    // stdin.*
    /// `stdin.control_channel` — reads NDJSON control envelopes from stdin.
    pub const STDIN_CONTROL_CHANNEL: &str = "stdin.control_channel";

    // outputs.*
    /// `outputs.streaming` — may create artifacts whose files are still being written.
    pub const OUTPUTS_STREAMING: &str = "outputs.streaming";
    /// `outputs.content_addressed` — uses `PathRef::ContentAddressed` for some outputs.
    pub const OUTPUTS_CONTENT_ADDRESSED: &str = "outputs.content_addressed";
    /// `outputs.deterministic` — repeated runs with same seed produce byte-identical files.
    pub const OUTPUTS_DETERMINISTIC: &str = "outputs.deterministic";

    // validation.*
    /// `validation.dry_run` — `api validate` performs full input parsing and validation.
    pub const VALIDATION_DRY_RUN: &str = "validation.dry_run";
    /// `validation.cost_estimate` — `ValidateResponsePayload` includes cost estimate.
    pub const VALIDATION_COST_ESTIMATE: &str = "validation.cost_estimate";
    /// `validation.warnings` — `ValidateResponsePayload` may include non-blocking warnings.
    pub const VALIDATION_WARNINGS: &str = "validation.warnings";

    // params.*
    /// `params.normalization` — emits a `params.json` artifact with normalized params.
    pub const PARAMS_NORMALIZATION: &str = "params.normalization";
    /// `params.echo` — emits an `input.echo` artifact with the original input.
    pub const PARAMS_ECHO: &str = "params.echo";
    /// `params.defaults_documented` — static `examples/` and `help/` describe every default.
    pub const PARAMS_DEFAULTS_DOCUMENTED: &str = "params.defaults_documented";
}

// ---------------------------------------------------------------------------
// Dependency closure validation
// ---------------------------------------------------------------------------

/// An error produced by [`validate_capability_set`] when a capability set
/// violates the dependency closure rules in `capabilities-1.md` §6.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CapabilityClosureError {
    /// A capability requires another capability that is not present.
    #[error("capability '{requirer}' requires '{required}' but it is not declared")]
    MissingDependency {
        /// The capability that has the dependency.
        requirer: String,
        /// The required capability that is missing.
        required: String,
    },
    /// `restart.exact` and `restart.approximate` are mutually exclusive.
    #[error("'restart.exact' and 'restart.approximate' are mutually exclusive")]
    MutuallyExclusiveRestart,
    /// `events.stage` was declared without any `composition.*` capability.
    #[error("'events.stage' requires at least one 'composition.*' capability")]
    StageWithoutComposition,
}

/// Validate that a set of capabilities satisfies the dependency closure rules
/// from `capabilities-1.md` §6.
///
/// This is what hosts call at plugin load time. Plugins MAY call it during
/// development to verify their manifest.
pub fn validate_capability_set(caps: &[Capability]) -> Result<(), CapabilityClosureError> {
    let set: HashSet<&str> = caps.iter().map(|c| c.as_str()).collect();

    // Pairwise dependencies.
    let deps: &[(&str, &str)] = &[
        (standard::CONTROL_CANCEL, standard::STDIN_CONTROL_CHANNEL),
        (standard::CONTROL_PAUSE, standard::STDIN_CONTROL_CHANNEL),
        (standard::CONTROL_DEADLINE, standard::STDIN_CONTROL_CHANNEL),
        (standard::COMPOSITION_PARALLEL, standard::COMPOSITION_WRAPPER),
        (standard::COMPOSITION_NESTED, standard::COMPOSITION_WRAPPER),
        (standard::COMPOSITION_RELAY, standard::COMPOSITION_WRAPPER),
        (standard::BUNDLED_COMPONENTS_ATTESTED, standard::BUNDLED_COMPONENTS),
    ];
    for (requirer, required) in deps {
        if set.contains(requirer) && !set.contains(required) {
            return Err(CapabilityClosureError::MissingDependency {
                requirer: (*requirer).into(),
                required: (*required).into(),
            });
        }
    }

    // Mutual exclusion: restart.exact vs restart.approximate.
    if set.contains(standard::RESTART_EXACT) && set.contains(standard::RESTART_APPROXIMATE) {
        return Err(CapabilityClosureError::MutuallyExclusiveRestart);
    }

    // events.stage requires at least one composition.* capability.
    if set.contains(standard::EVENTS_STAGE) {
        let has_composition = set.iter().any(|c| c.starts_with("composition."));
        if !has_composition {
            return Err(CapabilityClosureError::StageWithoutComposition);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(items: &[&str]) -> Vec<Capability> {
        items.iter().map(|s| Capability::parse(*s).unwrap()).collect()
    }

    #[test]
    fn parses_standard_capability() {
        let c = Capability::parse("control.cancel").unwrap();
        assert_eq!(c.namespace(), Some("control"));
        assert!(c.is_standard_namespace());
    }

    #[test]
    fn parses_vendor_capability() {
        let c = Capability::parse("maxdiff_pipeline.curve_family_v2").unwrap();
        assert_eq!(c.namespace(), Some("maxdiff_pipeline"));
        assert!(!c.is_standard_namespace());
    }

    #[test]
    fn rejects_namespace_only() {
        let err = Capability::parse("control").unwrap_err();
        assert!(matches!(err, CapabilityParseError::MissingFeature(_)));
    }

    #[test]
    fn rejects_uppercase_segment() {
        let err = Capability::parse("Control.cancel").unwrap_err();
        assert!(matches!(err, CapabilityParseError::InvalidSegment { .. }));
    }

    #[test]
    fn closure_accepts_valid_set() {
        let caps = caps(&[
            "stdin.control_channel",
            "control.cancel",
            "events.progress",
        ]);
        assert!(validate_capability_set(&caps).is_ok());
    }

    #[test]
    fn closure_rejects_cancel_without_stdin() {
        let caps = caps(&["control.cancel"]);
        let err = validate_capability_set(&caps).unwrap_err();
        match err {
            CapabilityClosureError::MissingDependency { requirer, required } => {
                assert_eq!(requirer, "control.cancel");
                assert_eq!(required, "stdin.control_channel");
            }
            _ => panic!("expected MissingDependency"),
        }
    }

    #[test]
    fn closure_rejects_parallel_without_wrapper() {
        let caps = caps(&["composition.parallel"]);
        let err = validate_capability_set(&caps).unwrap_err();
        assert!(matches!(err, CapabilityClosureError::MissingDependency { .. }));
    }

    #[test]
    fn closure_rejects_both_restart_modes() {
        let caps = caps(&["restart.exact", "restart.approximate"]);
        let err = validate_capability_set(&caps).unwrap_err();
        assert_eq!(err, CapabilityClosureError::MutuallyExclusiveRestart);
    }

    #[test]
    fn closure_rejects_stage_without_composition() {
        let caps = caps(&["events.stage"]);
        let err = validate_capability_set(&caps).unwrap_err();
        assert_eq!(err, CapabilityClosureError::StageWithoutComposition);
    }

    #[test]
    fn closure_accepts_stage_with_wrapper() {
        let caps = caps(&["composition.wrapper", "events.stage"]);
        assert!(validate_capability_set(&caps).is_ok());
    }

    #[test]
    fn closure_rejects_attested_without_bundled() {
        let caps = caps(&["bundled.components.attested"]);
        let err = validate_capability_set(&caps).unwrap_err();
        assert!(matches!(err, CapabilityClosureError::MissingDependency { .. }));
    }

    #[test]
    fn capability_round_trips_via_serde() {
        let c = Capability::parse("events.progress").unwrap();
        let json = serde_json::to_value(&c).unwrap();
        assert_eq!(json, serde_json::json!("events.progress"));
        let back: Capability = serde_json::from_value(json).unwrap();
        assert_eq!(back, c);
    }
}
