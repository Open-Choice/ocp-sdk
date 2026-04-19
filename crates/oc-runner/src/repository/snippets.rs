use rusqlite::params;

use crate::db::Db;
use crate::errors::RunnerError;

pub struct SnippetsRepository {
    db: Db,
}

impl SnippetsRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Upsert a plugin-contributed snippet. Used on plugin install.
    pub fn upsert_plugin(&self, id: &str, title: &str, body: &str, plugin_id: &str) -> Result<(), RunnerError> {
        let conn = self.db.connect()?;
        conn.execute(
            "INSERT INTO snippets (id, title, body, source, plugin_id)
             VALUES (?1, ?2, ?3, 'plugin', ?4)
             ON CONFLICT(id) DO UPDATE SET title = excluded.title, body = excluded.body",
            params![id, title, body, plugin_id],
        )
        .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(())
    }
}
