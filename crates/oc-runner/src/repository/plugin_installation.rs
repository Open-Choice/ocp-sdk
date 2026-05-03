use rusqlite::params;
use semver;

use crate::db::Db;
use crate::errors::RunnerError;

#[derive(Debug, Clone)]
pub struct PluginInstallationEntry {
    pub installation_id: String,
    pub plugin_id: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub install_dir: String,
    pub package_path: String,
    pub entrypoint_path: String,
    pub manifest_json: String,
    pub artifact_sha256: String,
    pub signature_status: String,
    pub hash_ok_flag: bool,
    pub quarantined_flag: bool,
    pub installed_at: String,
    pub last_verified_at: Option<String>,
    pub trust_tier: Option<String>,
    pub resolved_key_id: Option<String>,
    pub capabilities_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PluginCapabilityEntry {
    pub installation_id: String,
    pub category: String,
    pub scope_json: Option<String>,
    pub declared_value: String,
}

pub struct PluginInstallationRepository {
    db: Db,
}

impl PluginInstallationRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Returns the highest-semver non-quarantined installation for a plugin, if any.
    pub fn get_current(&self, plugin_id: &str) -> Result<Option<PluginInstallationEntry>, RunnerError> {
        let conn = self.db.connect()?;
        let mut stmt = conn
            .prepare(&format!("{} WHERE plugin_id = ?1 AND quarantined_flag = 0", SELECT_SQL))
            .map_err(|e| RunnerError::database(e.to_string()))?;
        let rows = stmt
            .query_map(params![plugin_id], map_entry)
            .map_err(|e| RunnerError::database(e.to_string()))?;
        let mut entries: Vec<PluginInstallationEntry> = rows
            .filter_map(|r| r.ok())
            .collect();
        entries.sort_by(|a, b| {
            match (semver::Version::parse(&a.version), semver::Version::parse(&b.version)) {
                (Ok(va), Ok(vb)) => vb.cmp(&va),
                _ => b.installed_at.cmp(&a.installed_at),
            }
        });
        Ok(entries.into_iter().next())
    }

    pub fn insert(&self, entry: &PluginInstallationEntry) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "INSERT INTO plugin_installations (
                installation_id, plugin_id, version, os, arch,
                install_dir, package_path, entrypoint_path, manifest_json,
                artifact_sha256, signature_status, hash_ok_flag, quarantined_flag,
                installed_at, last_verified_at, trust_tier, resolved_key_id, capabilities_hash
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                &entry.installation_id,
                &entry.plugin_id,
                &entry.version,
                &entry.os,
                &entry.arch,
                &entry.install_dir,
                &entry.package_path,
                &entry.entrypoint_path,
                &entry.manifest_json,
                &entry.artifact_sha256,
                &entry.signature_status,
                if entry.hash_ok_flag { 1i64 } else { 0i64 },
                if entry.quarantined_flag { 1i64 } else { 0i64 },
                &entry.installed_at,
                &entry.last_verified_at,
                &entry.trust_tier,
                &entry.resolved_key_id,
                &entry.capabilities_hash,
            ],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }

    pub fn insert_capability(&self, cap: &PluginCapabilityEntry) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "INSERT INTO plugin_capabilities (installation_id, category, scope_json, declared_value)
             VALUES (?1, ?2, ?3, ?4)",
            params![&cap.installation_id, &cap.category, &cap.scope_json, &cap.declared_value],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }

    /// Hard-deletes the installation row. With foreign-key enforcement on,
    /// this cascades to plugin_capabilities, plugin_runtime_events,
    /// plugin_content_cache, and plugin_endpoints.
    pub fn delete(&self, installation_id: &str) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "DELETE FROM plugin_installations WHERE installation_id = ?1",
            params![installation_id],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }

    /// Returns every non-quarantined installation row for a plugin, ordered by
    /// `installed_at DESC` (most recent first). Used by the executor to build a
    /// version-keyed dispatch map so tasks with `::version::` pins can route
    /// to the right installation.
    pub fn list_for_plugin(&self, plugin_id: &str) -> Result<Vec<PluginInstallationEntry>, RunnerError> {
        let conn = self.db.connect()?;
        let mut stmt = conn
            .prepare(&format!(
                "{} WHERE plugin_id = ?1 AND quarantined_flag = 0",
                SELECT_SQL
            ))
            .map_err(|e| RunnerError::database(e.to_string()))?;
        let rows = stmt
            .query_map(params![plugin_id], map_entry)
            .map_err(|e| RunnerError::database(e.to_string()))?;
        let mut entries: Vec<PluginInstallationEntry> = rows
            .filter_map(|r| r.ok())
            .collect();
        entries.sort_by(|a, b| {
            match (semver::Version::parse(&a.version), semver::Version::parse(&b.version)) {
                (Ok(va), Ok(vb)) => vb.cmp(&va),
                _ => b.installed_at.cmp(&a.installed_at),
            }
        });
        Ok(entries)
    }

    /// Returns the highest-semver installation row for a plugin regardless of
    /// quarantine state. Used by uninstall to find rows that `get_current`
    /// would skip.
    pub fn get_any(&self, plugin_id: &str) -> Result<Option<PluginInstallationEntry>, RunnerError> {
        let conn = self.db.connect()?;
        let mut stmt = conn
            .prepare(&format!("{} WHERE plugin_id = ?1", SELECT_SQL))
            .map_err(|e| RunnerError::database(e.to_string()))?;
        let rows = stmt
            .query_map(params![plugin_id], map_entry)
            .map_err(|e| RunnerError::database(e.to_string()))?;
        let mut entries: Vec<PluginInstallationEntry> = rows
            .filter_map(|r| r.ok())
            .collect();
        entries.sort_by(|a, b| {
            match (semver::Version::parse(&a.version), semver::Version::parse(&b.version)) {
                (Ok(va), Ok(vb)) => vb.cmp(&va),
                _ => b.installed_at.cmp(&a.installed_at),
            }
        });
        Ok(entries.into_iter().next())
    }

    /// Hard-deletes every installation row for a plugin_id (including
    /// quarantined ones). Returns the number of rows removed.
    pub fn delete_all_for_plugin(&self, plugin_id: &str) -> Result<usize, RunnerError> {
        let conn = self.db.connect()?;
        let n = conn.execute(
            "DELETE FROM plugin_installations WHERE plugin_id = ?1",
            params![plugin_id],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(n)
    }

    pub fn set_quarantined(&self, installation_id: &str, quarantined: bool) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "UPDATE plugin_installations SET quarantined_flag = ?2 WHERE installation_id = ?1",
            params![installation_id, if quarantined { 1i64 } else { 0i64 }],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }

    pub fn set_hash_ok(&self, installation_id: &str, ok: bool, verified_at: &str) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "UPDATE plugin_installations SET hash_ok_flag = ?2, last_verified_at = ?3 WHERE installation_id = ?1",
            params![installation_id, if ok { 1i64 } else { 0i64 }, verified_at],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }
}

const SELECT_SQL: &str = "SELECT
    installation_id, plugin_id, version, os, arch,
    install_dir, package_path, entrypoint_path, manifest_json,
    artifact_sha256, signature_status, hash_ok_flag, quarantined_flag,
    installed_at, last_verified_at, trust_tier, resolved_key_id, capabilities_hash
  FROM plugin_installations";

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<PluginInstallationEntry> {
    Ok(PluginInstallationEntry {
        installation_id: row.get(0)?,
        plugin_id: row.get(1)?,
        version: row.get(2)?,
        os: row.get(3)?,
        arch: row.get(4)?,
        install_dir: row.get(5)?,
        package_path: row.get(6)?,
        entrypoint_path: row.get(7)?,
        manifest_json: row.get(8)?,
        artifact_sha256: row.get(9)?,
        signature_status: row.get(10)?,
        hash_ok_flag: row.get::<_, i64>(11)? != 0,
        quarantined_flag: row.get::<_, i64>(12)? != 0,
        installed_at: row.get(13)?,
        last_verified_at: row.get(14)?,
        trust_tier: row.get(15)?,
        resolved_key_id: row.get(16)?,
        capabilities_hash: row.get(17)?,
    })
}
