/// Integration tests for `ocp_host::runner::run_tool_task`.
///
/// These tests use the `oc-test-helper` binary (built from
/// `src/bin/oc_test_helper.rs`). Cargo sets `CARGO_BIN_EXE_oc-test-helper`
/// to the path of the compiled binary at test-compile time.
///
/// The `--task` value is repurposed as a behaviour selector so that all tests
/// can run in parallel without env-var races:
///   "ok"       → emits finished event, exits 0
///   "error"    → emits finished event (success=false), exits 1
///   "bad_json" → emits a non-JSON line, exits 0
///   "hang"     → sleeps 60 s (used to test timeout)

use std::sync::{Arc, Mutex};
use ocp_host::runner::{run_tool_task, RunError};

fn helper() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_oc-test-helper"))
}

fn dummy_task_file() -> tempfile::NamedTempFile {
    let f = tempfile::NamedTempFile::new().expect("temp task file");
    std::fs::write(f.path(), b"{}").unwrap();
    f
}

// ── Spawn failure ─────────────────────────────────────────────────────────────

#[test]
fn nonexistent_exe_returns_spawn_error() {
    let task = dummy_task_file();
    let result = run_tool_task(
        std::path::Path::new("/nonexistent/no-such-binary-xyz"),
        task.path(),
        "ok",
        |_| {},
        |_| {},
        None,
        None,
    );
    assert!(
        matches!(result, Err(RunError::Spawn(_))),
        "expected Spawn error, got {result:?}"
    );
}

// ── Successful run ────────────────────────────────────────────────────────────

#[test]
fn successful_run_collects_finished_event() {
    let task = dummy_task_file();
    let events: Arc<Mutex<Vec<_>>> = Arc::new(Mutex::new(Vec::new()));
    let ev = events.clone();

    let summary = run_tool_task(
        &helper(),
        task.path(),
        "ok",
        move |event| ev.lock().unwrap().push(event),
        |_| {},
        None,
        None,
    )
    .expect("run should succeed");

    assert_eq!(summary.exit_code, 0);
    assert_eq!(summary.event_count, 1);

    let collected = events.lock().unwrap();
    assert_eq!(collected.len(), 1);
    assert_eq!(collected[0].event_name(), "finished");
}

// ── Non-zero exit code ────────────────────────────────────────────────────────

#[test]
fn nonzero_exit_code_reported_in_summary() {
    let task = dummy_task_file();
    let summary = run_tool_task(&helper(), task.path(), "error", |_| {}, |_| {}, None, None)
        .expect("run should not hard-error for non-zero exit");
    assert_ne!(summary.exit_code, 0);
}

// ── Non-envelope stdout lines ─────────────────────────────────────────────────

#[test]
fn non_envelope_line_synthesized_as_log_event() {
    let task = dummy_task_file();
    let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let received_clone = std::sync::Arc::clone(&received);
    let summary = run_tool_task(
        &helper(),
        task.path(),
        "bad_json",
        move |event| {
            received_clone.lock().unwrap().push(event.kind().to_string());
        },
        |_| {},
        None,
        None,
    )
    .expect("non-envelope lines must not hard-error the run");
    assert_eq!(summary.exit_code, 0);
    let kinds = received.lock().unwrap();
    assert!(
        kinds.iter().any(|k| k == "event.log.line"),
        "expected at least one synthesized event.log.line, got {kinds:?}"
    );
}

// ── Timeout ───────────────────────────────────────────────────────────────────

#[test]
fn hung_process_killed_after_timeout() {
    let task = dummy_task_file();
    let result = run_tool_task(
        &helper(),
        task.path(),
        "hang",
        |_| {},
        |_| {},
        Some(std::time::Duration::from_millis(300)),
        None,
    );
    assert!(
        matches!(result, Err(RunError::Timeout(_))),
        "expected Timeout error, got {result:?}"
    );
}

#[test]
fn timeout_error_reports_correct_seconds() {
    let task = dummy_task_file();
    let result = run_tool_task(
        &helper(),
        task.path(),
        "hang",
        |_| {},
        |_| {},
        Some(std::time::Duration::from_secs(1)),
        None,
    );
    assert!(
        matches!(result, Err(RunError::Timeout(1))),
        "expected Timeout(1), got {result:?}"
    );
}

// ── No timeout fires for fast process ────────────────────────────────────────

#[test]
fn fast_process_completes_before_generous_timeout() {
    let task = dummy_task_file();
    let result = run_tool_task(
        &helper(),
        task.path(),
        "ok",
        |_| {},
        |_| {},
        Some(std::time::Duration::from_secs(30)),
        None,
    );
    assert!(result.is_ok(), "should complete without timing out");
}
