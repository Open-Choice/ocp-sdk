use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::model::{AliasEntry, IncludeStage, OcConfig, OcError, OcFile, OcTask};

/// Parse the TOML content of an OC file.
///
/// Key shapes accepted:
///   `[["alias::endpoint"]]`
///   `[["alias::version::endpoint"]]`
///
/// An optional `[config]` table is recognised and excluded from task parsing:
///   `stop_on_error = true`
///   `include = ["./a.oce", ["./b.oce", "./c.oce"], "./d.oce"]`
///
/// Multiple `[[...]]` blocks with the same key accumulate into one TOML array,
/// each becoming its own task. When there is more than one block for a key the
/// task ids are suffixed `_1`, `_2`, …
pub fn parse_str(
    input: &str,
    aliases: &HashMap<String, AliasEntry>,
    source_path: PathBuf,
) -> Result<OcFile, OcError> {
    let raw: toml::Value = toml::from_str(input).map_err(|source| OcError::Parse {
        path: source_path.clone(),
        source,
    })?;

    let table = match raw {
        toml::Value::Table(t) => t,
        _ => {
            return Err(OcError::Validation(
                "expected a TOML table at the top level".into(),
            ))
        }
    };

    // ── [config] ─────────────────────────────────────────────────────────────
    let config = if let Some(cfg) = table.get("config") {
        parse_config(cfg, &source_path)?
    } else {
        OcConfig::default()
    };

    // ── Task blocks ───────────────────────────────────────────────────────────
    let mut tasks: Vec<OcTask> = Vec::new();

    for (key, value) in &table {
        // Reserved key handled above.
        if key == "config" {
            continue;
        }

        let parts: Vec<&str> = key.splitn(3, "::").collect();
        if parts.len() < 2 {
            return Err(OcError::Validation(format!(
                "key `{}` must be `alias::endpoint` or `alias::version::endpoint`",
                key
            )));
        }
        let alias_name = parts[0];
        let (pinned_version, endpoint_raw) = if parts.len() == 2 {
            (None::<&str>, parts[1])
        } else {
            (Some(parts[1]), parts[2])
        };

        // Extract optional `|label` suffix from the endpoint segment.
        let (endpoint, task_label) = match endpoint_raw.find('|') {
            Some(pos) => {
                let ep = &endpoint_raw[..pos];
                let lbl = endpoint_raw[pos + 1..].trim();
                (ep, if lbl.is_empty() { None } else { Some(lbl.to_string()) })
            }
            None => (endpoint_raw, None),
        };

        let entry = aliases.get(alias_name).ok_or_else(|| {
            OcError::Validation(format!("alias `{}` is not defined", alias_name))
        })?;

        let resolved_version: Option<String> = pinned_version
            .map(|v| v.to_string())
            .or_else(|| entry.version.clone());

        let items = match value {
            toml::Value::Array(arr) => arr.as_slice(),
            _ => {
                return Err(OcError::Validation(format!(
                    "expected array-of-tables for key `{}`",
                    key
                )))
            }
        };

        for (index, item) in items.iter().enumerate() {
            let params = serde_json::to_value(item).unwrap_or(Value::Null);
            let task_id = if items.len() == 1 {
                format!("{}_{}", alias_name, endpoint)
            } else {
                format!("{}_{}_{}", alias_name, endpoint, index + 1)
            };
            tasks.push(OcTask {
                id: task_id,
                plugin_id: entry.plugin_id.clone(),
                version: resolved_version.clone(),
                command: endpoint.to_string(),
                params,
                label: task_label.clone(),
            });
        }
    }

    Ok(OcFile { source_path, config, tasks })
}

/// Parse the `[config]` table into an `OcConfig`.
fn parse_config(value: &toml::Value, source_path: &Path) -> Result<OcConfig, OcError> {
    let tbl = match value {
        toml::Value::Table(t) => t,
        _ => {
            return Err(OcError::Validation(
                "`config` must be a TOML table ([config])".into(),
            ))
        }
    };

    let stop_on_error = match tbl.get("stop_on_error") {
        Some(toml::Value::Boolean(b)) => *b,
        Some(_) => {
            return Err(OcError::Validation(
                "`config.stop_on_error` must be a boolean".into(),
            ))
        }
        None => false,
    };

    let include = match tbl.get("include") {
        Some(v) => parse_include(v, source_path)?,
        None => Vec::new(),
    };

    Ok(OcConfig { stop_on_error, include })
}

/// Parse the `include` value into an ordered list of `IncludeStage`s.
///
/// Each element of the outer array is either:
/// - A string → `IncludeStage::Sequential(path)`
/// - An inner array of strings → `IncludeStage::Parallel(paths)`
fn parse_include(value: &toml::Value, source_path: &Path) -> Result<Vec<IncludeStage>, OcError> {
    let base_dir = source_path.parent().unwrap_or(Path::new("."));

    let items = match value {
        toml::Value::Array(arr) => arr,
        _ => {
            return Err(OcError::Validation(
                "`config.include` must be an array".into(),
            ))
        }
    };

    let mut stages = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match item {
            toml::Value::String(s) => {
                if s.is_empty() {
                    return Err(OcError::Validation(format!(
                        "`config.include[{}]` must not be an empty string",
                        i
                    )));
                }
                stages.push(IncludeStage::Sequential(base_dir.join(s)));
            }
            toml::Value::Array(group) => {
                if group.is_empty() {
                    return Err(OcError::Validation(format!(
                        "`config.include[{}]` is an empty parallel group",
                        i
                    )));
                }
                let mut paths = Vec::new();
                for (j, entry) in group.iter().enumerate() {
                    match entry {
                        toml::Value::String(s) => {
                            if s.is_empty() {
                                return Err(OcError::Validation(format!(
                                    "`config.include[{}][{}]` must not be an empty string",
                                    i, j
                                )));
                            }
                            paths.push(base_dir.join(s));
                        }
                        _ => {
                            return Err(OcError::Validation(format!(
                                "`config.include[{}][{}]` must be a string path",
                                i, j
                            )))
                        }
                    }
                }
                stages.push(IncludeStage::Parallel(paths));
            }
            _ => {
                return Err(OcError::Validation(format!(
                    "`config.include[{}]` must be a string or array of strings",
                    i
                )))
            }
        }
    }

    Ok(stages)
}

/// Read and parse a `.oce`, `.ocr`, `.txt`, or `.toml` file.
///
/// `.txt` and `.toml` are treated identically to `.oce` (edit-then-run, not auto-run).
pub fn load_file(
    path: impl AsRef<Path>,
    aliases: &HashMap<String, AliasEntry>,
) -> Result<OcFile, OcError> {
    let path = path.as_ref().to_path_buf();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !matches!(ext, "oce" | "ocr" | "txt" | "toml") {
        return Err(OcError::Validation(format!(
            "unsupported file extension `.{}`; expected .oce, .ocr, .txt, or .toml",
            ext
        )));
    }
    let content = std::fs::read_to_string(&path).map_err(|source| OcError::Io {
        path: path.clone(),
        source,
    })?;
    parse_str(&content, aliases, path)
}

/// Serialize an `OcFile` to the JSON task-file format that plugin executables read.
///
/// The file is written to a temp path and passed as a positional argument to the
/// plugin's `run` subcommand. Using JSON keeps the plugin free of any TOML
/// dependency and the format trivially simple.
///
/// Format:
/// ```json
/// {
///   "format": "oc-task-file/1",
///   "config": { "stop_on_error": false },
///   "tasks": [ { "id", "command", "params" } ]
/// }
/// ```
pub fn to_task_file_json(file: &OcFile) -> String {
    let tasks: Vec<serde_json::Value> = file
        .tasks
        .iter()
        .map(|t| {
            serde_json::json!({
                "id":      t.id,
                "command": t.command,
                "params":  t.params,
            })
        })
        .collect();
    serde_json::to_string(&serde_json::json!({
        "format": "oc-task-file/1",
        "config": { "stop_on_error": file.config.stop_on_error },
        "tasks": tasks,
    }))
    .expect("OcFile serialization is infallible")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(plugin_id: &str, version: Option<&str>) -> AliasEntry {
        AliasEntry {
            plugin_id: plugin_id.to_string(),
            version: version.map(|v| v.to_string()),
        }
    }

    fn calc() -> HashMap<String, AliasEntry> {
        let mut m = HashMap::new();
        m.insert("calc".to_string(), alias("com.example.calculator", None));
        m
    }

    // ── parse_str ─────────────────────────────────────────────────────────────

    #[test]
    fn parses_single_block() {
        let input = r#"[["calc::add"]]
operands = [1.0, 2.0, 3.0]
output_dir = "./out"
"#;
        let file = parse_str(input, &calc(), PathBuf::from("test.oce")).unwrap();
        assert_eq!(file.tasks.len(), 1);
        let t = &file.tasks[0];
        assert_eq!(t.id, "calc_add");
        assert_eq!(t.plugin_id, "com.example.calculator");
        assert!(t.version.is_none());
        assert_eq!(t.command, "add");
        assert_eq!(t.params["operands"][0], 1.0_f64);
    }

    #[test]
    fn two_endpoints_produce_two_tasks() {
        let input = r#"
[["calc::add"]]
operands = [1.0]

[["calc::subtract"]]
operands = [10.0]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        assert_eq!(file.tasks.len(), 2);
        let cmds: Vec<&str> = file.tasks.iter().map(|t| t.command.as_str()).collect();
        assert!(cmds.contains(&"add") && cmds.contains(&"subtract"));
    }

    #[test]
    fn pinned_version_in_key() {
        let input = r#"[["calc::1.5.0::add"]]
operands = [1.0]
"#;
        let t = &parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap().tasks[0];
        assert_eq!(t.version.as_deref(), Some("1.5.0"));
    }

    #[test]
    fn alias_default_version_used() {
        let mut aliases = HashMap::new();
        aliases.insert("calc".to_string(), alias("com.example.calculator", Some("2.0.0")));
        let input = r#"[["calc::add"]]
operands = [1.0]
"#;
        let t = &parse_str(input, &aliases, PathBuf::from("t.oce")).unwrap().tasks[0];
        assert_eq!(t.version.as_deref(), Some("2.0.0"));
    }

    #[test]
    fn key_pin_overrides_alias_default_version() {
        let mut aliases = HashMap::new();
        aliases.insert("calc".to_string(), alias("com.example.calculator", Some("2.0.0")));
        let input = r#"[["calc::3.0.0::add"]]
operands = [1.0]
"#;
        let t = &parse_str(input, &aliases, PathBuf::from("t.oce")).unwrap().tasks[0];
        assert_eq!(t.version.as_deref(), Some("3.0.0"));
    }

    #[test]
    fn repeated_key_produces_indexed_task_ids() {
        let input = r#"
[["calc::add"]]
operands = [1.0, 2.0]

[["calc::add"]]
operands = [3.0, 4.0]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        assert_eq!(file.tasks[0].id, "calc_add_1");
        assert_eq!(file.tasks[1].id, "calc_add_2");
        assert_eq!(file.tasks[0].params["operands"][0], 1.0_f64);
        assert_eq!(file.tasks[1].params["operands"][0], 3.0_f64);
    }

    #[test]
    fn unknown_alias_is_an_error() {
        let input = r#"[["missing::add"]]
operands = [1.0]
"#;
        let err = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap_err();
        assert!(matches!(err, OcError::Validation(_)));
        if let OcError::Validation(msg) = err {
            assert!(msg.contains("alias `missing` is not defined"), "{}", msg);
        }
    }

    #[test]
    fn all_tasks_enabled_by_default() {
        let input = r#"[["calc::add"]]
operands = [1.0]
"#;
        // OcTask has no `enabled` field — the simple format always runs what's in the file.
        // This test just confirms a file parses successfully (enabled is implicit).
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        assert_eq!(file.tasks.len(), 1);
    }

    // ── to_task_file_json ─────────────────────────────────────────────────────

    #[test]
    fn task_file_json_is_valid_json_with_expected_shape() {
        let input = r#"[["calc::add"]]
operands = [1.0, 2.0]
output_dir = "./out"
"#;
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        let json_str = to_task_file_json(&file);
        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .expect("to_task_file_json produced invalid JSON");
        assert_eq!(parsed["format"], "oc-task-file/1");
        let tasks = parsed["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["id"], "calc_add");
        assert_eq!(tasks[0]["command"], "add");
        assert_eq!(tasks[0]["params"]["operands"][0], 1.0_f64);
    }

    #[test]
    fn task_file_json_includes_all_tasks() {
        let input = r#"
[["calc::add"]]
operands = [1.0]

[["calc::subtract"]]
operands = [2.0]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&to_task_file_json(&file)).unwrap();
        assert_eq!(parsed["tasks"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn task_file_json_includes_config_stop_on_error() {
        let input = r#"
[config]
stop_on_error = true

[["calc::add"]]
operands = [1.0]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&to_task_file_json(&file)).unwrap();
        assert_eq!(parsed["config"]["stop_on_error"], true);
    }

    // ── [config] parsing ──────────────────────────────────────────────────────

    #[test]
    fn no_config_section_gives_defaults() {
        let input = r#"[["calc::add"]]
operands = [1.0]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        assert!(!file.config.stop_on_error);
        assert!(file.config.include.is_empty());
    }

    #[test]
    fn config_stop_on_error_parsed() {
        let input = r#"
[config]
stop_on_error = true

[["calc::add"]]
operands = [1.0]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        assert!(file.config.stop_on_error);
        assert_eq!(file.tasks.len(), 1);
    }

    #[test]
    fn config_include_sequential_only() {
        let input = r#"
[config]
include = ["./step1.oce", "./step2.oce"]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("/proj/pipeline.oce")).unwrap();
        assert_eq!(file.config.include.len(), 2);
        assert!(matches!(&file.config.include[0], IncludeStage::Sequential(p) if p.ends_with("step1.oce")));
        assert!(matches!(&file.config.include[1], IncludeStage::Sequential(p) if p.ends_with("step2.oce")));
    }

    #[test]
    fn config_include_mixed_sequential_and_parallel() {
        // step1 runs, then step2a+step2b in parallel, then step3.
        let input = r#"
[config]
include = ["./step1.oce", ["./step2a.oce", "./step2b.oce"], "./step3.oce"]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("/proj/pipeline.oce")).unwrap();
        assert_eq!(file.config.include.len(), 3);
        assert!(matches!(&file.config.include[0], IncludeStage::Sequential(p) if p.ends_with("step1.oce")));
        match &file.config.include[1] {
            IncludeStage::Parallel(paths) => {
                assert_eq!(paths.len(), 2);
                assert!(paths[0].ends_with("step2a.oce"));
                assert!(paths[1].ends_with("step2b.oce"));
            }
            other => panic!("expected Parallel, got {:?}", other),
        }
        assert!(matches!(&file.config.include[2], IncludeStage::Sequential(p) if p.ends_with("step3.oce")));
        // Tasks in the pipeline file itself may be empty.
        assert_eq!(file.tasks.len(), 0);
    }

    #[test]
    fn config_include_empty_parallel_group_is_error() {
        let input = r#"
[config]
include = [[]]
"#;
        let err = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap_err();
        assert!(matches!(err, OcError::Validation(_)));
    }

    #[test]
    fn config_section_does_not_become_a_task() {
        let input = r#"
[config]
stop_on_error = false

[["calc::add"]]
operands = [5.0]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        assert_eq!(file.tasks.len(), 1);
        assert_eq!(file.tasks[0].command, "add");
    }

    // ── |label suffix ─────────────────────────────────────────────────────────

    #[test]
    fn label_extracted_from_endpoint_key() {
        let input = r#"[["calc::add|my run"]]
operands = [1.0]
"#;
        let t = &parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap().tasks[0];
        assert_eq!(t.label.as_deref(), Some("my run"));
    }

    #[test]
    fn label_does_not_affect_task_id_or_command() {
        let input = r#"[["calc::add|my run"]]
operands = [1.0]
"#;
        let t = &parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap().tasks[0];
        assert_eq!(t.id, "calc_add");
        assert_eq!(t.command, "add");
    }

    #[test]
    fn no_label_gives_none() {
        let input = r#"[["calc::add"]]
operands = [1.0]
"#;
        let t = &parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap().tasks[0];
        assert!(t.label.is_none());
    }

    #[test]
    fn empty_label_suffix_gives_none() {
        // A trailing pipe with no text after it is treated as no label.
        let input = r#"[["calc::add|"]]
operands = [1.0]
"#;
        let t = &parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap().tasks[0];
        assert!(t.label.is_none());
        assert_eq!(t.id, "calc_add");
    }

    #[test]
    fn tasks_execute_in_source_file_order_not_alphabetically() {
        // Keys: "r-wrapper::run_script|d" sorts before "toy-calc::add|kk" alphabetically,
        // but "kk" appears first in the source file. The Vec must reflect source order.
        let mut aliases = HashMap::new();
        aliases.insert("toy-calc".to_string(), alias("com.example.calc", None));
        aliases.insert("r-wrapper".to_string(), alias("com.example.r", None));
        let input = r#"
[["toy-calc::add|kk"]]
operands = [1.0]

[["toy-calc::add|b"]]
operands = [2.0]

[["toy-calc::add|c"]]
operands = [3.0]

[["r-wrapper::run_script|d"]]
code = "print(1)"
"#;
        let file = parse_str(input, &aliases, PathBuf::from("t.oce")).unwrap();
        assert_eq!(file.tasks.len(), 4);
        let labels: Vec<Option<&str>> = file.tasks.iter().map(|t| t.label.as_deref()).collect();
        assert_eq!(labels, vec![Some("kk"), Some("b"), Some("c"), Some("d")]);
    }

    #[test]
    fn different_labels_same_base_endpoint_both_produce_same_task_id() {
        // Two blocks with different |label suffixes are different TOML keys.
        // Each key has items.len() == 1, so both get the same base id `calc_add`.
        // This is intentional — the JS CodeLens layer handles disambiguation
        // via inlineContent when multiple blocks share the same base task id.
        let input = r#"
[["calc::add|x"]]
operands = [1.0]

[["calc::add|y"]]
operands = [2.0]
"#;
        let file = parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap();
        assert_eq!(file.tasks.len(), 2);
        assert!(file.tasks.iter().all(|t| t.id == "calc_add"));
        let labels: Vec<Option<&str>> = file.tasks.iter().map(|t| t.label.as_deref()).collect();
        assert!(labels.contains(&Some("x")));
        assert!(labels.contains(&Some("y")));
    }

    #[test]
    fn version_pinning_with_label() {
        let input = r#"[["calc::1.5.0::add|pinned run"]]
operands = [1.0]
"#;
        let t = &parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap().tasks[0];
        assert_eq!(t.version.as_deref(), Some("1.5.0"));
        assert_eq!(t.label.as_deref(), Some("pinned run"));
        assert_eq!(t.id, "calc_add");
        assert_eq!(t.command, "add");
    }

    #[test]
    fn label_whitespace_trimmed() {
        // The trim() call in parse_str removes leading/trailing spaces from the label.
        let input = r#"[["calc::add|  spaced  "]]
operands = [1.0]
"#;
        let t = &parse_str(input, &calc(), PathBuf::from("t.oce")).unwrap().tasks[0];
        assert_eq!(t.label.as_deref(), Some("spaced"));
    }
}
