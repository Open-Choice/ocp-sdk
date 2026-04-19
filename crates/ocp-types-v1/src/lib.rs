//! `ocp-types-v1` — canonical Rust implementation of `ocp-json/1`.
//!
//! This crate is the **frozen** type surface for the `ocp-json/1` protocol. Once
//! `ocp-types-1.0.0` is tagged, every public type, variant, and field becomes part
//! of the protocol contract and is governed by the rules in `CONTRIBUTING.md`.
//!
//! The crate is organized into layers that mirror the spec documents:
//!
//! - [`wire`] — wire-format primitives ([`Identifier`], [`Timestamp`], [`Duration`],
//!   [`ContentDigest`], [`PathRef`], [`ToolRef`]). See `wire-format-1.md`.
//! - [`envelope`] — the universal envelope ([`Envelope`], [`EnvelopeClass`],
//!   [`RunContext`], [`Ext`]). See `envelope-1.md`.
//! - [`kind`] — the kind grammar and parser ([`Kind`], [`KindClass`]). See `kinds-1.md`.
//! - [`common`] — types shared across multiple payload modules ([`ValidationIssue`],
//!   [`ArtifactRecord`], [`Severity`], [`ProgressMetric`], [`CostEstimate`]).
//! - [`events`] — payload types for standard `event.*` kinds. See `kinds-1.md` §3.
//! - [`responses`] — payload types for standard `response.*` kinds. See `kinds-1.md` §4.
//! - [`controls`] — payload types for standard `control.*` kinds. See `kinds-1.md` §6.
//! - [`capabilities`] — capability registry and dependency-closure validation.
//!   See `capabilities-1.md`.
//! - [`manifest`] — static `manifest.json` types.
//!
//! # Forward compatibility
//!
//! Every wire type in this crate is designed to round-trip unknown fields. Consumers
//! MUST NOT use `#[serde(deny_unknown_fields)]` on any wire type. Tests in
//! `tests/forward_compat.rs` enforce this property.
//!
//! # Conformance
//!
//! This crate is one implementation of `ocp-json/1`. It is wire-conformant if and
//! only if it passes the `ocp-conformance` test corpus. The crate is not the
//! definition of the protocol; the spec files in `docs/src/protocol/` are.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![warn(missing_docs)]

pub mod capabilities;
pub mod common;
pub mod controls;
pub mod envelope;
pub mod events;
pub mod kind;
pub mod manifest;
pub mod responses;
pub mod wire;

pub use capabilities::{
    validate_capability_set, Capability, CapabilityClosureError, CapabilityParseError,
};
pub use common::{ArtifactRecord, CostEstimate, OutputDescriptor, ProgressMetric, Severity, ValidationIssue};
pub use envelope::{Envelope, EnvelopeClass, Ext, RunChainEntry, RunContext};
pub use kind::{Kind, KindClass, KindParseError};
pub use manifest::{
    BundledComponent, Manifest, ManifestProtocol, ManifestRuntime, ManifestSandbox,
    ManifestSigning, RuntimeEntrypoint,
};
pub use wire::{
    ContentDigest, DigestAlgorithm, Duration, Identifier, IdentifierFormat, PathRef, Timestamp,
    ToolRef,
};
