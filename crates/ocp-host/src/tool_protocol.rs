use ocp_types_v1::responses::{SelfTestResponsePayload, ValidateResponsePayload};
use ocp_types_v1::{Envelope, EnvelopeClass};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Wall-clock ceiling on a single protocol command (`api validate`,
/// `api self-test`). Without this, a hung plugin blocks the calling
/// thread — usually the Tauri command worker — indefinitely.
const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(30);

/// How often to check whether the child has exited while waiting on the
/// timeout. 20 ms keeps tail-latency low without busy-spinning.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Hard ceiling on the bytes we'll buffer from a single command's stdout
/// or stderr. Validation responses are typically a few KiB. 8 MiB lets
/// reasonable payloads through while bounding memory against a malicious
/// or runaway plugin.
const MAX_PROTOCOL_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ProtocolClientError {
    #[error("failed to spawn tool process: {0}")]
    Spawn(String),
    #[error("tool process returned non-success status for `{command}`: {stderr}")]
    Status { command: String, stderr: String },
    #[error("failed to parse JSON response for `{command}`: {source}")]
    Json {
        command: String,
        source: serde_json::Error,
    },
    #[error("failed to serialize input JSON: {0}")]
    Serialize(String),
    #[error("tool process timed out after {seconds}s for `{command}`")]
    Timeout { command: String, seconds: u64 },
    #[error(
        "tool process produced more than {limit_bytes} bytes of output for `{command}`"
    )]
    OversizedOutput { command: String, limit_bytes: usize },
    #[error("unexpected envelope for `{command}`: {reason}")]
    UnexpectedEnvelope { command: String, reason: String },
}

/// A parsed `response.*` envelope together with its deserialized payload.
#[derive(Debug, Clone)]
pub struct ResponseEnvelope<P> {
    pub envelope: Envelope,
    pub payload: P,
}

#[derive(Debug, Clone)]
pub struct ToolProtocolClient {
    executable_path: PathBuf,
}

impl ToolProtocolClient {
    pub fn new(executable_path: impl Into<PathBuf>) -> Self {
        Self {
            executable_path: executable_path.into(),
        }
    }

    pub fn validate_value(
        &self,
        command: &str,
        input_value: &Value,
    ) -> Result<ResponseEnvelope<ValidateResponsePayload>, ProtocolClientError> {
        // The plugin contract (`ocp-json/1`) defines `--input-json` as a literal
        // JSON string, not a path. Serialize compactly so the command line stays
        // well under the Windows CreateProcess limit (~32 KiB) for typical
        // validation payloads.
        let input_json = serde_json::to_string(input_value)
            .map_err(|err| ProtocolClientError::Serialize(err.to_string()))?;
        self.run_response(
            "response.validate",
            &[
                "api",
                "validate",
                "--command",
                command,
                "--input-json",
                &input_json,
            ],
        )
    }

    pub fn self_test(&self) -> Result<ResponseEnvelope<SelfTestResponsePayload>, ProtocolClientError> {
        self.run_response("response.self_test", &["api", "self-test"])
    }

    fn run_response<P: DeserializeOwned>(
        &self,
        expected_kind: &str,
        args: &[&str],
    ) -> Result<ResponseEnvelope<P>, ProtocolClientError> {
        let command_string = args.join(" ");
        let stdout_buf = self.run_raw(args, &command_string)?;

        let envelope: Envelope = serde_json::from_slice(&stdout_buf).map_err(|source| {
            ProtocolClientError::Json {
                command: command_string.clone(),
                source,
            }
        })?;

        if !envelope.is_ocp_v1() {
            return Err(ProtocolClientError::UnexpectedEnvelope {
                command: command_string,
                reason: format!("expected ocp=\"1\", got ocp={:?}", envelope.ocp),
            });
        }
        if envelope.class != EnvelopeClass::Response {
            return Err(ProtocolClientError::UnexpectedEnvelope {
                command: command_string,
                reason: format!("expected class=response, got class={:?}", envelope.class),
            });
        }
        if envelope.kind.as_str() != expected_kind {
            return Err(ProtocolClientError::UnexpectedEnvelope {
                command: command_string,
                reason: format!(
                    "expected kind={expected_kind}, got kind={}",
                    envelope.kind.as_str()
                ),
            });
        }

        let payload_value = envelope.payload.clone().ok_or_else(|| {
            ProtocolClientError::UnexpectedEnvelope {
                command: command_string.clone(),
                reason: "envelope has no payload".into(),
            }
        })?;
        let payload: P = serde_json::from_value(payload_value).map_err(|source| {
            ProtocolClientError::Json {
                command: command_string,
                source,
            }
        })?;

        Ok(ResponseEnvelope { envelope, payload })
    }

    fn run_raw(&self, args: &[&str], command_string: &str) -> Result<Vec<u8>, ProtocolClientError> {
        // Validate the path before spawning to prevent path-traversal exploitation.
        // A relative path or one containing `..` components could resolve to an arbitrary
        // binary outside the plugin install directory.
        if !self.executable_path.is_absolute() {
            return Err(ProtocolClientError::Spawn(
                "Executable path must be absolute.".to_string(),
            ));
        }
        if self.executable_path.components().any(|c| c == std::path::Component::ParentDir) {
            return Err(ProtocolClientError::Spawn(
                "Executable path must not contain '..' components.".to_string(),
            ));
        }

        // Spawn manually instead of using `Command::output()` so we can enforce
        // a wall-clock timeout. `output()` blocks indefinitely on a hung plugin.
        // Drain stdout/stderr on background threads with a hard byte cap so a
        // chatty plugin can't OOM the host.
        let mut child = Command::new(&self.executable_path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| ProtocolClientError::Spawn(err.to_string()))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProtocolClientError::Spawn("stdout pipe was not available".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProtocolClientError::Spawn("stderr pipe was not available".to_string()))?;

        // Reading both pipes on threads is mandatory: a single-threaded reader
        // can deadlock when one pipe's OS buffer fills while we're blocked on
        // the other. `take(MAX + 1)` lets us detect overflow without unbounded
        // allocation.
        let stdout_handle = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout
                .take(MAX_PROTOCOL_OUTPUT_BYTES as u64 + 1)
                .read_to_end(&mut buf);
            buf
        });
        let stderr_handle = thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr
                .take(MAX_PROTOCOL_OUTPUT_BYTES as u64 + 1)
                .read_to_end(&mut buf);
            buf
        });

        let deadline = Instant::now() + PROTOCOL_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        // Joining drains the reader threads cleanly now that
                        // the pipes have closed; otherwise we leak descriptors.
                        let _ = stdout_handle.join();
                        let _ = stderr_handle.join();
                        return Err(ProtocolClientError::Timeout {
                            command: command_string.to_string(),
                            seconds: PROTOCOL_TIMEOUT.as_secs(),
                        });
                    }
                    thread::sleep(POLL_INTERVAL);
                }
                Err(err) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(ProtocolClientError::Spawn(err.to_string()));
                }
            }
        };

        let stdout_buf = stdout_handle.join().unwrap_or_default();
        let stderr_buf = stderr_handle.join().unwrap_or_default();

        if stdout_buf.len() > MAX_PROTOCOL_OUTPUT_BYTES
            || stderr_buf.len() > MAX_PROTOCOL_OUTPUT_BYTES
        {
            return Err(ProtocolClientError::OversizedOutput {
                command: command_string.to_string(),
                limit_bytes: MAX_PROTOCOL_OUTPUT_BYTES,
            });
        }

        if !status.success() {
            return Err(ProtocolClientError::Status {
                command: command_string.to_string(),
                stderr: String::from_utf8_lossy(&stderr_buf).trim().to_string(),
            });
        }

        Ok(stdout_buf)
    }
}
