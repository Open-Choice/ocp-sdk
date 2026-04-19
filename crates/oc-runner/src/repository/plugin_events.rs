use rusqlite::params;

use crate::db::Db;
use crate::errors::RunnerError;

#[derive(Debug, Clone)]
pub struct PluginRuntimeEventEntry {
    pub event_id: String,
    pub installation_id: String,
    pub event_type: String,
    pub severity: String,
    pub message: String,
    pub detail_json: Option<String>,
    pub created_at: String,
}

pub struct PluginEventsRepository {
    db: Db,
}

impl PluginEventsRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn insert(&self, entry: &PluginRuntimeEventEntry) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "INSERT INTO plugin_runtime_events
                (event_id, installation_id, event_type, severity, message, detail_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                &entry.event_id,
                &entry.installation_id,
                &entry.event_type,
                &entry.severity,
                &entry.message,
                &entry.detail_json,
                &entry.created_at,
            ],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }
}
