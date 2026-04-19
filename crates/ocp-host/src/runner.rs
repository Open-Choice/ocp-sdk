use chrono::Utc;
use ocp_types_v1::events::LogLinePayload;
use ocp_types_v1::{Envelope, EnvelopeClass, Identifier, Kind, Severity, Timestamp};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use thiserror::Error;
use ulid::Ulid;

/// Bounded capacity for the channel between the stdout reader thread and the
/// main event loop. A bounded channel applies backpressure: when the consumer
/// falls behind, the reader blocks on `send`, which lets the OS pipe buffer
/// fill, which slows the plugin's writes. Without this, a tight emit loop on
/// the plugin side grows the host heap without bound.
///
/// 256 entries is large enough that the main loop's microsecond-scale drain
/// keeps it empty in the common case, but small enough that the worst-case
/// retained memory is bounded by `256 * sizeof(RunEvent)`.
const EVENT_CHANNEL_CAPACITY: usize = 256;

/// Same rationale as `EVENT_CHANNEL_CAPACITY`, applied to stderr lines.
const STDERR_CHANNEL_CAPACITY: usize = 256;

/// Hard ceiling on the number of bytes we'll accept on a single line of
/// stdout (one event envelope) or stderr from a plugin. `BufRead::lines()`
/// has no length limit, so a malicious plugin emitting `"a" * 4 GiB \n`
/// would allocate 4 GiB in the reader thread before yielding the line.
/// 4 MiB is generous for any plausible envelope or log line.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// One parsed `ocp-json/1` envelope emitted by a running plugin.
///
/// `RunEvent` is a thin wrapper around [`ocp_types_v1::Envelope`] that adds
/// host-side convenience accessors. It serializes transparently as the
/// underlying envelope so that callers can forward it to other consumers
/// (e.g., the Tauri frontend) without re-shaping.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct RunEvent(pub Envelope);

impl RunEvent {
    /// The envelope's full kind string (e.g., `"event.run.finished"`).
    pub fn kind(&self) -> &str {
        self.0.kind.as_str()
    }

    /// The last segment of the kind, e.g., `"finished"` for `"event.run.finished"`.
    /// Used by host code that wants the legacy `event` name without parsing the
    /// full kind grammar.
    pub fn event_name(&self) -> &str {
        let kind = self.0.kind.as_str();
        kind.rsplit_once('.').map(|(_, last)| last).unwrap_or(kind)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunExecutionSummary {
    pub exit_code: i32,
    pub event_count: usize,
    pub stderr_lines: Vec<String>,
}

#[derive(Debug, Error)]
pub enum RunError {
    #[error("failed to start tool process: {0}")]
    Spawn(String),
    #[error("failed to read runtime event stream: {0}")]
    EventStream(String),
    #[error("tool process timed out after {0} seconds")]
    Timeout(u64),
}

pub fn run_tool_task<F, G>(
    executable_path: &Path,
    task_file_path: &Path,
    task_id: &str,
    mut on_event: F,
    mut on_stderr: G,
    timeout: Option<std::time::Duration>,
    working_dir: Option<&Path>,
) -> Result<RunExecutionSummary, RunError>
where
    F: FnMut(RunEvent) + Send + 'static,
    G: FnMut(String) + Send + 'static,
{
    let mut cmd = Command::new(executable_path);
    // Pass the task file path as an `OsStr` rather than going through `Display`
    // (which substitutes � for non-UTF-8 path components and would silently
    // fail to open the file). `Command::arg` accepts `AsRef<OsStr>`, preserving
    // every byte of the original path.
    cmd.arg("run")
        .arg(task_file_path.as_os_str())
        .arg("--task")
        .arg(task_id)
        .arg("--output-format")
        .arg("protocol")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn()
        .map_err(|err| RunError::Spawn(err.to_string()))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| RunError::Spawn("stdout pipe was not available".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| RunError::Spawn("stderr pipe was not available".to_string()))?;

    // Bounded channels: see EVENT_CHANNEL_CAPACITY / STDERR_CHANNEL_CAPACITY.
    let (event_tx, event_rx) =
        mpsc::sync_channel::<Result<RunEvent, String>>(EVENT_CHANNEL_CAPACITY);
    let (stderr_tx, stderr_rx) = mpsc::sync_channel::<String>(STDERR_CHANNEL_CAPACITY);

    let event_tx_stderr = event_tx.clone();
    let stdout_handle = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            match read_line_bounded(&mut reader, MAX_LINE_BYTES) {
                Ok(BoundedLine::Eof) => break,
                Ok(BoundedLine::Line(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    // A protocol-shaped line is forwarded as-is. Anything else
                    // (formatted progress text from the plugin, raw stdout
                    // passed through from a child tool like cmdstan, etc.) is
                    // wrapped in a synthetic event.log.line so consumers see
                    // it in the same event stream as structured envelopes.
                    let event = match serde_json::from_str::<Envelope>(&line) {
                        Ok(env) => RunEvent(env),
                        Err(_) => RunEvent(synthesize_log_event(&line, Severity::Info)),
                    };
                    if event_tx.send(Ok(event)).is_err() {
                        break;
                    }
                }
                Ok(BoundedLine::TooLong) => {
                    let _ = event_tx.send(Err(format!(
                        "runtime event line exceeded {MAX_LINE_BYTES}-byte limit"
                    )));
                    break;
                }
                Err(err) => {
                    let _ = event_tx.send(Err(err.to_string()));
                    break;
                }
            }
        }
    });

    let stderr_handle = thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        loop {
            match read_line_bounded(&mut reader, MAX_LINE_BYTES) {
                Ok(BoundedLine::Eof) => break,
                Ok(BoundedLine::Line(line)) => {
                    // Keep the on_stderr callback fed for backward compat,
                    // and surface stderr in the event stream as a warning-
                    // severity log line so consumers render it inline.
                    let event = RunEvent(synthesize_log_event(&line, Severity::Warning));
                    let _ = event_tx_stderr.send(Ok(event));
                    if stderr_tx.send(line).is_err() {
                        break;
                    }
                }
                Ok(BoundedLine::TooLong) => {
                    let _ = stderr_tx.send(format!(
                        "[ocp-host: stderr line exceeded {MAX_LINE_BYTES}-byte limit; truncated]"
                    ));
                    // Keep reading: a truncated line is recoverable for stderr
                    // (unlike for the event stream where the next byte may be
                    // mid-envelope and unparseable). The bounded reader has
                    // already consumed up to the next newline.
                }
                Err(err) => {
                    let _ = stderr_tx.send(format!("stderr read error: {err}"));
                    break;
                }
            }
        }
    });

    let mut event_count = 0usize;
    let mut stderr_lines = Vec::new();
    let deadline = timeout.map(|d| std::time::Instant::now() + d);

    // Hard ceiling on the number of stderr lines we retain in memory for the
    // returned summary. Without this, a plugin that spams stderr (e.g., a
    // tight loop printing diagnostics) can grow `stderr_lines` without bound
    // even though the per-line callback is invoked normally. The user-supplied
    // callback still observes every line — only the in-memory retention is
    // capped, so consumers can implement their own bounded persistence.
    const MAX_RETAINED_STDERR_LINES: usize = 1000;

    // Push to `stderr_lines` only while under the cap. The line just before
    // the cap is replaced with a sentinel so consumers of the summary can see
    // that truncation happened. The on_stderr callback fires for every line.
    macro_rules! handle_stderr_line {
        ($line:expr) => {{
            let line = $line;
            if stderr_lines.len() < MAX_RETAINED_STDERR_LINES - 1 {
                stderr_lines.push(line.clone());
            } else if stderr_lines.len() == MAX_RETAINED_STDERR_LINES - 1 {
                stderr_lines.push(format!(
                    "[ocp-host: stderr buffer capped at {} lines; further lines delivered via callback only]",
                    MAX_RETAINED_STDERR_LINES
                ));
            }
            // Once at the cap, drop the line from the buffer (callback still fires).
            on_stderr(line);
        }};
    }

    // Helper that aborts the child and joins reader threads when the wall-clock
    // deadline is exceeded. Must be checked on every loop iteration — including
    // the hot path where events arrive faster than `recv_timeout`'s 50 ms tick —
    // otherwise a chatty plugin can run indefinitely.
    macro_rules! check_deadline {
        () => {
            if let Some(dl) = deadline {
                if std::time::Instant::now() >= dl {
                    child.kill().ok();
                    child.wait().ok();
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(RunError::Timeout(timeout.unwrap().as_secs()));
                }
            }
        };
    }

    loop {
        check_deadline!();

        match event_rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(Ok(event)) => {
                event_count += 1;
                on_event(event);
            }
            Ok(Err(err)) => return Err(RunError::EventStream(err)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|err| RunError::EventStream(err.to_string()))?
                {
                    while let Ok(line) = stderr_rx.try_recv() {
                        handle_stderr_line!(line);
                    }
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Ok(RunExecutionSummary {
                        exit_code: status.code().unwrap_or(-1),
                        event_count,
                        stderr_lines,
                    });
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        while let Ok(line) = stderr_rx.try_recv() {
            handle_stderr_line!(line);
        }
    }

    while let Ok(line) = stderr_rx.try_recv() {
        handle_stderr_line!(line);
    }

    let status = child
        .wait()
        .map_err(|err| RunError::EventStream(err.to_string()))?;

    let _ = stdout_handle.join();
    let _ = stderr_handle.join();

    Ok(RunExecutionSummary {
        exit_code: status.code().unwrap_or(-1),
        event_count,
        stderr_lines,
    })
}

/// Result of a single bounded line read.
enum BoundedLine {
    /// A complete line was read (newline stripped). May be empty.
    Line(String),
    /// The line exceeded `max_bytes` before reaching a newline. The reader
    /// has been advanced past the next newline (or to EOF) so further reads
    /// resume on the next line.
    TooLong,
    /// The reader hit EOF without producing any bytes for this call.
    Eof,
}

/// Build a synthetic `event.log.line` envelope from a raw text line that did
/// not parse as a protocol envelope. This is what lets the host transparently
/// pass through formatted progress strings (from the plugin or any sub-tool
/// like cmdstan) without requiring every plugin to translate its output into
/// structured payloads.
fn synthesize_log_event(line: &str, severity: Severity) -> Envelope {
    let kind = Kind::parse("event.log.line").expect("static kind parses");
    let mut env = Envelope::new(
        EnvelopeClass::Event,
        Identifier::ulid(Ulid::new().to_string()),
        Timestamp::new(Utc::now().format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()),
        kind,
    );
    let payload = LogLinePayload {
        severity,
        message: line.to_string(),
        logger: None,
        fields: BTreeMap::new(),
        ts: None,
        other: BTreeMap::new(),
    };
    env.payload = Some(serde_json::to_value(&payload).expect("payload serializes"));
    env
}

/// Read one line from `reader`, capping at `max_bytes`. Unlike `BufRead::lines`,
/// this never allocates more than `max_bytes` even against an adversarial input
/// that emits a single multi-gigabyte "line". On overflow, the helper drains
/// bytes until the next newline (or EOF) so the caller can keep reading.
fn read_line_bounded<R: BufRead>(reader: &mut R, max_bytes: usize) -> std::io::Result<BoundedLine> {
    let mut buf = String::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(if buf.is_empty() {
                BoundedLine::Eof
            } else {
                strip_trailing_cr(&mut buf);
                BoundedLine::Line(buf)
            });
        }

        // Find a newline in the buffered slice.
        if let Some(newline_pos) = available.iter().position(|&b| b == b'\n') {
            let take = newline_pos + 1; // include the newline so we consume it
            // If appending the new chunk would exceed the cap, this is overflow.
            if buf.len() + newline_pos > max_bytes {
                // Drain past the newline and return TooLong.
                reader.consume(take);
                return Ok(BoundedLine::TooLong);
            }
            // Lossily decode just the bytes before the newline. Plugin output
            // should be UTF-8; if it isn't, we substitute � rather than crash.
            let chunk = String::from_utf8_lossy(&available[..newline_pos]);
            buf.push_str(&chunk);
            reader.consume(take);
            strip_trailing_cr(&mut buf);
            return Ok(BoundedLine::Line(buf));
        }

        // No newline in the current buffer. If appending the whole buffer
        // would exceed the cap, declare overflow and drain to the next newline.
        if buf.len() + available.len() > max_bytes {
            // Consume everything in this fill, then keep draining until we
            // find a newline (or EOF) so subsequent reads start clean.
            let consume_now = available.len();
            reader.consume(consume_now);
            drain_to_next_newline(reader)?;
            return Ok(BoundedLine::TooLong);
        }

        // Otherwise append the whole buffer and loop for more.
        let chunk = String::from_utf8_lossy(available);
        buf.push_str(&chunk);
        let consume_now = available.len();
        reader.consume(consume_now);
    }
}

/// Drop bytes from `reader` until just past the next newline (or EOF).
/// Used to recover from an over-long line.
fn drain_to_next_newline<R: BufRead>(reader: &mut R) -> std::io::Result<()> {
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let take = pos + 1;
            reader.consume(take);
            return Ok(());
        }
        let n = available.len();
        reader.consume(n);
    }
}

/// `BufRead::lines` strips trailing `\r` for CRLF line endings; mirror that
/// behaviour so plugins running on Windows produce the same lines as POSIX.
fn strip_trailing_cr(s: &mut String) {
    if s.ends_with('\r') {
        s.pop();
    }
}

#[cfg(test)]
mod read_line_bounded_tests {
    use super::*;
    use std::io::Cursor;

    fn read_all_lines(input: &[u8], cap: usize) -> Vec<BoundedLine> {
        let mut reader = std::io::BufReader::new(Cursor::new(input));
        let mut out = Vec::new();
        loop {
            let line = read_line_bounded(&mut reader, cap).unwrap();
            let is_eof = matches!(line, BoundedLine::Eof);
            out.push(line);
            if is_eof {
                return out;
            }
        }
    }

    #[test]
    fn reads_two_lf_lines() {
        let lines = read_all_lines(b"alpha\nbeta\n", 1024);
        assert!(matches!(&lines[0], BoundedLine::Line(s) if s == "alpha"));
        assert!(matches!(&lines[1], BoundedLine::Line(s) if s == "beta"));
        assert!(matches!(&lines[2], BoundedLine::Eof));
    }

    #[test]
    fn reads_crlf_lines_stripping_cr() {
        let lines = read_all_lines(b"alpha\r\nbeta\r\n", 1024);
        assert!(matches!(&lines[0], BoundedLine::Line(s) if s == "alpha"));
        assert!(matches!(&lines[1], BoundedLine::Line(s) if s == "beta"));
    }

    #[test]
    fn final_line_without_newline_is_returned() {
        let lines = read_all_lines(b"only-line", 1024);
        assert!(matches!(&lines[0], BoundedLine::Line(s) if s == "only-line"));
        assert!(matches!(&lines[1], BoundedLine::Eof));
    }

    #[test]
    fn empty_input_yields_eof_immediately() {
        let lines = read_all_lines(b"", 1024);
        assert_eq!(lines.len(), 1);
        assert!(matches!(&lines[0], BoundedLine::Eof));
    }

    #[test]
    fn empty_line_is_returned_as_empty_string() {
        let lines = read_all_lines(b"\nnext\n", 1024);
        assert!(matches!(&lines[0], BoundedLine::Line(s) if s.is_empty()));
        assert!(matches!(&lines[1], BoundedLine::Line(s) if s == "next"));
    }

    #[test]
    fn over_long_line_returns_too_long_then_recovers() {
        // 50 'a's then a newline, then a normal next line. Cap = 10.
        let mut buf = vec![b'a'; 50];
        buf.push(b'\n');
        buf.extend_from_slice(b"recovered\n");
        let lines = read_all_lines(&buf, 10);
        assert!(matches!(&lines[0], BoundedLine::TooLong));
        assert!(matches!(&lines[1], BoundedLine::Line(s) if s == "recovered"));
        assert!(matches!(&lines[2], BoundedLine::Eof));
    }

    #[test]
    fn over_long_unterminated_line_returns_too_long_then_eof() {
        // No trailing newline at all; reader hits EOF mid-overflow.
        let buf = vec![b'b'; 5000];
        let lines = read_all_lines(&buf, 100);
        assert!(matches!(&lines[0], BoundedLine::TooLong));
        assert!(matches!(&lines[1], BoundedLine::Eof));
    }

    #[test]
    fn line_exactly_at_cap_succeeds() {
        // 10 bytes of payload + newline. cap = 10 means newline_pos == 10
        // and `buf.len() + newline_pos == 0 + 10 == 10`, which is NOT > cap,
        // so this should succeed.
        let lines = read_all_lines(b"0123456789\n", 10);
        assert!(matches!(&lines[0], BoundedLine::Line(s) if s == "0123456789"));
    }

    #[test]
    fn line_one_past_cap_returns_too_long() {
        let lines = read_all_lines(b"01234567890\n", 10);
        assert!(matches!(&lines[0], BoundedLine::TooLong));
    }

    #[test]
    fn invalid_utf8_substituted_with_replacement_char() {
        // 0xFF is not valid UTF-8 in any position. We should get the
        // replacement char rather than panicking.
        let lines = read_all_lines(b"a\xFFb\n", 1024);
        if let BoundedLine::Line(s) = &lines[0] {
            assert!(s.contains('a'));
            assert!(s.contains('b'));
            assert!(s.chars().any(|c| c == '\u{FFFD}'));
        } else {
            panic!("expected Line, got {:?}", lines[0]);
        }
    }
}

#[cfg(test)]
impl std::fmt::Debug for BoundedLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoundedLine::Line(s) => write!(f, "Line({s:?})"),
            BoundedLine::TooLong => write!(f, "TooLong"),
            BoundedLine::Eof => write!(f, "Eof"),
        }
    }
}
