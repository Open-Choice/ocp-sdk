use rusqlite::params;

use crate::db::Db;
use crate::errors::RunnerError;

#[derive(Debug, Clone)]
pub struct CachedContentEntry {
    pub installation_id: String,
    pub content_kind: String,
    pub content_key: Option<String>,
    pub payload_json: String,
    pub fetched_at: String,
    pub invalidated_at: Option<String>,
}

pub struct PluginContentCacheRepository {
    db: Db,
}

impl PluginContentCacheRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Returns the cached payload JSON for a given (installation, kind, key) triple,
    /// or `None` if not present or invalidated.
    pub fn get(
        &self,
        installation_id: &str,
        content_kind: &str,
        content_key: Option<&str>,
    ) -> Result<Option<String>, RunnerError> {
        let conn = self.db.connect()?;
        let result = conn.query_row(
            "SELECT payload_json
             FROM plugin_content_cache
             WHERE installation_id = ?1
               AND content_kind    = ?2
               AND ((content_key IS NULL AND ?3 IS NULL) OR content_key = ?3)
               AND invalidated_at IS NULL",
            params![installation_id, content_kind, content_key],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(json) => Ok(Some(json)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(RunnerError::database(e.to_string())),
        }
    }

    pub fn upsert(&self, entry: &CachedContentEntry) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "INSERT INTO plugin_content_cache
                (installation_id, content_kind, content_key, payload_json, fetched_at, invalidated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(installation_id, content_kind, content_key) DO UPDATE SET
                payload_json   = excluded.payload_json,
                fetched_at     = excluded.fetched_at,
                invalidated_at = excluded.invalidated_at",
            params![
                &entry.installation_id,
                &entry.content_kind,
                &entry.content_key,
                &entry.payload_json,
                &entry.fetched_at,
                &entry.invalidated_at,
            ],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }
}
