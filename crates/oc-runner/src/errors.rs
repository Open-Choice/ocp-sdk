use std::fmt;

#[derive(Debug, Clone)]
pub struct RunnerError {
    pub code: &'static str,
    pub message: String,
}

impl RunnerError {
    pub fn database(msg: impl Into<String>) -> Self {
        Self { code: "DATABASE", message: msg.into() }
    }
    pub fn parse(msg: impl Into<String>) -> Self {
        Self { code: "PARSE", message: msg.into() }
    }
    pub fn plugin_not_found(msg: impl Into<String>) -> Self {
        Self { code: "PLUGIN_NOT_FOUND", message: msg.into() }
    }
    pub fn process_spawn_failed(msg: impl Into<String>) -> Self {
        Self { code: "PROCESS_SPAWN_FAILED", message: msg.into() }
    }
    pub fn timeout(secs: u64) -> Self {
        Self { code: "TIMEOUT", message: format!("Task timed out after {} seconds.", secs) }
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        Self { code: "INTERNAL", message: msg.into() }
    }
    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Self { code: "INVALID_ARGUMENT", message: msg.into() }
    }
    pub fn db_not_found(path: &std::path::Path) -> Self {
        Self {
            code: "DB_NOT_FOUND",
            message: format!(
                "Open Choice database not found at '{}'. Is Open Choice installed?",
                path.display()
            ),
        }
    }
    pub fn plugin_revoked(msg: impl Into<String>) -> Self {
        Self { code: "PLUGIN_REVOKED", message: msg.into() }
    }
    pub fn untrusted_publisher(msg: impl Into<String>) -> Self {
        Self { code: "UNTRUSTED_PUBLISHER", message: msg.into() }
    }
    pub fn signature_verification_failed(msg: impl Into<String>) -> Self {
        Self { code: "SIGNATURE_VERIFICATION_FAILED", message: msg.into() }
    }
    pub fn unsigned_package(msg: impl Into<String>) -> Self {
        Self { code: "UNSIGNED_PACKAGE", message: msg.into() }
    }
}

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for RunnerError {}
