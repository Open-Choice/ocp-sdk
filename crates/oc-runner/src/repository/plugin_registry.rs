use rusqlite::params;

use crate::db::Db;
use crate::errors::RunnerError;

#[derive(Debug, Clone)]
pub struct PluginRegistryEntry {
    pub plugin_id: String,
    pub display_name: String,
    pub current_version: String,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub runtime_type: String,
    pub protocol_family: Option<String>,
    pub protocol_version: Option<String>,
    pub trust_status: String,
    pub risk_profile: String,
    pub enabled_flag: bool,
    pub installed_at: String,
    pub updated_at: String,
}

pub struct PluginRegistryRepository {
    db: Db,
}

impl PluginRegistryRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn get(&self, plugin_id: &str) -> Result<Option<PluginRegistryEntry>, RunnerError> {
        let conn = self.db.connect()?;
        let result = conn.query_row(
            &format!("{} WHERE plugin_id = ?1", SELECT_SQL),
            params![plugin_id],
            map_entry,
        );
        match result {
            Ok(entry) => Ok(Some(entry)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(RunnerError::database(e.to_string())),
        }
    }

    pub fn list(&self) -> Result<Vec<PluginRegistryEntry>, RunnerError> {
        let conn = self.db.connect()?;
        let mut stmt = conn
            .prepare(SELECT_SQL)
            .map_err(|e| RunnerError::database(e.to_string()))?;
        let rows = stmt
            .query_map([], map_entry)
            .map_err(|e| RunnerError::database(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| RunnerError::database(e.to_string()))
    }

    pub fn upsert(&self, entry: &PluginRegistryEntry) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "INSERT INTO plugin_registry (
                plugin_id, display_name, current_version, publisher, description,
                runtime_type, protocol_family, protocol_version,
                trust_status, risk_profile, enabled_flag, installed_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(plugin_id) DO UPDATE SET
                display_name = excluded.display_name,
                current_version = excluded.current_version,
                publisher = excluded.publisher,
                description = excluded.description,
                runtime_type = excluded.runtime_type,
                protocol_family = excluded.protocol_family,
                protocol_version = excluded.protocol_version,
                trust_status = excluded.trust_status,
                risk_profile = excluded.risk_profile,
                enabled_flag = excluded.enabled_flag,
                updated_at = excluded.updated_at",
            params![
                &entry.plugin_id,
                &entry.display_name,
                &entry.current_version,
                &entry.publisher,
                &entry.description,
                &entry.runtime_type,
                &entry.protocol_family,
                &entry.protocol_version,
                &entry.trust_status,
                &entry.risk_profile,
                if entry.enabled_flag { 1i64 } else { 0i64 },
                &entry.installed_at,
                &entry.updated_at,
            ],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }

    pub fn set_trust_status(&self, plugin_id: &str, trust_status: &str, updated_at: &str) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "UPDATE plugin_registry SET trust_status = ?2, updated_at = ?3 WHERE plugin_id = ?1",
            params![plugin_id, trust_status, updated_at],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }
}

const SELECT_SQL: &str = "SELECT
    plugin_id, display_name, current_version, publisher, description,
    runtime_type, protocol_family, protocol_version,
    trust_status, risk_profile, enabled_flag, installed_at, updated_at
  FROM plugin_registry";

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<PluginRegistryEntry> {
    Ok(PluginRegistryEntry {
        plugin_id: row.get(0)?,
        display_name: row.get(1)?,
        current_version: row.get(2)?,
        publisher: row.get(3)?,
        description: row.get(4)?,
        runtime_type: row.get(5)?,
        protocol_family: row.get(6)?,
        protocol_version: row.get(7)?,
        trust_status: row.get(8)?,
        risk_profile: row.get(9)?,
        enabled_flag: row.get::<_, i64>(10)? != 0,
        installed_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}
