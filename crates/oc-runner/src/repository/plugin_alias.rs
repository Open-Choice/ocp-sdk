use crate::db::Db;
use crate::errors::RunnerError;

#[derive(Debug, Clone)]
pub struct PluginAliasEntry {
    pub alias: String,
    pub plugin_id: String,
    pub version: Option<String>,
}

pub struct PluginAliasRepository {
    db: Db,
}

impl PluginAliasRepository {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    pub fn list(&self) -> Result<Vec<PluginAliasEntry>, RunnerError> {
        let conn = self.db.connect()?;
        let mut stmt = conn
            .prepare("SELECT alias, plugin_id, version FROM plugin_aliases ORDER BY alias ASC")
            .map_err(|e| RunnerError::database(e.to_string()))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(PluginAliasEntry {
                    alias: row.get(0)?,
                    plugin_id: row.get(1)?,
                    version: row.get(2)?,
                })
            })
            .map_err(|e| RunnerError::database(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| RunnerError::database(e.to_string()))
    }
}
