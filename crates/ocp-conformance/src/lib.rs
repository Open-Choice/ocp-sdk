//! `ocp-conformance` — operative test corpus for `ocp-json/1`.
//!
//! Per `CONTRIBUTING.md`, the conformance corpus is the **operative** definition
//! of `ocp-json/1`. The spec documents are explanatory; if they ever disagree
//! with the corpus, the corpus wins and a fix is filed against the spec.
//!
//! The crate provides:
//!
//! - [`Fixture`] — a single conformance test case loaded from JSON.
//! - [`FixtureCorpus`] — a collection of fixtures grouped by category.
//! - [`load_corpus`] — load the bundled fixture corpus from the `fixtures/`
//!   directory at the crate root.
//!
//! The bundled corpus is small but covers the load-bearing forward-compat
//! properties (round-trip, unknown-field preservation, grammar rejection,
//! capability dependency closure). It will grow as new spec sections gain
//! coverage.
//!
//! # Running the corpus
//!
//! The crate's own test suite (in `tests/run_corpus.rs`) runs every fixture
//! against `ocp-types-v1`. External implementations (other languages, host
//! re-implementations) consume the JSON fixture files directly.

#![forbid(unsafe_code)]
#![deny(missing_debug_implementations)]
#![warn(missing_docs)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// What a fixture asserts about its `input` JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureAssertion {
    /// The input MUST parse and round-trip losslessly.
    Roundtrip,
    /// The input MUST be rejected by a conformant parser.
    Reject,
    /// The input MUST parse, and the parsed form MUST preserve at least the
    /// fields named in [`Fixture::expect_preserved`] when re-serialized.
    PreserveUnknown,
    /// The input MUST parse and the resulting capability set MUST satisfy the
    /// dependency closure rules in `capabilities-1.md` §6.
    CapabilityClosureValid,
    /// The input MUST parse but the resulting capability set MUST FAIL the
    /// dependency closure rules.
    CapabilityClosureInvalid,
}

/// Which type the input should be parsed as. The runner uses this to dispatch
/// to the correct implementation in `ocp-types-v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureTarget {
    /// Parse as a top-level `Envelope`.
    Envelope,
    /// Parse as a `Kind` string.
    Kind,
    /// Parse as a `Capability` string.
    Capability,
    /// Parse as a list of `Capability` strings (for closure validation).
    CapabilityList,
    /// Parse as a `Manifest` object.
    Manifest,
    /// Parse as an `Identifier` object.
    Identifier,
    /// Parse as a `Duration` object.
    Duration,
    /// Parse as a `ContentDigest` object.
    ContentDigest,
    /// Parse as a `PathRef` object.
    PathRef,
}

/// A single conformance fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    /// Stable identifier for this fixture (typically the file stem).
    pub id: String,

    /// One-line human description of what the fixture exercises.
    pub description: String,

    /// Which type to parse the `input` as.
    pub target: FixtureTarget,

    /// What the fixture asserts about the `input`.
    pub assertion: FixtureAssertion,

    /// The input value. For string-targeted fixtures (Kind, Capability),
    /// this is a JSON string. For object-targeted fixtures, a JSON object.
    pub input: Value,

    /// For [`FixtureAssertion::PreserveUnknown`]: the field paths that MUST
    /// survive a round-trip. Each entry is a top-level key in the input.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expect_preserved: Vec<String>,
}

impl Fixture {
    /// Load a fixture from a JSON file at `path`.
    pub fn from_file(path: &Path) -> Result<Self, FixtureLoadError> {
        let bytes = fs::read(path).map_err(|e| FixtureLoadError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        serde_json::from_slice(&bytes).map_err(|e| FixtureLoadError::Parse {
            path: path.to_path_buf(),
            source: e,
        })
    }
}

// ---------------------------------------------------------------------------
// FixtureCorpus
// ---------------------------------------------------------------------------

/// A collection of fixtures grouped by category (subdirectory under `fixtures/`).
#[derive(Debug, Clone, Default)]
pub struct FixtureCorpus {
    /// Fixtures grouped by category. The category is the immediate parent
    /// directory name (e.g., `envelope`, `kind`, `capability`).
    pub by_category: BTreeMap<String, Vec<Fixture>>,
}

impl FixtureCorpus {
    /// All fixtures across all categories.
    pub fn all(&self) -> impl Iterator<Item = (&str, &Fixture)> {
        self.by_category
            .iter()
            .flat_map(|(cat, fixtures)| fixtures.iter().map(move |f| (cat.as_str(), f)))
    }

    /// Total fixture count.
    pub fn len(&self) -> usize {
        self.by_category.values().map(|v| v.len()).sum()
    }

    /// Returns true if the corpus is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Load the bundled fixture corpus from the given directory. The directory
/// MUST contain one subdirectory per category, each containing one or more
/// `.json` files.
pub fn load_corpus(root: &Path) -> Result<FixtureCorpus, FixtureLoadError> {
    let mut corpus = FixtureCorpus::default();
    let entries = fs::read_dir(root).map_err(|e| FixtureLoadError::Io {
        path: root.to_path_buf(),
        source: e,
    })?;
    for entry in entries {
        let entry = entry.map_err(|e| FixtureLoadError::Io {
            path: root.to_path_buf(),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let category = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| FixtureLoadError::BadCategoryName(path.clone()))?
            .to_string();
        let mut fixtures = Vec::new();
        let cat_entries = fs::read_dir(&path).map_err(|e| FixtureLoadError::Io {
            path: path.clone(),
            source: e,
        })?;
        for cat_entry in cat_entries {
            let cat_entry = cat_entry.map_err(|e| FixtureLoadError::Io {
                path: path.clone(),
                source: e,
            })?;
            let fpath = cat_entry.path();
            if fpath.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            fixtures.push(Fixture::from_file(&fpath)?);
        }
        fixtures.sort_by(|a, b| a.id.cmp(&b.id));
        corpus.by_category.insert(category, fixtures);
    }
    Ok(corpus)
}

/// Path to the bundled fixture root, computed at compile time.
pub fn bundled_fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors produced when loading fixtures from disk.
#[derive(Debug, thiserror::Error)]
pub enum FixtureLoadError {
    /// I/O error reading a fixture file or directory.
    #[error("I/O error reading {path}: {source}")]
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// JSON parse error in a fixture file.
    #[error("failed to parse fixture {path}: {source}")]
    Parse {
        /// The path that failed.
        path: PathBuf,
        /// The underlying parse error.
        #[source]
        source: serde_json::Error,
    },
    /// A category subdirectory had a non-UTF-8 name.
    #[error("category directory has invalid name: {0}")]
    BadCategoryName(PathBuf),
}
