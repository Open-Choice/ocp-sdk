use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::errors::RunnerError;

/// Lightweight handle to the Open Choice SQLite database.
///
/// Unlike the Tauri app's `Db`, this type does not run migrations — it opens
/// an existing database that was already initialised by Open Choice.
#[derive(Debug, Clone)]
pub struct Db {
    path: PathBuf,
}

impl Db {
    /// Open an existing Open Choice database. Returns an error if the file
    /// does not exist or cannot be opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RunnerError> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Err(RunnerError::db_not_found(&path));
        }
        // Verify connectivity up-front so callers get a clear error immediately.
        Connection::open(&path).map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(Self { path })
    }

    pub fn connect(&self) -> Result<Connection, RunnerError> {
        let conn = Connection::open(&self.path)
            .map_err(|e| RunnerError::database(e.to_string()))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|e| RunnerError::database(e.to_string()))?;
        Ok(conn)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the default database path for the current platform.
    ///
    /// Mirrors where the Open Choice Tauri app stores its database:
    /// `{data_dir}/com.numerious.openchoice/open_choice.db`
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_dir().map(|d: PathBuf| d.join("com.numerious.openchoice").join("open_choice.db"))
    }

    /// Returns the default plugins directory for the current platform.
    ///
    /// Mirrors where the Open Choice Tauri app extracts plugin binaries:
    /// `{data_dir}/com.numerious.openchoice/plugins`
    pub fn default_plugins_dir() -> Option<PathBuf> {
        dirs::data_dir().map(|d: PathBuf| d.join("com.numerious.openchoice").join("plugins"))
    }
}
