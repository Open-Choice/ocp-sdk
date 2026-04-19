use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oc_core::{load_file, to_task_file_json, AliasEntry, IncludeStage};
use ocp_host::run_tool_task;

use crate::db::Db;
use crate::errors::RunnerError;
use crate::repository::{PluginAliasRepository, PluginInstallationRepository, PluginRegistryRepository};

// ── Plugin map ────────────────────────────────────────────────────────────────

pub struct PluginRunInfo {
    pub entrypoint_path: PathBuf,
}

/// Loads every plugin that is currently enabled, trusted, and has a valid
/// installation. Shared between pipeline and single-task execution paths.
pub fn load_plugin_map(db: &Db) -> Result<HashMap<String, PluginRunInfo>, RunnerError> {
    let registry_repo = PluginRegistryRepository::new(db.clone());
    let install_repo = PluginInstallationRepository::new(db.clone());

    let mut map = HashMap::new();
    for entry in registry_repo.list()? {
        if !entry.enabled_flag {
            continue;
        }
        match entry.trust_status.as_str() {
            "verified" | "warning" => {}
            _ => continue,
        }
        if let Some(install) = install_repo.get_current(&entry.plugin_id)? {
            let ep = PathBuf::from(&install.entrypoint_path);
            if ep.exists() {
                map.insert(entry.plugin_id, PluginRunInfo { entrypoint_path: ep });
            } else {
                eprintln!(
                    "[oc-runner] plugin '{}' skipped: entrypoint not found at '{}'",
                    entry.plugin_id, install.entrypoint_path
                );
            }
        }
    }
    Ok(map)
}

// ── Alias map ─────────────────────────────────────────────────────────────────

/// Builds the alias → plugin_id map from the Open Choice database.
///
/// Implied aliases (last segment of plugin_id) are seeded first so explicit
/// user-defined aliases take precedence.
pub fn load_alias_map(db: &Db) -> Result<HashMap<String, AliasEntry>, RunnerError> {
    let mut map: HashMap<String, AliasEntry> = HashMap::new();

    for p in PluginRegistryRepository::new(db.clone()).list()? {
        if let Some(short) = p.plugin_id.split('.').next_back() {
            if !map.contains_key(short) {
                map.insert(
                    short.to_string(),
                    AliasEntry { plugin_id: p.plugin_id, version: None },
                );
            }
        }
    }

    for e in PluginAliasRepository::new(db.clone()).list()? {
        map.insert(e.alias, AliasEntry { plugin_id: e.plugin_id, version: e.version });
    }

    Ok(map)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_run_tmp_path(source_path: &Path) -> PathBuf {
    let stem = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("oc-run")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>();
    std::env::temp_dir().join(format!(
        "oc-run-{}-{}-{}.tmp",
        stem,
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ))
}

fn resolve_include(base: &Path, rel: &Path) -> PathBuf {
    if rel.is_absolute() {
        rel.to_path_buf()
    } else {
        base.parent().unwrap_or(Path::new(".")).join(rel)
    }
}

// ── Pipeline executor ─────────────────────────────────────────────────────────

/// Runs the full pipeline for a single .oce file, recursively expanding includes.
///
/// `on_event` receives each raw NDJSON event string as it is emitted by the
/// plugin. Callers decide what to do with it: stream to a UI channel, print to
/// stdout, collect into a buffer, or nothing (pass `None`).
pub fn run_oc_file_impl(
    path: &Path,
    aliases: Arc<HashMap<String, AliasEntry>>,
    plugins: Arc<HashMap<String, PluginRunInfo>>,
    on_event: Option<Arc<dyn Fn(String) + Send + Sync>>,
    inherited_stop: bool,
) -> Result<bool, RunnerError> {
    let oc_file = load_file(path, &aliases)
        .map_err(|e| RunnerError::parse(e.to_string()))?;

    let stop_on_error = inherited_stop || oc_file.config.stop_on_error;
    let mut all_ok = true;

    // ── 1. Include stages ─────────────────────────────────────────────────────
    for stage in &oc_file.config.include {
        let stage_ok = match stage {
            IncludeStage::Sequential(rel) => {
                let abs = resolve_include(path, rel);
                run_oc_file_impl(&abs, aliases.clone(), plugins.clone(), on_event.clone(), stop_on_error)?
            }
            IncludeStage::Parallel(rels) => {
                if stop_on_error {
                    // Run sequentially so the first failure halts immediately.
                    let mut ok = true;
                    for rel in rels.iter() {
                        let abs = resolve_include(path, rel);
                        if !run_oc_file_impl(&abs, aliases.clone(), plugins.clone(), on_event.clone(), stop_on_error)? {
                            ok = false;
                            break;
                        }
                    }
                    ok
                } else {
                    let handles: Vec<_> = rels
                        .iter()
                        .map(|rel| {
                            let abs = resolve_include(path, rel);
                            let a = aliases.clone();
                            let p = plugins.clone();
                            let ev = on_event.clone();
                            std::thread::spawn(move || run_oc_file_impl(&abs, a, p, ev, stop_on_error))
                        })
                        .collect();

                    let mut ok = true;
                    for handle in handles {
                        match handle.join() {
                            Ok(Ok(r)) => { if !r { ok = false; } }
                            Ok(Err(e)) => return Err(e),
                            Err(_) => return Err(RunnerError::internal("A parallel include thread panicked.")),
                        }
                    }
                    ok
                }
            }
        };

        if !stage_ok {
            all_ok = false;
            if stop_on_error {
                return Ok(false);
            }
        }
    }

    // ── 2. Tasks defined in this file ─────────────────────────────────────────
    let temp_path = make_run_tmp_path(path);
    let task_file_json = to_task_file_json(&oc_file);
    let working_dir = path.parent().map(Path::to_path_buf);

    for task in &oc_file.tasks {
        let plugin = plugins.get(&task.plugin_id).ok_or_else(|| {
            RunnerError::plugin_not_found(format!(
                "Plugin '{}' is not available (not installed, not trusted, or disabled).",
                task.plugin_id
            ))
        })?;

        fs::write(&temp_path, &task_file_json)
            .map_err(|e| RunnerError::internal(format!("Failed to write run file: {}", e)))?;

        let task_id_str = task.id.clone();
        let task_label = task.label.clone();
        let ev = on_event.clone();

        let result = run_tool_task(
            &plugin.entrypoint_path,
            &temp_path,
            &task_id_str,
            move |event| {
                if let Some(ref cb) = ev {
                    if let Ok(mut val) = serde_json::to_value(&event) {
                        if let (Some(ref lbl), serde_json::Value::Object(ref mut map)) =
                            (&task_label, &mut val)
                        {
                            map.insert("label".to_string(), serde_json::json!(lbl));
                        }
                        if let Ok(json) = serde_json::to_string(&val) {
                            cb(json);
                        }
                    }
                }
            },
            |_| {},
            Some(std::time::Duration::from_secs(3600)),
            working_dir.as_deref(),
        );

        let task_ok = match result {
            Ok(summary) => summary.exit_code == 0,
            Err(ocp_host::runner::RunError::Timeout(secs)) => {
                let _ = fs::remove_file(&temp_path);
                return Err(RunnerError::timeout(secs));
            }
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                return Err(RunnerError::process_spawn_failed(e.to_string()));
            }
        };

        if !task_ok {
            all_ok = false;
            if stop_on_error {
                let _ = fs::remove_file(&temp_path);
                return Ok(false);
            }
        }
    }

    let _ = fs::remove_file(&temp_path);
    Ok(all_ok)
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Run a .oce file, optionally scoped to a single task.
///
/// This is the main entry point for both `oc-cli` and the Tauri app's
/// scheduled-job runner. Loads plugin and alias maps from `db`, then
/// dispatches to single-task or full-pipeline mode.
///
/// Each NDJSON event emitted by the plugin is forwarded to `on_event` as it
/// arrives. Pass `None` for fire-and-forget execution with no live output.
pub fn run_oce_file(
    path: &Path,
    task_id: Option<&str>,
    db: &Db,
    on_event: Option<Arc<dyn Fn(String) + Send + Sync>>,
) -> Result<(), RunnerError> {
    let aliases = Arc::new(load_alias_map(db)?);
    let plugins = Arc::new(load_plugin_map(db)?);

    if let Some(id) = task_id.filter(|s| !s.is_empty()) {
        // Single-task mode: parse the file, find the named task, run only it.
        let oc_file = load_file(path, &aliases)
            .map_err(|e| RunnerError::parse(e.to_string()))?;

        let task = oc_file
            .tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| RunnerError::invalid_argument(format!("Task '{}' not found.", id)))?;

        let plugin = plugins.get(&task.plugin_id).ok_or_else(|| {
            RunnerError::plugin_not_found(format!("Plugin '{}' is not available.", task.plugin_id))
        })?;

        let temp_path = make_run_tmp_path(path);
        fs::write(&temp_path, to_task_file_json(&oc_file))
            .map_err(|e| RunnerError::internal(format!("Failed to write run file: {}", e)))?;

        let task_id_str = task.id.clone();
        let task_label = task.label.clone();
        let ev = on_event.clone();
        let working_dir = path.parent().map(Path::to_path_buf);

        let result = run_tool_task(
            &plugin.entrypoint_path,
            &temp_path,
            &task_id_str,
            move |event| {
                if let Some(ref cb) = ev {
                    if let Ok(mut val) = serde_json::to_value(&event) {
                        if let (Some(ref lbl), serde_json::Value::Object(ref mut map)) =
                            (&task_label, &mut val)
                        {
                            map.insert("label".to_string(), serde_json::json!(lbl));
                        }
                        if let Ok(json) = serde_json::to_string(&val) {
                            cb(json);
                        }
                    }
                }
            },
            |_| {},
            Some(std::time::Duration::from_secs(3600)),
            working_dir.as_deref(),
        );

        let _ = fs::remove_file(&temp_path);

        match result {
            Ok(_) => {}
            Err(ocp_host::runner::RunError::Timeout(secs)) => return Err(RunnerError::timeout(secs)),
            Err(e) => return Err(RunnerError::process_spawn_failed(e.to_string())),
        }
    } else {
        // Pipeline mode: expand includes and run all tasks.
        run_oc_file_impl(path, aliases, plugins, on_event, false).map(|_| ())?;
    }

    Ok(())
}
