/// High-level query helpers used by oc-cli for discovery and help commands.
///
/// All data is read from the Open Choice SQLite database (populated at plugin
/// install time). No plugin binaries are invoked.
use serde::Deserialize;

use crate::db::Db;
use crate::errors::RunnerError;
use crate::repository::{
    PluginContentCacheRepository, PluginInstallationRepository, PluginRegistryRepository,
};

// ── Output types ──────────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct PluginInfo {
    pub plugin_id: String,
    pub display_name: String,
    pub version: String,
    pub publisher: Option<String>,
    pub trust_status: String,
    pub enabled: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct EndpointInfo {
    pub endpoint_id: String,
    pub summary: Option<String>,
    pub has_help: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct EndpointHelp {
    pub endpoint_id: String,
    pub summary: String,
    pub usage: Option<String>,
    pub parameters: Vec<HelpParameter>,
    pub output_notes: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct HelpParameter {
    pub name: String,
    pub description: String,
    pub required: bool,
    pub accepted_values: Vec<String>,
}

// ── Internal manifest shape (subset of what the Tauri app parses) ─────────────

#[derive(Debug, Deserialize)]
struct ManifestData {
    #[serde(default)]
    commands: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExplainData {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    fields: Vec<ExplainField>,
    #[serde(default)]
    usage: Option<String>,
    #[serde(default)]
    output_notes: Vec<String>,
    #[serde(default)]
    notes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExplainField {
    name: String,
    description: String,
    required: bool,
    #[serde(default)]
    accepted_values: Vec<String>,
}

// ── Public query functions ────────────────────────────────────────────────────

/// List all installed plugins with their status information.
pub fn list_plugins(db: &Db) -> Result<Vec<PluginInfo>, RunnerError> {
    let registry_repo = PluginRegistryRepository::new(db.clone());
    let install_repo = PluginInstallationRepository::new(db.clone());

    let mut plugins = Vec::new();
    for entry in registry_repo.list()? {
        if entry.trust_status == "uninstalled" {
            continue;
        }
        let version = install_repo
            .get_current(&entry.plugin_id)?
            .map(|i| {
                // Extract version from manifest JSON if possible, fall back to registry version.
                serde_json::from_str::<serde_json::Value>(&i.manifest_json)
                    .ok()
                    .and_then(|v| v["version"].as_str().map(|s| s.to_string()))
                    .unwrap_or(entry.current_version.clone())
            })
            .unwrap_or(entry.current_version.clone());

        plugins.push(PluginInfo {
            plugin_id: entry.plugin_id,
            display_name: entry.display_name,
            version,
            publisher: entry.publisher,
            trust_status: entry.trust_status,
            enabled: entry.enabled_flag,
        });
    }
    Ok(plugins)
}

/// List all endpoints for a plugin, with summaries from the help cache.
pub fn list_endpoints(db: &Db, plugin_id: &str) -> Result<Vec<EndpointInfo>, RunnerError> {
    let install_repo = PluginInstallationRepository::new(db.clone());
    let cache_repo = PluginContentCacheRepository::new(db.clone());

    let installation = install_repo
        .get_current(plugin_id)?
        .ok_or_else(|| RunnerError::plugin_not_found(format!("Plugin '{}' not found or not installed.", plugin_id)))?;

    let manifest: ManifestData = serde_json::from_str(&installation.manifest_json)
        .map_err(|e| RunnerError::internal(format!("Failed to parse plugin manifest: {}", e)))?;

    let mut endpoints = Vec::new();
    for cmd in &manifest.commands {
        let cached = cache_repo.get(&installation.installation_id, "help", Some(cmd))?;
        let summary = cached.as_ref().and_then(|json| {
            serde_json::from_str::<ExplainData>(json).ok().map(|d| d.summary)
        });
        endpoints.push(EndpointInfo {
            endpoint_id: cmd.clone(),
            summary,
            has_help: cached.is_some(),
        });
    }
    Ok(endpoints)
}

/// Get detailed help for a specific endpoint.
pub fn get_endpoint_help(db: &Db, plugin_id: &str, endpoint_id: &str) -> Result<EndpointHelp, RunnerError> {
    let install_repo = PluginInstallationRepository::new(db.clone());
    let cache_repo = PluginContentCacheRepository::new(db.clone());

    let installation = install_repo
        .get_current(plugin_id)?
        .ok_or_else(|| RunnerError::plugin_not_found(format!("Plugin '{}' not found or not installed.", plugin_id)))?;

    let payload = cache_repo
        .get(&installation.installation_id, "help", Some(endpoint_id))?
        .ok_or_else(|| RunnerError::invalid_argument(format!(
            "No help found for endpoint '{}' on plugin '{}'. Try running Open Choice to populate help.",
            endpoint_id, plugin_id
        )))?;

    let d: ExplainData = serde_json::from_str(&payload)
        .map_err(|e| RunnerError::internal(format!("Failed to parse help data: {}", e)))?;

    Ok(EndpointHelp {
        endpoint_id: endpoint_id.to_string(),
        summary: d.summary,
        usage: d.usage,
        parameters: d.fields.into_iter().map(|f| HelpParameter {
            name: f.name,
            description: f.description,
            required: f.required,
            accepted_values: f.accepted_values,
        }).collect(),
        output_notes: d.output_notes,
        notes: d.notes,
    })
}
