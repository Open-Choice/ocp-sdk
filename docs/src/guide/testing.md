# Testing

## Testing the CLI directly

The fastest feedback loop is running your binary directly from the command line.

### Validate

```bash
cargo build

./target/debug/my-echo-plugin api validate \
  --command echo \
  --input-json '{"message":"hello","output_dir":"./out"}'
```

Expected output: a single `ocp-json/1` envelope on stdout with `kind: "response.validate"`:

```json
{"ocp":"1","class":"response","id":{"fmt":"ulid","value":"01HQRZ..."},"ts":"2026-04-07T12:34:56.123456Z","kind":"response.validate","payload":{"ok":true,"issues":[],"normalized_params":null}}
```

Test a bad input:

```bash
./target/debug/my-echo-plugin api validate \
  --command echo \
  --input-json '{}'
```

Expected: the response envelope's `payload.ok` is `false` and `payload.issues` lists a `ValidationIssue` for each missing or malformed field, each with a `severity`, `message`, and optional `path` / `code` / `hint`.

### Self-test

```bash
./target/debug/my-echo-plugin api self-test
```

Expected: a single `response.self_test` envelope whose `payload.status` is `"ok"` and whose `payload.checks` array lists each self-check with its own `status`. A failing overall status means at least one check returned `"fail"` — its `message` field explains why.

### Run

Write a minimal task file (`test-task.json`):

```json
{
  "format": "oc-task-file/1",
  "config": { "stop_on_error": false },
  "tasks": [
    {
      "id": "echo_echo",
      "command": "echo",
      "params": {
        "message": "Hello from test",
        "output_dir": "./test-outputs"
      }
    }
  ]
}
```

Run it:

```bash
./target/debug/my-echo-plugin test-task.json \
  --task echo_echo \
  --output-format protocol
```

Expected: an NDJSON stream of `event.*` envelopes, starting with `event.run.started` and ending with `event.run.finished`. See [Events](events.md) for the full stream contract.

---

## Unit testing validation logic

Extract your validation function into a module that returns typed `ValidationIssue`s and unit-test it directly:

```rust
use ocp_types_v1::common::{Severity, ValidationIssue};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn valid_input_passes() {
        let input = json!({ "message": "hi", "output_dir": "./out" });
        let issues = validate("echo", &input);
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
    }

    #[test]
    fn missing_message_fails() {
        let input = json!({ "output_dir": "./out" });
        let issues = validate("echo", &input);
        assert!(issues.iter().any(|i| {
            i.severity == Severity::Error
                && i.path.as_deref() == Some("/message")
                && i.code.as_deref() == Some("missing_field")
        }));
    }

    #[test]
    fn unknown_command_fails() {
        let issues = validate("nonexistent", &json!({}));
        assert!(issues.iter().any(|i| i.code.as_deref() == Some("unknown_command")));
    }
}
```

Run with:

```bash
cargo test
```

Validation issue paths are JSON Pointers (RFC 6901), so `"/message"` — not `"$.message"`. See `ocp_types_v1::common::ValidationIssue` for the full field list.

---

## Integration testing with the host

For a full end-to-end test, build a release package and install it in Open Choice with developer mode on:

1. **Enable developer mode** — Settings → Developer Mode (allows unsigned or self-signed plugins).
2. `cargo build --release`.
3. `oc-sign pack packaging/manifest.json target/release/my-echo-plugin.exe --key-file dev.key --out test.ocplugin`.
4. **Plugins → Install from file** → select `test.ocplugin`.
5. Open the Help panel and confirm your examples appear.
6. Create an `.oce` file using one of your snippets and run it.

The `plugin-toy-calculator` plugin is the canonical integration test target used by the Open Choice team. Its `static/examples/calculate.json` and self-test cases are the reference for what complete coverage looks like.

---

## Testing the event stream

The `ocp_types_v1::Envelope` type round-trips every envelope losslessly, so the sharpest test is a Rust integration test that invokes the binary as a subprocess, reads stdout line by line, and deserializes each line into an `Envelope`:

```rust
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use ocp_types_v1::{Envelope, EnvelopeClass};

#[test]
fn run_emits_started_and_finished() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_my-echo-plugin"))
        .args(["test-task.json", "--task", "echo_echo", "--output-format", "protocol"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let mut kinds = Vec::new();
    let reader = BufReader::new(child.stdout.take().unwrap());
    for line in reader.lines() {
        let env: Envelope = serde_json::from_str(&line.unwrap()).unwrap();
        assert!(env.is_ocp_v1());
        assert_eq!(env.class, EnvelopeClass::Event);
        kinds.push(env.kind.as_str().to_string());
    }

    assert!(child.wait().unwrap().success());
    assert_eq!(kinds.first().map(String::as_str), Some("event.run.started"));
    let terminal = kinds.last().map(String::as_str);
    assert!(matches!(terminal, Some("event.run.finished" | "event.run.failed" | "event.run.cancelled")));
}
```

For quick shell-level spot checks, pipe the NDJSON through `jq`:

```bash
./target/debug/my-echo-plugin test-task.json --task echo_echo --output-format protocol \
  | jq -r '.kind'
```

This prints one kind per line — a quick sanity check that `event.run.started` comes first and exactly one of `event.run.finished` / `event.run.failed` / `event.run.cancelled` comes last.
