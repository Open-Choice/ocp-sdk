use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::errors::RunnerError;
use crate::repository::{
    CachedContentEntry, PluginCapabilityEntry, PluginContentCacheRepository, PluginEventsRepository,
    PluginInstallationEntry, PluginInstallationRepository, PluginRegistryEntry,
    PluginRegistryRepository, PluginRuntimeEventEntry, SnippetsRepository,
};
use crate::trust::{RevocationList, TrustedKeyStore};

// ── Manifest types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, Clone)]
pub struct ManifestSnippet {
    pub id: String,
    pub title: String,
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    pub schema_version: String,
    pub plugin_id: String,
    pub display_name: String,
    pub version: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub runtime: RuntimeSection,
    pub protocol: ProtocolSection,
    #[serde(default)]
    pub capabilities: serde_json::Value,
    #[serde(default)]
    pub sandbox: Option<serde_json::Value>,
    pub signing: Option<SigningSection>,
    pub risk_profile: Option<String>,
    pub snippets: Option<Vec<ManifestSnippet>>,
    #[serde(default)]
    pub commands: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeSection {
    pub r#type: String,
    pub entrypoints: Vec<EntrypointEntry>,
}

#[derive(Debug, Deserialize)]
pub struct EntrypointEntry {
    pub os: String,
    pub arch: String,
    pub path: String,
    pub digest: EntrypointDigest,
}

#[derive(Debug, Deserialize)]
pub struct EntrypointDigest {
    pub algorithm: String,
    pub value: String,
}

#[derive(Debug, Deserialize)]
pub struct ProtocolSection {
    pub family: String,
    pub version: String,
    #[serde(default)]
    pub supported_versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct SigningSection {
    pub signature_path: String,
    pub key_id: String,
}

// ── Install result ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct InstalledPluginResult {
    pub plugin_id: String,
    pub installation_id: String,
    pub trust_status: String,
    pub trust_tier: String,
    pub signature_status: String,
    pub warnings: Vec<String>,
}

// ── Trust decision (internal) ─────────────────────────────────────────────────

struct TrustDecision {
    trust_status: String,
    trust_tier: String,
    signature_status: String,
    resolved_key_id: Option<String>,
    warnings: Vec<String>,
}

// ── Service ───────────────────────────────────────────────────────────────────

pub struct PluginInstallService {
    registry_repo: PluginRegistryRepository,
    installation_repo: PluginInstallationRepository,
    events_repo: PluginEventsRepository,
    snippets_repo: SnippetsRepository,
    content_cache_repo: PluginContentCacheRepository,
    db: crate::db::Db,
    plugins_dir: PathBuf,
    pub developer_mode: bool,
    pub risk_acknowledged: bool,
    pub capabilities_change_acknowledged: bool,
}

impl PluginInstallService {
    pub fn new(
        db: crate::db::Db,
        plugins_dir: PathBuf,
        developer_mode: bool,
        risk_acknowledged: bool,
        capabilities_change_acknowledged: bool,
    ) -> Self {
        Self {
            registry_repo: PluginRegistryRepository::new(db.clone()),
            installation_repo: PluginInstallationRepository::new(db.clone()),
            events_repo: PluginEventsRepository::new(db.clone()),
            snippets_repo: SnippetsRepository::new(db.clone()),
            content_cache_repo: PluginContentCacheRepository::new(db.clone()),
            db,
            plugins_dir,
            developer_mode,
            risk_acknowledged,
            capabilities_change_acknowledged,
        }
    }

    pub fn install_package(&self, package_path: &Path) -> Result<InstalledPluginResult, RunnerError> {
        let key_store = TrustedKeyStore::load()?;
        let revocation = RevocationList::load()?;

        // 1. Open archive.
        let file = std::fs::File::open(package_path).map_err(|e| {
            RunnerError::invalid_argument(format!("Cannot open plugin package: {}", e))
        })?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| {
            RunnerError::invalid_argument(format!("Not a valid .ocplugin archive: {}", e))
        })?;

        // 2. Read and parse manifest.json.
        let manifest_json = read_archive_file(&mut archive, "manifest.json")?;
        let manifest: PluginManifest = serde_json::from_str(&manifest_json).map_err(|e| {
            RunnerError::invalid_argument(format!("manifest.json is invalid: {}", e))
        })?;

        // 3. Validate required manifest fields.
        validate_manifest(&manifest)?;

        // 4. Revocation check — hard block.
        if let Some(reason) = revocation.check(&manifest.plugin_id, &manifest.version) {
            return Err(RunnerError::plugin_revoked(format!(
                "Plugin '{}' v{} is on the revocation blocklist: {}",
                manifest.plugin_id, manifest.version, reason
            )));
        }

        // 4b. Risk-profile consent gate.
        let risk_profile_str = manifest.risk_profile.as_deref().unwrap_or("safe");
        if risk_profile_str == "arbitrary-code-execution" && !self.risk_acknowledged {
            return Err(RunnerError::invalid_argument(
                "This plugin declares risk_profile 'arbitrary-code-execution'. \
                 Pass --risk-acknowledged after reviewing the warning to proceed.",
            ));
        }

        // 4c. Capability-change consent gate.
        let caps_hash = compute_capabilities_hash(
            &manifest.capabilities,
            manifest.sandbox.as_ref(),
            risk_profile_str,
        );
        let capabilities_changed = self.installation_repo
            .get_current(&manifest.plugin_id)?
            .and_then(|inst| inst.capabilities_hash)
            .map(|stored| stored != caps_hash)
            .unwrap_or(false);
        if capabilities_changed && !self.capabilities_change_acknowledged {
            return Err(RunnerError::invalid_argument(
                "This plugin update changes the declared capabilities compared to the installed version. \
                 Pass --capabilities-change-acknowledged after reviewing the diff to proceed.",
            ));
        }

        // 5. Select entrypoint for current OS/arch.
        let current_os = current_os();
        let current_arch = current_arch();
        let entrypoint = manifest.runtime.entrypoints.iter()
            .find(|ep| ep.os == current_os && ep.arch == current_arch)
            .ok_or_else(|| {
                RunnerError::invalid_argument(format!(
                    "Plugin has no entrypoint for {}-{}. Available: {}",
                    current_os, current_arch,
                    manifest.runtime.entrypoints.iter()
                        .map(|ep| format!("{}-{}", ep.os, ep.arch))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;

        // 6. Extract entrypoint binary and verify SHA-256.
        let binary_bytes = read_archive_bytes(&mut archive, &entrypoint.path)?;
        let computed_sha256 = compute_sha256_bytes(&binary_bytes);
        if entrypoint.digest.algorithm != "sha256" {
            return Err(RunnerError::invalid_argument(format!(
                "Plugin entrypoint '{}' declares unsupported digest algorithm '{}' (only 'sha256' is supported).",
                entrypoint.path, entrypoint.digest.algorithm
            )));
        }
        if computed_sha256 != entrypoint.digest.value.to_lowercase() {
            return Err(RunnerError::invalid_argument(format!(
                "Plugin artifact integrity check FAILED for '{}'. \
                 Expected: {}, computed: {}.",
                entrypoint.path, entrypoint.digest.value, computed_sha256
            )));
        }

        // 7. Signature verification.
        let trust = self.evaluate_trust(&manifest, &manifest_json, &mut archive, &key_store)?;

        // 8. Set up install directory.
        let install_dir = self.plugins_dir
            .join("installs")
            .join(&manifest.plugin_id)
            .join(&manifest.version);
        let installs_root = self.plugins_dir.join("installs");
        if !install_dir.starts_with(&installs_root) {
            return Err(RunnerError::invalid_argument(
                "Plugin ID or version resolves to a path outside the plugin install directory.",
            ));
        }
        std::fs::create_dir_all(&install_dir).map_err(|e| {
            RunnerError::internal(format!("Failed to create install directory: {}", e))
        })?;

        // 9. Write entrypoint binary.
        let binary_filename = Path::new(&entrypoint.path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| RunnerError::invalid_argument(format!(
                "Entrypoint path '{}' has no valid filename component.", entrypoint.path
            )))?;
        let entrypoint_path = install_dir.join(binary_filename);
        std::fs::write(&entrypoint_path, &binary_bytes).map_err(|e| {
            RunnerError::internal(format!("Failed to write plugin binary: {}", e))
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&entrypoint_path)
                .map_err(|e| RunnerError::internal(e.to_string()))?
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&entrypoint_path, perms)
                .map_err(|e| RunnerError::internal(e.to_string()))?;
        }

        // 10. Copy the package file for reference.
        let package_filename = package_path.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| RunnerError::invalid_argument(
                "Plugin package path has no valid filename component."
            ))?;
        let stored_package_path = install_dir.join(package_filename);
        std::fs::copy(package_path, &stored_package_path).map_err(|e| {
            RunnerError::internal(format!("Failed to copy package file: {}", e))
        })?;

        // 11. Write manifest.json into the install dir.
        std::fs::write(install_dir.join("manifest.json"), &manifest_json).map_err(|e| {
            RunnerError::internal(format!("Failed to write manifest: {}", e))
        })?;

        let now = Utc::now().to_rfc3339();
        let installation_id = format!(
            "inst:{}:{}:{}",
            manifest.plugin_id, manifest.version, &computed_sha256[..8]
        );

        // 12. Persist to database.
        let risk_profile = risk_profile_str.to_string();

        self.registry_repo.upsert(&PluginRegistryEntry {
            plugin_id: manifest.plugin_id.clone(),
            display_name: manifest.display_name.clone(),
            current_version: manifest.version.clone(),
            publisher: manifest.publisher.clone(),
            description: manifest.description.clone(),
            runtime_type: manifest.runtime.r#type.clone(),
            protocol_family: Some(manifest.protocol.family.clone()),
            protocol_version: Some(manifest.protocol.version.clone()),
            trust_status: trust.trust_status.clone(),
            risk_profile: risk_profile.clone(),
            enabled_flag: true,
            installed_at: now.clone(),
            updated_at: now.clone(),
        })?;

        // Defensively clear any pre-existing row with the same installation_id
        // (e.g. from an earlier install/uninstall cycle that left an orphaned
        // row). FK CASCADE removes any orphaned dependent rows as well.
        self.installation_repo.delete(&installation_id)?;

        self.installation_repo.insert(&PluginInstallationEntry {
            installation_id: installation_id.clone(),
            plugin_id: manifest.plugin_id.clone(),
            version: manifest.version.clone(),
            os: current_os,
            arch: current_arch,
            install_dir: install_dir.display().to_string(),
            package_path: stored_package_path.display().to_string(),
            entrypoint_path: entrypoint_path.display().to_string(),
            manifest_json: manifest_json.clone(),
            artifact_sha256: computed_sha256,
            signature_status: trust.signature_status.clone(),
            hash_ok_flag: true,
            quarantined_flag: false,
            installed_at: now.clone(),
            last_verified_at: Some(now.clone()),
            trust_tier: Some(trust.trust_tier.clone()),
            resolved_key_id: trust.resolved_key_id.clone(),
            capabilities_hash: Some(caps_hash),
        })?;

        // 13. Persist capabilities.
        //
        // `ocp-json/1` splits what used to be a single `capabilities` object into
        // two places:
        //   - `capabilities`: array of dotted feature flags (e.g. `events.progress`).
        //   - `sandbox`: object with `fs_read`, `fs_write`, `network`.
        // We persist both into `plugin_capabilities` so capability-change diffs
        // keep seeing the full declaration surface.
        if let Some(caps_arr) = manifest.capabilities.as_array() {
            for value in caps_arr {
                if let Some(name) = value.as_str() {
                    self.installation_repo.insert_capability(&PluginCapabilityEntry {
                        installation_id: installation_id.clone(),
                        category: name.to_string(),
                        scope_json: None,
                        declared_value: "true".to_string(),
                    })?;
                }
            }
        }
        if let Some(sandbox_obj) = manifest.sandbox.as_ref().and_then(|s| s.as_object()) {
            for (category, value) in sandbox_obj {
                self.installation_repo.insert_capability(&PluginCapabilityEntry {
                    installation_id: installation_id.clone(),
                    category: format!("sandbox.{}", category),
                    scope_json: Some(value.to_string()),
                    declared_value: value.to_string(),
                })?;
            }
        }

        // 14. Upsert snippets (non-fatal).
        if let Some(snippets) = &manifest.snippets {
            for snippet in snippets {
                let composite_id = format!("{}:{}", manifest.plugin_id, snippet.id);
                if let Err(e) = self.snippets_repo.upsert_plugin(
                    &composite_id, &snippet.title, &snippet.body, &manifest.plugin_id,
                ) {
                    eprintln!(
                        "[oc-install] WARNING: failed to upsert snippet '{}': {}",
                        snippet.id, e
                    );
                }
            }
        }

        // 15. Extract and cache static zip assets (non-fatal).
        for cmd in &manifest.commands {
            let assets: &[(&str, String)] = &[
                ("schema",   format!("schemas/{}.schema.json", cmd)),
                ("examples", format!("examples/{}.json", cmd)),
                ("help",     format!("help/{}.json", cmd)),
                ("outputs",  format!("outputs/{}.json", cmd)),
            ];
            for (kind, zip_path) in assets {
                if let Ok(json) = read_archive_file(&mut archive, zip_path) {
                    if let Err(e) = self.content_cache_repo.upsert(&CachedContentEntry {
                        installation_id: installation_id.clone(),
                        content_kind: kind.to_string(),
                        content_key: Some(cmd.clone()),
                        payload_json: json,
                        fetched_at: now.clone(),
                        invalidated_at: None,
                    }) {
                        eprintln!(
                            "[oc-install] WARNING: failed to cache '{}' asset for command '{}': {}",
                            kind, cmd, e
                        );
                    }
                }
            }
        }

        // 15b. Index endpoints so Open Choice's Help Explorer search finds this
        // plugin without requiring a manual "Discover" step. Non-fatal: search
        // can always be rebuilt on demand by the host app.
        if let Err(e) = self.index_endpoints(&manifest.plugin_id, &installation_id, &manifest.commands) {
            eprintln!(
                "[oc-install] WARNING: failed to index endpoints for '{}': {}",
                manifest.plugin_id, e
            );
        }

        // 16. Audit log.
        let severity = if trust.trust_status == "warning" { "warning" } else { "info" };
        let warning_notes = if trust.warnings.is_empty() {
            String::new()
        } else {
            format!(" Warnings: {}", trust.warnings.join("; "))
        };
        self.log_event(
            &installation_id,
            "install",
            severity,
            &format!(
                "Plugin '{}' v{} installed. trust_status={}, trust_tier={}, signature_status={}.{}",
                manifest.plugin_id, manifest.version,
                trust.trust_status, trust.trust_tier, trust.signature_status,
                warning_notes,
            ),
        );

        Ok(InstalledPluginResult {
            plugin_id: manifest.plugin_id,
            installation_id,
            trust_status: trust.trust_status,
            trust_tier: trust.trust_tier,
            signature_status: trust.signature_status,
            warnings: trust.warnings,
        })
    }

    // ── Uninstall ─────────────────────────────────────────────────────────────

    /// Mark a plugin as uninstalled. If `remove_files` is true, the install
    /// directory is also deleted from disk (irreversible).
    ///
    /// Robust against orphaned state: looks at any installation row (including
    /// quarantined) for file-removal info, then hard-deletes every installation
    /// row for the plugin. Succeeds if any installation row OR registry row
    /// existed; only errors when the plugin is genuinely unknown.
    pub fn uninstall_plugin(&self, plugin_id: &str, remove_files: bool) -> Result<(), RunnerError> {
        let installation = self.installation_repo.get_any(plugin_id)?;
        let registry = self.registry_repo.get(plugin_id)?;

        if installation.is_none() && registry.is_none() {
            return Err(RunnerError::plugin_not_found(
                format!("Plugin '{}' is not installed.", plugin_id)
            ));
        }

        if remove_files {
            if let Some(ref inst) = installation {
                let install_dir = std::path::PathBuf::from(&inst.install_dir);
                if install_dir.exists() {
                    std::fs::remove_dir_all(&install_dir).map_err(|e| {
                        RunnerError::internal(format!(
                            "Failed to remove install directory '{}': {}", install_dir.display(), e
                        ))
                    })?;
                }
            }
        }

        // Hard-delete every installation row for this plugin; FK CASCADE removes
        // plugin_capabilities, plugin_runtime_events, plugin_content_cache, and
        // plugin_endpoints rows.
        self.installation_repo.delete_all_for_plugin(plugin_id)?;

        if registry.is_some() {
            let now = Utc::now().to_rfc3339();
            self.registry_repo.set_trust_status(plugin_id, "uninstalled", &now)?;
        }

        Ok(())
    }

    // ── Verify ────────────────────────────────────────────────────────────────

    /// Re-verify the binary hash of the current installation.
    ///
    /// Returns `Ok(true)` if the hash still matches, `Ok(false)` if it has
    /// been tampered with (the installation is quarantined automatically).
    pub fn verify_installed(&self, plugin_id: &str) -> Result<bool, RunnerError> {
        let installation = self.installation_repo
            .get_current(plugin_id)?
            .ok_or_else(|| RunnerError::plugin_not_found(
                format!("Plugin '{}' is not installed.", plugin_id)
            ))?;

        let path = std::path::PathBuf::from(&installation.entrypoint_path);
        let now = Utc::now().to_rfc3339();

        if !path.exists() {
            self.installation_repo.set_quarantined(&installation.installation_id, true)?;
            self.registry_repo.set_trust_status(plugin_id, "quarantined", &now)?;
            self.log_event(
                &installation.installation_id,
                "entrypoint_missing",
                "error",
                &format!(
                    "Plugin '{}' entrypoint not found at '{}'. Quarantined.",
                    plugin_id, installation.entrypoint_path
                ),
            );
            return Ok(false);
        }

        let bytes = std::fs::read(&path).map_err(|e| {
            RunnerError::internal(format!("Failed to read plugin binary for verification: {}", e))
        })?;
        let computed = compute_sha256_bytes(&bytes);
        let matches = computed == installation.artifact_sha256;

        self.installation_repo.set_hash_ok(&installation.installation_id, matches, &now)?;

        if !matches {
            self.installation_repo.set_quarantined(&installation.installation_id, true)?;
            self.registry_repo.set_trust_status(plugin_id, "quarantined", &now)?;
            self.log_event(
                &installation.installation_id,
                "hash_mismatch_post_install",
                "error",
                &format!(
                    "Plugin '{}' binary hash mismatch. Expected: {}, computed: {}. Quarantined.",
                    plugin_id, installation.artifact_sha256, computed
                ),
            );
        } else {
            self.log_event(
                &installation.installation_id,
                "verify_ok",
                "info",
                &format!("Plugin '{}' binary hash verified OK.", plugin_id),
            );
        }

        Ok(matches)
    }

    // ── Trust evaluation ──────────────────────────────────────────────────────

    fn evaluate_trust(
        &self,
        manifest: &PluginManifest,
        manifest_json: &str,
        archive: &mut zip::ZipArchive<std::fs::File>,
        key_store: &TrustedKeyStore,
    ) -> Result<TrustDecision, RunnerError> {
        let signing = match &manifest.signing {
            None => return self.handle_unsigned_package(manifest),
            Some(s) => s,
        };

        let sig_bytes = match read_archive_bytes(archive, &signing.signature_path) {
            Ok(bytes) => bytes,
            Err(_) => {
                let mut dec = self.handle_unsigned_package(manifest)?;
                dec.warnings.insert(
                    0,
                    format!(
                        "manifest.json declares a signing section but '{}' was not found in the archive.",
                        signing.signature_path
                    ),
                );
                return Ok(dec);
            }
        };

        match key_store.verify(&signing.key_id, manifest_json.as_bytes(), &sig_bytes) {
            Ok(trust_tier) => Ok(TrustDecision {
                trust_status: "verified".to_string(),
                trust_tier,
                signature_status: "verified".to_string(),
                resolved_key_id: Some(signing.key_id.clone()),
                warnings: Vec::new(),
            }),
            Err(e) if e.code == "UNTRUSTED_PUBLISHER" => {
                if self.developer_mode {
                    Ok(TrustDecision {
                        trust_status: "warning".to_string(),
                        trust_tier: "self_signed".to_string(),
                        signature_status: "unverified".to_string(),
                        resolved_key_id: None,
                        warnings: vec![format!(
                            "Publisher signing key '{}' is not in the trusted key store. \
                             Installed in developer mode with trust_status=warning.",
                            signing.key_id
                        )],
                    })
                } else {
                    Err(RunnerError::untrusted_publisher(format!(
                        "Publisher signing key '{}' is not recognized. \
                         Pass --developer-mode to install packages from unrecognized publishers.",
                        signing.key_id
                    )))
                }
            }
            Err(e) if e.code == "SIGNATURE_VERIFICATION_FAILED" => {
                Err(RunnerError::signature_verification_failed(format!(
                    "Ed25519 signature verification FAILED for plugin '{}' v{}. \
                     The manifest content does not match its signature under key '{}'. \
                     The package may have been tampered with.",
                    manifest.plugin_id, manifest.version, signing.key_id
                )))
            }
            Err(e) => Err(e),
        }
    }

    fn handle_unsigned_package(&self, manifest: &PluginManifest) -> Result<TrustDecision, RunnerError> {
        if self.developer_mode {
            Ok(TrustDecision {
                trust_status: "warning".to_string(),
                trust_tier: "unsigned".to_string(),
                signature_status: "unsigned".to_string(),
                resolved_key_id: None,
                warnings: vec![
                    "This plugin package has no publisher signature. \
                     Installed in developer mode with trust_status=warning. \
                     Do not install unsigned packages from untrusted sources."
                        .to_string(),
                ],
            })
        } else {
            Err(RunnerError::unsigned_package(format!(
                "Plugin '{}' v{} has no publisher signature and cannot be installed. \
                 Pass --developer-mode to install unsigned packages.",
                manifest.plugin_id, manifest.version
            )))
        }
    }

    // ── Audit logging ─────────────────────────────────────────────────────────

    fn log_event(&self, installation_id: &str, event_type: &str, severity: &str, message: &str) {
        let now = Utc::now().to_rfc3339();
        let entry = PluginRuntimeEventEntry {
            event_id: format!("evt:{}:{}", installation_id, now),
            installation_id: installation_id.to_string(),
            event_type: event_type.to_string(),
            severity: severity.to_string(),
            message: message.to_string(),
            detail_json: None,
            created_at: now,
        };
        if let Err(e) = self.events_repo.insert(&entry) {
            eprintln!("[oc-install] WARNING: failed to persist audit event: {}", e);
        }
    }

    /// Populate `plugin_endpoints` rows for each declared command so the
    /// Open Choice Help Explorer can surface this plugin immediately, without
    /// a manual "Discover endpoints" step.
    ///
    /// `search_text` mirrors what the Tauri-app protocol service writes
    /// (`endpoint_id + summary + parameter names`) so search results are
    /// consistent across install paths.
    fn index_endpoints(
        &self,
        plugin_id: &str,
        installation_id: &str,
        commands: &[String],
    ) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        for cmd in commands {
            let (summary, param_names) = self.content_cache_repo
                .get(installation_id, "help", Some(cmd))
                .ok()
                .flatten()
                .and_then(|payload| serde_json::from_str::<IndexingExplainData>(&payload).ok())
                .map(|d| {
                    let params: Vec<String> = d.fields.into_iter().map(|f| f.name).collect();
                    (Some(d.summary), params)
                })
                .unwrap_or((None, Vec::new()));

            let title = title_case_endpoint(cmd);
            let base = format!("{} {}", cmd, summary.as_deref().unwrap_or(""));
            let search_text = if param_names.is_empty() {
                base.trim().to_string()
            } else {
                format!("{} {}", base.trim(), param_names.join(" "))
            };
            let search_text = if search_text.is_empty() { None } else { Some(search_text) };

            let id = format!("{}:{}", installation_id, cmd);
            conn.execute(
                "INSERT INTO plugin_endpoints (id, installation_id, plugin_id, endpoint_id, title, description, search_text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(installation_id, endpoint_id) DO UPDATE SET
                   title = excluded.title,
                   description = excluded.description,
                   search_text = excluded.search_text",
                rusqlite::params![
                    id,
                    installation_id,
                    plugin_id,
                    cmd,
                    title,
                    summary,
                    search_text,
                ],
            )
            .map_err(|e| RunnerError::database(e.to_string()))?;
        }
        Ok(())
    }
}

// Minimal shape used only for the install-time endpoint index. Kept here
// instead of reusing `query::ExplainData` so this module stays independent.
#[derive(Debug, Deserialize)]
struct IndexingExplainData {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    fields: Vec<IndexingExplainField>,
}

#[derive(Debug, Deserialize)]
struct IndexingExplainField {
    name: String,
}

/// Tauri-app title-case: `"cv-fit"` -> `"Cv Fit"`. Kept in sync with
/// `title_case` in the host app's `plugin_protocol_service.rs`.
fn title_case_endpoint(s: &str) -> String {
    s.split(|c: char| c == '-' || c == '_' || c == ' ')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().chain(chars.flat_map(|c| c.to_lowercase())).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn validate_manifest(manifest: &PluginManifest) -> Result<(), RunnerError> {
    if manifest.schema_version.is_empty() {
        return Err(RunnerError::invalid_argument("manifest.json missing schema_version"));
    }
    if manifest.plugin_id.is_empty() {
        return Err(RunnerError::invalid_argument("manifest.json missing plugin_id"));
    }
    for ch in ['/', '\\', ':'] {
        if manifest.plugin_id.contains(ch) {
            return Err(RunnerError::invalid_argument(format!(
                "manifest.json plugin_id '{}' contains an illegal character '{}'.",
                manifest.plugin_id, ch
            )));
        }
        if manifest.version.contains(ch) {
            return Err(RunnerError::invalid_argument(format!(
                "manifest.json version '{}' contains an illegal character '{}'.",
                manifest.version, ch
            )));
        }
    }
    if manifest.display_name.is_empty() {
        return Err(RunnerError::invalid_argument("manifest.json missing display_name"));
    }
    if manifest.version.is_empty() {
        return Err(RunnerError::invalid_argument("manifest.json missing version"));
    }
    if manifest.runtime.r#type != "native-sidecar" {
        return Err(RunnerError::invalid_argument(format!(
            "Unsupported runtime type '{}'. Only 'native-sidecar' is supported.",
            manifest.runtime.r#type
        )));
    }
    if manifest.runtime.entrypoints.is_empty() {
        return Err(RunnerError::invalid_argument("manifest.json has no runtime.entrypoints"));
    }
    for ep in &manifest.runtime.entrypoints {
        if ep.digest.algorithm != "sha256" {
            return Err(RunnerError::invalid_argument(format!(
                "Entrypoint '{}-{}' declares unsupported digest algorithm '{}' (only 'sha256' is supported).",
                ep.os, ep.arch, ep.digest.algorithm
            )));
        }
        if ep.digest.value.is_empty() {
            return Err(RunnerError::invalid_argument(format!(
                "Entrypoint '{}-{}' missing digest value", ep.os, ep.arch
            )));
        }
        if ep.digest.value.len() != 64 || ep.digest.value.chars().any(|c| !c.is_ascii_hexdigit()) {
            return Err(RunnerError::invalid_argument(format!(
                "Entrypoint '{}-{}' has an invalid digest value (expected 64 lowercase hex chars for sha256).",
                ep.os, ep.arch
            )));
        }
    }
    Ok(())
}

const MAX_ARCHIVE_TEXT_BYTES: u64 = 4 * 1024 * 1024;

fn read_archive_file(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<String, RunnerError> {
    let mut entry = archive.by_name(name).map_err(|_| {
        RunnerError::invalid_argument(format!("Plugin package is missing '{}'", name))
    })?;
    let mut bytes = Vec::new();
    entry.by_ref()
        .take(MAX_ARCHIVE_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| RunnerError::invalid_argument(format!("Failed to read '{}': {}", name, e)))?;
    if bytes.len() as u64 > MAX_ARCHIVE_TEXT_BYTES {
        return Err(RunnerError::invalid_argument(format!(
            "Plugin entry '{}' exceeds the 4 MB text file size limit.", name
        )));
    }
    String::from_utf8(bytes)
        .map_err(|e| RunnerError::invalid_argument(format!("'{}' contains invalid UTF-8: {}", name, e)))
}

const MAX_ARCHIVE_EXTRACT_BYTES: u64 = 200 * 1024 * 1024;

fn read_archive_bytes(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>, RunnerError> {
    let mut entry = archive.by_name(name).map_err(|_| {
        RunnerError::invalid_argument(format!("Plugin package is missing '{}'", name))
    })?;
    let mut bytes = Vec::new();
    entry.by_ref()
        .take(MAX_ARCHIVE_EXTRACT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| RunnerError::invalid_argument(format!("Failed to read '{}': {}", name, e)))?;
    if bytes.len() as u64 > MAX_ARCHIVE_EXTRACT_BYTES {
        return Err(RunnerError::invalid_argument(format!(
            "Plugin entry '{}' exceeds the 200 MB extraction size limit.", name
        )));
    }
    Ok(bytes)
}

fn compute_sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn compute_capabilities_hash(
    capabilities: &serde_json::Value,
    sandbox: Option<&serde_json::Value>,
    risk_profile: &str,
) -> String {
    let caps_canonical = serde_json::to_string(capabilities)
        .expect("serde_json::Value is always serializable");
    let sandbox_canonical = sandbox
        .map(|v| serde_json::to_string(v).expect("serde_json::Value is always serializable"))
        .unwrap_or_else(|| "null".into());
    let mut hasher = Sha256::new();
    hasher.update(caps_canonical.as_bytes());
    hasher.update(b"|");
    hasher.update(sandbox_canonical.as_bytes());
    hasher.update(b"|");
    hasher.update(risk_profile.as_bytes());
    hex::encode(hasher.finalize())
}

fn current_os() -> String {
    #[cfg(target_os = "windows")] { "windows".to_string() }
    #[cfg(target_os = "macos")]   { "macos".to_string() }
    #[cfg(target_os = "linux")]   { "linux".to_string() }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    { "unknown".to_string() }
}

fn current_arch() -> String {
    #[cfg(target_arch = "x86_64")]  { "x86_64".to_string() }
    #[cfg(target_arch = "aarch64")] { "aarch64".to_string() }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    { "unknown".to_string() }
}
