use serde_json::Value;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OcError {
    #[error("failed to read file at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("{0}")]
    Validation(String),
}

/// A single runnable task resolved from an OC file.
#[derive(Debug, Clone)]
pub struct OcTask {
    /// Auto-generated id: `<alias>_<endpoint>` or `<alias>_<endpoint>_N`.
    pub id: String,
    /// Resolved plugin id from the alias registry.
    pub plugin_id: String,
    /// Version pinned in the key or alias default; `None` = latest installed.
    pub version: Option<String>,
    /// The plugin endpoint to invoke.
    pub command: String,
    /// Task parameters as a JSON value (object).
    pub params: Value,
    /// Optional display name from the `|label` suffix in the task key.
    /// When set, console output lines are prefixed with `label > `.
    pub label: Option<String>,
}

/// One entry in an `include` list.
///
/// A flat string becomes `Sequential`; an inner array becomes `Parallel`.
///
/// Example .oce:
/// ```toml
/// [config]
/// include = ["./step1.oce", ["./step2a.oce", "./step2b.oce"], "./step3.oce"]
/// ```
/// Produces: Sequential(step1) → Parallel(step2a, step2b) → Sequential(step3)
///
/// Semantics: stages execute in order; all files within a `Parallel` stage
/// run concurrently and the next stage waits for all of them to finish.
/// If any file in a stage fails the whole stage fails (all outputs are
/// collected before the failure is reported).
#[derive(Debug, Clone)]
pub enum IncludeStage {
    /// A single .oce file that runs to completion before the next stage.
    Sequential(PathBuf),
    /// A set of .oce files that all run at the same time.
    /// The next stage starts only after every file in this group finishes.
    Parallel(Vec<PathBuf>),
}

/// Document-level configuration parsed from `[config]` in the .oce file.
#[derive(Debug, Clone, Default)]
pub struct OcConfig {
    /// When `true`, a task failure stops all remaining tasks in this file
    /// from running. Defaults to `false`.
    pub stop_on_error: bool,
    /// Ordered list of include stages. Empty when no `include` key is present.
    pub include: Vec<IncludeStage>,
}

/// A loaded OC file with all aliases resolved.
#[derive(Debug, Clone)]
pub struct OcFile {
    pub source_path: PathBuf,
    /// Document-level configuration from `[config]`.
    pub config: OcConfig,
    pub tasks: Vec<OcTask>,
}

/// Alias definition: a short name maps to an installed plugin.
pub struct AliasEntry {
    pub plugin_id: String,
    /// Default version; overridden by a version segment in the .oce key.
    pub version: Option<String>,
}
