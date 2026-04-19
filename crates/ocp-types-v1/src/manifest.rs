//! Static `manifest.json` types.
//!
//! `manifest.json` is the static discovery surface for `ocp-json/1` plugins.
//! It carries plugin identity, runtime entrypoints, declared capabilities, and
//! signing metadata. The host reads it at install time and at every load
//! before invoking any binary endpoint.
//!
//! See `ocp-json-1.md` for the high-level package layout. The types in this
//! module are part of the frozen surface.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::capabilities::Capability;
use crate::wire::ContentDigest;

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// The top-level `manifest.json` document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// Manifest schema version. MUST be `"1"` for `ocp-json/1`.
    pub schema_version: String,

    /// Plugin identifier (typically reverse-DNS).
    pub plugin_id: String,

    /// Display name shown in the host UI.
    pub display_name: String,

    /// Plugin version (semver string).
    pub version: String,

    /// Publisher / author name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,

    /// Free-form description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Runtime entrypoint information.
    pub runtime: ManifestRuntime,

    /// Protocol family and version. For `ocp-json/1` plugins, this MUST be
    /// `{ family: "ocp-json", version: "1" }`.
    pub protocol: ManifestProtocol,

    /// The list of commands this plugin exposes. Each command corresponds to
    /// a `schemas/<command>.schema.json`, `examples/<command>.json`, etc.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<String>,

    /// The capabilities this plugin advertises. See `capabilities-1.md`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<Capability>,

    /// Filesystem and network sandbox declarations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<ManifestSandbox>,

    /// Signing metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing: Option<ManifestSigning>,

    /// Bundled component declarations, when the plugin advertises
    /// `bundled.components.attested`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bundled_components: Vec<BundledComponent>,

    /// Vendor extension bag for manifest-level extensions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub ext: BTreeMap<String, Value>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ManifestRuntime
// ---------------------------------------------------------------------------

/// Runtime entrypoint information from the manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestRuntime {
    /// Runtime type tag (e.g., `"native-sidecar"`).
    #[serde(rename = "type")]
    pub runtime_type: String,

    /// Per-platform binary entrypoints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entrypoints: Vec<RuntimeEntrypoint>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

/// A single platform entrypoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEntrypoint {
    /// Operating system identifier (`"windows"`, `"macos"`, `"linux"`).
    pub os: String,

    /// CPU architecture identifier (`"x86_64"`, `"aarch64"`).
    pub arch: String,

    /// Path inside the `.ocplugin` package, relative to the package root.
    /// MUST use forward slashes per `wire-format-1.md` §11.
    pub path: String,

    /// SHA-256 of the binary, as a 64-character lowercase hex string.
    /// (Carried as a tagged [`ContentDigest`] for forward-compat with future
    /// hash algorithms.)
    pub digest: ContentDigest,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ManifestProtocol
// ---------------------------------------------------------------------------

/// Protocol family and version declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestProtocol {
    /// Protocol family. For `ocp-json/1`, this MUST be `"ocp-json"`.
    pub family: String,

    /// Protocol version. For `ocp-json/1`, this MUST be `"1"`.
    pub version: String,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ManifestSandbox
// ---------------------------------------------------------------------------

/// Sandbox declarations: what the plugin needs to read, write, and access on
/// the network.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ManifestSandbox {
    /// Filesystem read paths the plugin requires.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fs_read: Vec<String>,

    /// Filesystem write paths the plugin requires.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fs_write: Vec<String>,

    /// Whether the plugin requires network access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<bool>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// ManifestSigning
// ---------------------------------------------------------------------------

/// Signing metadata for the manifest. The actual signature lives in
/// `signatures/manifest.sig` inside the `.ocplugin` package.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestSigning {
    /// Identifier of the signing key.
    pub key_id: String,

    /// Path to the signature file inside the package.
    pub signature_path: String,

    /// Signing algorithm tag (e.g., `"ed25519"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<String>,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

// ---------------------------------------------------------------------------
// BundledComponent
// ---------------------------------------------------------------------------

/// A bundled component declaration. Present when the plugin advertises
/// `bundled.components.attested`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BundledComponent {
    /// Stable identifier for the bundled component.
    pub component_id: String,

    /// Display name of the bundled component.
    pub display_name: String,

    /// Version of the bundled component.
    pub version: String,

    /// Path to the component's binary inside the wrapping `.ocplugin` package,
    /// relative to the package root. Forward slashes only.
    pub path: String,

    /// Content digest of the bundled binary.
    pub digest: ContentDigest,

    /// Forward-compat slot.
    #[serde(flatten)]
    pub other: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_manifest() -> Manifest {
        Manifest {
            schema_version: "1".into(),
            plugin_id: "com.example.toy-calculator".into(),
            display_name: "Toy Calculator".into(),
            version: "1.0.0".into(),
            publisher: Some("Example Org".into()),
            description: Some("Toy calculator for testing".into()),
            runtime: ManifestRuntime {
                runtime_type: "native-sidecar".into(),
                entrypoints: vec![RuntimeEntrypoint {
                    os: "windows".into(),
                    arch: "x86_64".into(),
                    path: "bin/windows-x86_64/toy-calculator.exe".into(),
                    digest: ContentDigest::sha256("a".repeat(64)),
                    other: BTreeMap::new(),
                }],
                other: BTreeMap::new(),
            },
            protocol: ManifestProtocol {
                family: "ocp-json".into(),
                version: "1".into(),
                other: BTreeMap::new(),
            },
            commands: vec!["calculate".into()],
            capabilities: vec![
                Capability::parse("stdin.control_channel").unwrap(),
                Capability::parse("control.cancel").unwrap(),
                Capability::parse("events.progress").unwrap(),
            ],
            sandbox: Some(ManifestSandbox {
                fs_read: vec![],
                fs_write: vec!["plugin-workdir".into()],
                network: Some(false),
                other: BTreeMap::new(),
            }),
            signing: Some(ManifestSigning {
                key_id: "open-choice-2026".into(),
                signature_path: "signatures/manifest.sig".into(),
                algorithm: Some("ed25519".into()),
                other: BTreeMap::new(),
            }),
            bundled_components: vec![],
            ext: BTreeMap::new(),
            other: BTreeMap::new(),
        }
    }

    #[test]
    fn manifest_round_trips() {
        let m = sample_manifest();
        let json = serde_json::to_value(&m).unwrap();
        let back: Manifest = serde_json::from_value(json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn manifest_preserves_unknown_top_level_fields() {
        let json = json!({
            "schema_version": "1",
            "plugin_id": "x",
            "display_name": "x",
            "version": "1.0.0",
            "runtime": {
                "type": "native-sidecar",
                "entrypoints": []
            },
            "protocol": { "family": "ocp-json", "version": "1" },
            "future_field": { "experimental": true }
        });
        let m: Manifest = serde_json::from_value(json.clone()).unwrap();
        assert!(m.other.contains_key("future_field"));
        let back = serde_json::to_value(&m).unwrap();
        assert_eq!(back["future_field"], json["future_field"]);
    }

    #[test]
    fn manifest_with_bundled_components() {
        let mut m = sample_manifest();
        m.capabilities.push(Capability::parse("bundled.components").unwrap());
        m.capabilities
            .push(Capability::parse("bundled.components.attested").unwrap());
        m.bundled_components.push(BundledComponent {
            component_id: "design-maxdiff".into(),
            display_name: "Design MaxDiff".into(),
            version: "0.5.2".into(),
            path: "components/design-maxdiff/design-maxdiff.exe".into(),
            digest: ContentDigest::sha256("c".repeat(64)),
            other: BTreeMap::new(),
        });
        let json = serde_json::to_value(&m).unwrap();
        let back: Manifest = serde_json::from_value(json).unwrap();
        assert_eq!(back, m);
    }
}
