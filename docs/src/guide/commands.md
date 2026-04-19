# Commands

Every plugin binary must respond to three CLI invocations. This chapter shows exactly what each one must do and how to implement it using `ocp-types-v1`.

## Overview

| Invocation | Purpose | Output |
|-----------|---------|--------|
| `<binary> api validate --command <name> --input-json <json>` | Validate parameters before a run | One `response.validate` envelope |
| `<binary> api self-test` | Health check — is the plugin functional? | One `response.self_test` envelope |
| `<binary> run <task-file.json> --task <id> [--output-format protocol]` | Execute a task | NDJSON stream of `event.*` envelopes |

All three write to stdout. Stderr is for diagnostic messages only and is not parsed by the host.

Every line of stdout — for every invocation — is an `ocp-json/1` [envelope](../protocol/envelope-1.md). The envelope shape is frozen; the `payload` field is what changes between kinds.

---

## A helper module

All three invocations build envelopes, so it pays to factor the wiring into a small helper module. The rest of the guide assumes you have something like this in `src/wire_io.rs`:

```rust
use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use chrono::Utc;
use ocp_types_v1::{
    envelope::{Envelope, EnvelopeClass, RunContext},
    kind::Kind,
    wire::{Identifier, Timestamp, ToolRef},
};
use serde::Serialize;
use ulid::Ulid;

pub const FAMILY: &str = "my-echo-plugin";
pub const TOOL:   &str = "my-echo-plugin";

pub fn tool_ref() -> ToolRef {
    ToolRef::new(FAMILY, TOOL, env!("CARGO_PKG_VERSION"))
}

pub fn new_ulid() -> Identifier {
    Identifier::ulid(Ulid::new().to_string())
}

pub fn now_ts() -> Timestamp {
    Timestamp::new(Utc::now().format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string())
}

pub fn run_context(run_id: &Identifier, task_id: &str) -> RunContext {
    RunContext {
        run_id: run_id.clone(),
        task_id: Some(task_id.to_string()),
        parent_run_id: None,
        run_chain: Vec::new(),
        stage_id: None,
        originating_tool: None,
        tool: tool_ref(),
        run_metadata: BTreeMap::new(),
        other: BTreeMap::new(),
    }
}

pub fn event_envelope<P: Serialize>(kind: &str, run: RunContext, payload: &P) -> Result<Envelope> {
    build(EnvelopeClass::Event, kind, Some(run), Some(payload))
}

pub fn response_envelope<P: Serialize>(kind: &str, payload: &P) -> Result<Envelope> {
    build(EnvelopeClass::Response, kind, None, Some(payload))
}

fn build<P: Serialize>(
    class: EnvelopeClass,
    kind: &str,
    run: Option<RunContext>,
    payload: Option<&P>,
) -> Result<Envelope> {
    let parsed = Kind::parse(kind).map_err(|e| anyhow::anyhow!("invalid kind '{kind}': {e}"))?;
    let mut env = Envelope::new(class, new_ulid(), now_ts(), parsed);
    env.run = run;
    if let Some(p) = payload {
        env.payload = Some(serde_json::to_value(p)?);
    }
    Ok(env)
}

pub fn write_envelope<W: Write + ?Sized>(w: &mut W, env: &Envelope) -> Result<()> {
    serde_json::to_writer(&mut *w, env)?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
}
```

This module is the only place in the plugin that touches `ocp_types_v1` wire types directly. Everything else builds a typed payload and hands it to `event_envelope` or `response_envelope`.

---

## `api validate`

The host calls this before displaying a run dialog or executing a task. It should be fast — no disk I/O, no network.

**Input** (via `--input-json`): a JSON object containing the proposed parameter values.

**Output**: exactly one `response.validate` envelope. The payload is a [`ValidateResponsePayload`](../protocol/kinds-1.md).

### Parsing arguments

```rust
fn handle_validate(args: &[String], out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let command = flag_value(args, "--command")
        .ok_or_else(|| anyhow::anyhow!("api validate requires --command <name>"))?;
    let input_json = flag_value(args, "--input-json")
        .ok_or_else(|| anyhow::anyhow!("api validate requires --input-json <json>"))?;
    let input: serde_json::Value = serde_json::from_str(input_json)?;

    let payload = validate(command, &input);
    let env = wire_io::response_envelope("response.validate", &payload)?;
    wire_io::write_envelope(out, &env)
}
```

### Building a `ValidateResponsePayload`

```rust
use std::collections::BTreeMap;
use ocp_types_v1::{
    common::{Severity, ValidationIssue},
    responses::ValidateResponsePayload,
};

fn validate(command: &str, input: &serde_json::Value) -> ValidateResponsePayload {
    let mut issues: Vec<ValidationIssue> = Vec::new();

    if command != "echo" {
        issues.push(issue(Severity::Error, "/", "unsupported_command",
            format!("command `{command}` is not supported")));
    }

    // Validate required field: message
    match input.get("message").and_then(|v| v.as_str()) {
        None | Some("") => issues.push(issue(
            Severity::Error, "/message", "required",
            "message is required and must be a non-empty string",
        )),
        _ => {}
    }

    // Validate required field: output_dir
    if input.get("output_dir").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
        issues.push(issue(
            Severity::Error, "/output_dir", "required",
            "output_dir is required",
        ));
    }

    let ok = !issues.iter().any(|i| i.severity == Severity::Error);
    ValidateResponsePayload {
        ok,
        issues,
        cost_estimate: None,
        normalized_params: if ok { Some(input.clone()) } else { None },
        other: BTreeMap::new(),
    }
}

fn issue(severity: Severity, path: &str, code: &str, message: impl Into<String>) -> ValidationIssue {
    ValidationIssue {
        severity,
        message: message.into(),
        path: Some(path.to_string()),
        code: Some(code.to_string()),
        hint: None,
        other: BTreeMap::new(),
    }
}
```

`ValidationIssue` unifies what older drafts called `ValidationError` and `ValidationWarning` into a single type discriminated by `severity`. A `severity: Warning` issue does not fail the response — only `severity: Error` does.

Fields:

- `severity` — `Info` / `Warning` / `Error`
- `message` — human-readable description
- `path` — JSON Pointer (RFC 6901) into the input document, e.g. `"/operands/0"`; use `"/"` for whole-input issues
- `code` — machine-readable code; vendors SHOULD namespace (`<vendor>.<code>`)
- `hint` — optional fix suggestion shown in the UI

---

## `api self-test`

The host calls this when the user opens the plugin detail page or runs a health check. Run a minimal end-to-end smoke test — enough to verify the plugin is functional.

**Output**: exactly one `response.self_test` envelope. The payload is a [`SelfTestResponsePayload`](../protocol/kinds-1.md).

```rust
use std::collections::BTreeMap;
use ocp_types_v1::responses::{SelfTestCheck, SelfTestResponsePayload, SelfTestStatus};

fn handle_self_test(out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let payload = run_self_test();
    let env = wire_io::response_envelope("response.self_test", &payload)?;
    wire_io::write_envelope(out, &env)
}

fn run_self_test() -> SelfTestResponsePayload {
    let mut checks: Vec<SelfTestCheck> = Vec::new();

    // Check 1: validate accepts a valid input
    let valid_input = serde_json::json!({
        "message": "hello",
        "output_dir": "./test-out",
    });
    let outcome = validate("echo", &valid_input);
    checks.push(SelfTestCheck {
        id: "validate_valid_input".into(),
        label: "validate accepts a well-formed input".into(),
        status: if outcome.ok { SelfTestStatus::Pass } else { SelfTestStatus::Fail },
        message: if outcome.ok { None } else { Some("unexpected issues".into()) },
        elapsed: None,
        other: BTreeMap::new(),
    });

    // Check 2: validate rejects a missing message
    let invalid_input = serde_json::json!({ "output_dir": "./test-out" });
    let outcome = validate("echo", &invalid_input);
    let rejected = !outcome.ok
        && outcome.issues.iter().any(|i| i.path.as_deref() == Some("/message"));
    checks.push(SelfTestCheck {
        id: "validate_rejects_missing_message".into(),
        label: "validate rejects missing message".into(),
        status: if rejected { SelfTestStatus::Pass } else { SelfTestStatus::Fail },
        message: if rejected { None } else { Some("expected rejection".into()) },
        elapsed: None,
        other: BTreeMap::new(),
    });

    let ok = checks.iter().all(|c| c.status == SelfTestStatus::Pass);
    SelfTestResponsePayload {
        ok,
        checks,
        elapsed: None,
        summary: None,
        other: BTreeMap::new(),
    }
}
```

Each `SelfTestCheck`:

- `id` — stable identifier shown in the UI
- `label` — human-readable name
- `status` — `Pass` / `Fail` / `Skipped`
- `message` — explanation; shown when `status` is `Fail`
- `elapsed` — optional duration for this check

---

## The run subcommand

This is the main execution path. The host writes a JSON task file to disk and invokes:

```
<binary> run <path/to/task.json> --task <task-id> --output-format protocol
```

Your binary reads the task file, finds the task by ID, executes it, and emits a stream of `event.*` envelopes as NDJSON on stdout.

See the [Events](events.md) chapter for the full event kind reference.

### Task file format

```json
{
  "format": "oc-task-file/1",
  "config": { "stop_on_error": false },
  "tasks": [
    {
      "id": "echo_echo",
      "command": "echo",
      "params": {
        "message": "Hello from Open Choice!",
        "output_dir": "./outputs/echo"
      }
    }
  ]
}
```

### Reading the task file

```rust
use serde_json::Value;

fn handle_run(task_file_path: &str, task_id: &str, out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let raw = std::fs::read_to_string(task_file_path)?;
    let doc: Value = serde_json::from_str(&raw)?;

    let task = doc["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|t| t["id"].as_str() == Some(task_id)))
        .ok_or_else(|| anyhow::anyhow!("task '{task_id}' not found"))?;

    let command = task["command"].as_str().unwrap_or("");
    let params = &task["params"];

    run_task(task_id, command, params, out)
}
```

### Emitting events

A minimal run emits `event.run.started`, optional progress/artifact events, then `event.run.finished` (or `event.run.failed`).

```rust
use std::collections::BTreeMap;
use ocp_types_v1::{
    common::ArtifactRecord,
    events::{ArtifactCreatedPayload, RunFinishedPayload, RunStartedPayload},
    wire::{Duration, PathRef},
};
use ulid::Ulid;

fn run_task(task_id: &str, _command: &str, params: &serde_json::Value,
            out: &mut dyn std::io::Write) -> anyhow::Result<()> {
    let run_id = wire_io::new_ulid();
    let message = params["message"].as_str().unwrap_or("");
    let output_dir = params["output_dir"].as_str().unwrap_or(".");

    // event.run.started
    let payload = RunStartedPayload {
        seed: None,
        output_dir: Some(PathRef::Local { path: output_dir.into() }),
        argv: std::env::args().collect(),
        label: None,
        other: BTreeMap::new(),
    };
    let env = wire_io::event_envelope(
        "event.run.started",
        wire_io::run_context(&run_id, task_id),
        &payload,
    )?;
    wire_io::write_envelope(out, &env)?;

    // Do the actual work
    std::fs::create_dir_all(output_dir)?;
    let out_path = format!("{output_dir}/result.txt");
    std::fs::write(&out_path, message)?;

    // event.artifact.created
    let artifact = ArtifactRecord {
        artifact_id: Ulid::new().to_string(),
        path: PathRef::Local { path: out_path },
        kind: "result.txt".into(),
        media_type: Some("text/plain".into()),
        digest: None,
        size_bytes: Some(message.len() as u64),
        created_at: None,
        modified_at: None,
        label: Some("Echoed message".into()),
        description: None,
        ext: BTreeMap::new(),
        other: BTreeMap::new(),
    };
    let env = wire_io::event_envelope(
        "event.artifact.created",
        wire_io::run_context(&run_id, task_id),
        &ArtifactCreatedPayload { artifact: artifact.clone(), other: BTreeMap::new() },
    )?;
    wire_io::write_envelope(out, &env)?;

    // event.run.finished
    let payload = RunFinishedPayload {
        elapsed: Some(Duration::from_secs(0)),
        artifacts: vec![artifact],
        summary: Some(format!("echoed: {message}")),
        metrics: Vec::new(),
        other: BTreeMap::new(),
    };
    let env = wire_io::event_envelope(
        "event.run.finished",
        wire_io::run_context(&run_id, task_id),
        &payload,
    )?;
    wire_io::write_envelope(out, &env)?;

    Ok(())
}
```

### Handling errors

If execution fails, emit `event.run.failed` as the terminal envelope — **not** a `finished` with a failure flag. `run.finished` and `run.failed` are mutually exclusive.

```rust
use ocp_types_v1::events::RunFailedPayload;

let payload = RunFailedPayload {
    error: "could not write output file".into(),
    error_code: Some("echo.write_failed".into()),
    cause_chain: vec![err.to_string()],
    elapsed: Some(Duration::from_secs(0)),
    partial_artifacts: Vec::new(),
    other: BTreeMap::new(),
};
let env = wire_io::event_envelope(
    "event.run.failed",
    wire_io::run_context(&run_id, task_id),
    &payload,
)?;
wire_io::write_envelope(out, &env)?;
```

The host treats exactly one of `event.run.finished`, `event.run.failed`, or `event.run.cancelled` as the terminal envelope and marks the run complete.

---

## Parsing `--flag value` pairs

Nothing fancy is needed:

```rust
fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}
```
