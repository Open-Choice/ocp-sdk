# Quickstart: your first plugin

This guide walks from an empty directory to a working `ocp-json/1` plugin the Open Choice host can install and run. It takes about 15 minutes.

You will build a plugin that echoes a message — simple enough that nothing gets in the way of understanding the structure. Every real plugin in the ecosystem (the toy calculator, the HB model, the R wrapper) follows the same layout.

## Prerequisites

- Rust toolchain (`rustup`, `cargo`) — stable channel
- `oc-sign` CLI: `cargo install --path crates/oc-sign` from the ocp-sdk root

## 1. Create the crate

```bash
cargo new --bin my-echo-plugin
cd my-echo-plugin
```

## 2. Add the dependency

```toml
[package]
name    = "my-echo-plugin"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "my-echo-plugin"

[dependencies]
ocp-types-v1 = "1"
anyhow       = "1"
serde        = { version = "1", features = ["derive"] }
serde_json   = "1"
chrono       = { version = "0.4", features = ["serde", "clock"] }
ulid         = "1"
```

All plugins depend on `ocp-types-v1` — it's the frozen wire-format crate for `ocp-json/1`. Everything you emit on stdout goes through this crate's `Envelope` type.

## 3. Add the `wire_io.rs` helper module

Every plugin funnels envelope construction through a small local helper that wraps `ocp_types_v1`. This keeps the rest of your code focused on building typed payloads. Copy this as `src/wire_io.rs`:

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

pub fn write_envelope<W: Write + ?Sized>(w: &mut W, env: &Envelope) -> Result<()> {
    serde_json::to_writer(&mut *w, env)?;
    w.write_all(b"\n")?;
    w.flush()?;
    Ok(())
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
```

From here on, the rest of the plugin talks to `event_envelope` / `response_envelope` and never touches the envelope shape directly. Future protocol bumps land in this file and nowhere else.

## 4. Write `src/main.rs`

A plugin binary serves three jobs dispatched by its first argument: `api validate`, `api self-test`, and a task-file invocation for the run subcommand.

```rust
mod wire_io;

use std::collections::BTreeMap;
use std::io::{self, Write};

use anyhow::Result;
use ocp_types_v1::{
    common::Severity,
    events::{RunFinishedPayload, RunStartedPayload},
    responses::{
        SelfTestCheck, SelfTestResponsePayload, SelfTestStatus, ValidateResponsePayload,
    },
    wire::{Duration, PathRef},
};

use wire_io::{event_envelope, new_ulid, response_envelope, run_context, write_envelope};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mut stdout = io::stdout().lock();

    match args.get(1).map(String::as_str) {
        Some("api") => match args.get(2).map(String::as_str) {
            Some("validate")  => cmd_validate(&args, &mut stdout),
            Some("self-test") => cmd_self_test(&mut stdout),
            other => {
                eprintln!("unknown api subcommand: {other:?}");
                std::process::exit(2);
            }
        },
        Some(task_file)       => cmd_run(task_file, &args, &mut stdout),
        None => {
            eprintln!("usage: my-echo-plugin <api validate|api self-test|task-file>");
            std::process::exit(2);
        }
    }
}

fn cmd_validate(args: &[String], stdout: &mut dyn Write) -> Result<()> {
    // Parse --command <name> --input-json <literal-json> from args.
    let command = flag(args, "--command").unwrap_or_default();
    let input   = flag(args, "--input-json").unwrap_or("{}".into());
    let value: serde_json::Value = serde_json::from_str(&input)?;

    let mut issues = Vec::new();
    if command != "echo" {
        issues.push(ocp_types_v1::common::ValidationIssue {
            severity: Severity::Error,
            message: format!("unknown command '{command}'"),
            path: None,
            code: Some("unknown_command".into()),
            hint: Some("supported commands: echo".into()),
            other: Default::default(),
        });
    }
    if value.get("message").and_then(|v| v.as_str()).unwrap_or("").is_empty() {
        issues.push(ocp_types_v1::common::ValidationIssue {
            severity: Severity::Error,
            message: "`message` is required".into(),
            path: Some("/message".into()),
            code: Some("missing_field".into()),
            hint: None,
            other: Default::default(),
        });
    }

    let payload = ValidateResponsePayload {
        ok: issues.is_empty(),
        issues,
        normalized_params: None,
        other: Default::default(),
    };
    write_envelope(stdout, &response_envelope("response.validate", &payload)?)
}

fn cmd_self_test(stdout: &mut dyn Write) -> Result<()> {
    let payload = SelfTestResponsePayload {
        status: SelfTestStatus::Ok,
        checks: vec![SelfTestCheck {
            name: "binary_responds".into(),
            status: SelfTestStatus::Ok,
            message: Some("binary loaded and answered api self-test".into()),
            duration: None,
            other: Default::default(),
        }],
        message: None,
        other: Default::default(),
    };
    write_envelope(stdout, &response_envelope("response.self_test", &payload)?)
}

fn cmd_run(task_file: &str, _args: &[String], stdout: &mut dyn Write) -> Result<()> {
    // (Task-file parsing and message extraction trimmed for the quickstart.
    //  The Commands chapter walks through this in full.)
    let task_id = "demo";
    let run_id = new_ulid();

    let started = RunStartedPayload {
        seed: None,
        output_dir: Some(PathRef::Local { path: "./out".into() }),
        argv: std::env::args().collect(),
        label: Some(format!("echo run from {task_file}")),
        other: BTreeMap::new(),
    };
    write_envelope(stdout, &event_envelope("event.run.started", run_context(&run_id, task_id), &started)?)?;

    let finished = RunFinishedPayload {
        elapsed: Some(Duration::from_secs(0)),
        artifacts: Vec::new(),
        summary: Some("Hello, world!".into()),
        metrics: Vec::new(),
        other: BTreeMap::new(),
    };
    write_envelope(stdout, &event_envelope("event.run.finished", run_context(&run_id, task_id), &finished)?)
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}
```

The [Commands](commands.md) chapter expands each handler with realistic validation, normalization, self-test coverage, and a full `run` sequence with artifacts and progress.

## 5. Create the manifest

Create `packaging/manifest.json`:

```json
{
  "schema_version": "1",
  "plugin_id": "com.example.my-echo-plugin",
  "display_name": "My Echo Plugin",
  "version": "0.1.0",
  "publisher": "Your Name",
  "description": "Echoes a message.",
  "runtime": {
    "type": "native-sidecar",
    "entrypoints": [
      {
        "os": "windows",
        "arch": "x86_64",
        "path": "bin/windows-x86_64/my-echo-plugin.exe",
        "digest": { "algorithm": "sha256", "value": "__PLACEHOLDER__" }
      }
    ]
  },
  "protocol": { "family": "ocp-json", "version": "1" },
  "commands": ["echo"],
  "capabilities": ["events.progress"],
  "sandbox": {
    "fs_read":  [],
    "fs_write": ["plugin-workdir"],
    "network":  false
  },
  "signing": {
    "key_id": "my-key-2026",
    "signature_path": "signatures/manifest.sig",
    "algorithm": "ed25519"
  }
}
```

`digest.value` is filled in automatically by `oc-sign pack`. Every other field is detailed in the [Manifest reference](manifest.md).

## 6. Build and smoke-test locally

```bash
cargo build --release

# api validate
./target/release/my-echo-plugin api validate \
  --command echo \
  --input-json '{"message":"hello","output_dir":"./out"}'

# api self-test
./target/release/my-echo-plugin api self-test
```

Each command prints a single `response.*` envelope on stdout. [Commands](commands.md) explains the envelope shape and what the host checks for.

## 7. Generate a signing key

```bash
oc-sign keygen --key-id my-key-2026
```

Prints the public key hex (for adding to `trusted_keys.json` during testing) and writes the private key to `my-key-2026.key`. Keep the `.key` file secret — never commit it.

## 8. Pack the plugin

```bash
oc-sign pack packaging/manifest.json \
  target/release/my-echo-plugin.exe \
  --key-file my-key-2026.key \
  --out my-echo-plugin-0.1.0-windows-x86_64.ocplugin
```

`oc-sign pack`:

1. Computes the SHA-256 of the binary.
2. Patches the placeholder digest on the matching entrypoint with the real hash.
3. Signs the patched manifest with Ed25519.
4. Assembles the `.ocplugin` zip (manifest + binary + signature).

## 9. Install in Open Choice

In the Open Choice app: **Plugins → Install from file** → select the `.ocplugin` file.

The host shows the install consent dialog with your plugin's capabilities and sandbox declarations. After confirming, the plugin appears in the plugin browser.

## Next steps

- [Commands](commands.md) — the inspection API (`api validate`, `api self-test`) and the run subcommand in full
- [Events](events.md) — every `event.*` kind with its payload type
- [Static assets](static-assets.md) — schemas, examples, help, and output catalogs
- [Manifest reference](manifest.md) — every manifest field explained
- [Packaging](packaging.md) — directory layout inside a `.ocplugin` file
