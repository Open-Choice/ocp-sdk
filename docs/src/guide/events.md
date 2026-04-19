# Events

During a run the plugin writes an NDJSON stream of `event.*` envelopes to stdout. The host reads the stream line-by-line and surfaces progress, artifacts, and errors in the UI as they arrive.

This chapter is the event reference. The envelope shape, the `wire_io.rs` helper module, and the inspection `response.*` envelopes are covered in the [Commands](commands.md) chapter — read that first.

## The run envelope contract

Every line is exactly one envelope with `class: "event"` and a `kind` starting with `event.`. Every event envelope carries a `RunContext` identifying the `run_id`, optional `task_id`, and the `tool` that produced it.

Minimum valid sequence for a successful run:

```
event.run.started
  (zero or more: event.run.progress | event.run.heartbeat |
                 event.artifact.created | event.artifact.updated |
                 event.message.warning | event.message.error |
                 event.log.line | event.metric |
                 event.checkpoint.committed | event.stage.started | event.stage.finished)
event.run.finished
```

The run MUST terminate with **exactly one** of:

- `event.run.finished` — completed successfully.
- `event.run.failed` — terminated by an error.
- `event.run.cancelled` — stopped in response to a `control.cancel` envelope from the host.

A run that exits without a terminal envelope is reported as a protocol violation by the host. A run that emits two terminal envelopes is also a violation.

Lines on stdout that don't parse as a `ocp-json/1` envelope are ignored by the host. Diagnostic output belongs on stderr.

---

## Emitting events

The `run.rs` pattern established by the toy-calculator plugin uses the local `wire_io.rs` helpers introduced in the [Commands](commands.md) chapter:

```rust
use ocp_types_v1::events::{RunStartedPayload, RunProgressPayload, RunFinishedPayload};
use ocp_types_v1::wire::{Duration, PathRef};
use crate::wire_io::{event_envelope, new_ulid, run_context, write_envelope};

let run_id = new_ulid();

// event.run.started
let ctx = run_context(&run_id, task_id);
let started = RunStartedPayload {
    seed: None,
    output_dir: Some(PathRef::Local {
        path: output_dir.display().to_string().replace('\\', "/"),
    }),
    argv: std::env::args().collect(),
    label: Some("demo run".into()),
    other: Default::default(),
};
write_envelope(stdout, &event_envelope("event.run.started", ctx, &started)?)?;

// ... progress, artifacts, etc. ...

// event.run.finished
let ctx = run_context(&run_id, task_id);
let finished = RunFinishedPayload {
    elapsed: Some(Duration::from_secs(3)),
    artifacts: vec![/* ArtifactRecords */],
    summary: Some("1 + 2 = 3".into()),
    metrics: Vec::new(),
    other: Default::default(),
};
write_envelope(stdout, &event_envelope("event.run.finished", ctx, &finished)?)?;
```

`event_envelope` wraps the payload, stamps a fresh ULID and timestamp, and attaches the `RunContext`. `write_envelope` emits a single NDJSON line and flushes, which is what the host requires for streaming.

All payload types live in [`ocp_types_v1::events`](https://docs.rs/ocp-types-v1) and share the same forward-compat rules: every struct has an `other: BTreeMap<String, Value>` slot so consumers preserve unknown fields added in future minor releases.

---

## Run lifecycle

### `event.run.started`

Emitted once, as the first envelope of the run.

```rust
pub struct RunStartedPayload {
    pub seed: Option<String>,         // stringified so it can exceed i64 range
    pub output_dir: Option<PathRef>,  // where artifacts will land
    pub argv: Vec<String>,            // the invocation, for audit
    pub label: Option<String>,        // optional human label
    pub other: BTreeMap<String, Value>,
}
```

`seed` is a string (not an integer) because the protocol supports seeds outside the JSON safe-integer range.

### `event.run.progress`

Emitted periodically. Optional, but strongly recommended for any run that runs for more than a second or two — the UI uses it to render progress bars and ETAs.

```rust
pub struct RunProgressPayload {
    pub iter_completed: Option<u64>,
    pub iter_target: Option<u64>,
    pub phase: Option<String>,        // e.g. "warmup", "sampling"
    pub fraction: Option<f64>,        // 0.0..=1.0, independent of iter counts
    pub metrics: Vec<ProgressMetric>, // free-form sampled metrics
    pub elapsed: Option<Duration>,
    pub remaining: Option<Duration>,  // ETA
    pub other: BTreeMap<String, Value>,
}
```

Use `fraction` for phases where iteration counts aren't meaningful. `metrics` is the place for domain values like acceptance rates, log-likelihood, or loss:

```rust
use ocp_types_v1::common::ProgressMetric;

ProgressMetric {
    name: "acceptance_rate".into(),
    value: 0.42,
    unit: None,
    min: Some(0.0),
    max: Some(1.0),
    other: Default::default(),
};
```

### `event.run.heartbeat`

A liveness ping carrying only an optional `elapsed`. Use it when the plugin is in a phase that can't easily produce `progress` envelopes but still needs to signal "I'm alive" so the host doesn't mark the run hung.

```rust
pub struct RunHeartbeatPayload {
    pub elapsed: Option<Duration>,
    pub other: BTreeMap<String, Value>,
}
```

### `event.run.finished` *(terminal)*

Emitted once, as the last envelope of a successful run.

```rust
pub struct RunFinishedPayload {
    pub elapsed: Option<Duration>,
    pub artifacts: Vec<ArtifactRecord>, // final roster
    pub summary: Option<String>,
    pub metrics: Vec<ProgressMetric>,   // final metric values
    pub other: BTreeMap<String, Value>,
}
```

The `artifacts` list is the canonical roster for the host's summary panel. Hosts reconcile it against every `event.artifact.*` envelope they received during the run, so be sure every artifact you reported earlier is also present here.

### `event.run.failed` *(terminal)*

Emitted once when the run ends with an error.

```rust
pub struct RunFailedPayload {
    pub error: String,                      // required
    pub error_code: Option<String>,         // vendor-namespaced
    pub cause_chain: Vec<String>,           // underlying causes
    pub elapsed: Option<Duration>,
    pub partial_artifacts: Vec<ArtifactRecord>,
    pub other: BTreeMap<String, Value>,
}
```

Do not also emit `event.run.finished` — `failed` is the terminal envelope in this case. List any artifacts written before the failure under `partial_artifacts` so the host can expose them in the run panel.

### `event.run.cancelled` *(terminal)*

Emitted once when the plugin ends the run in response to a `control.cancel` envelope on stdin.

```rust
pub struct RunCancelledPayload {
    pub reason: Option<String>,            // usually echoed from control.cancel
    pub elapsed: Option<Duration>,
    pub partial_artifacts: Vec<ArtifactRecord>,
    pub other: BTreeMap<String, Value>,
}
```

Only declare support for cancellation if your manifest lists the `control.cancel` capability; see [Manifest](manifest.md).

### `event.run.paused` / `event.run.resumed`

Optional lifecycle envelopes for plugins that support `control.pause` / `control.resume`. A paused run stays alive but emits no progress until resumed.

```rust
pub struct RunPausedPayload  { pub reason: Option<String>, pub other: ... }
pub struct RunResumedPayload {
    pub checkpoint_id: Option<String>,
    pub resumed_at_iter: Option<u64>,
    pub other: ...,
}
```

---

## Artifacts

### `event.artifact.created`

Emitted when a new artifact is written. The envelope's payload wraps an [`ArtifactRecord`](#artifactrecord).

```rust
pub struct ArtifactCreatedPayload {
    pub artifact: ArtifactRecord,
    pub other: BTreeMap<String, Value>,
}
```

### `event.artifact.updated`

Emitted when an artifact that was already announced is overwritten or appended to. The `artifact.artifact_id` MUST match the original `event.artifact.created` so the host can correlate updates.

```rust
pub struct ArtifactUpdatedPayload {
    pub artifact: ArtifactRecord,
    pub other: BTreeMap<String, Value>,
}
```

### `ArtifactRecord`

```rust
pub struct ArtifactRecord {
    pub artifact_id: String,             // stable across create/update — use a ULID
    pub path: PathRef,                   // Local, RunRelative, or Resource
    pub kind: String,                    // e.g. "result.csv", "summary.md"
    pub media_type: Option<String>,      // IANA media type
    pub digest: Option<ContentDigest>,   // SHA-256 recommended in final roster
    pub size_bytes: Option<u64>,
    pub created_at: Option<Timestamp>,
    pub modified_at: Option<Timestamp>,
    pub label: Option<String>,           // display name
    pub description: Option<String>,     // tooltip / long form
    pub ext: BTreeMap<String, Value>,    // vendor-namespaced extensions
    pub other: BTreeMap<String, Value>,
}
```

`kind` is how the host picks the icon and open-action. Use the standard values from `kinds-1.md` §8 where possible (`result.csv`, `summary.md`, `run.log`, `report.pdf`, …) and fall back to a vendor-namespaced kind (`<vendor>.<name>`) for anything domain-specific.

---

## Messages, logs, and metrics

### `event.message.warning` / `event.message.error`

Non-terminal diagnostics the user should see. The envelope's `kind` carries the severity; the payload repeats it so downstream tools parsing only the payload can still classify the line.

```rust
pub struct MessagePayload {
    pub severity: Severity,              // matches the envelope kind
    pub message: String,
    pub code: Option<String>,            // vendor-namespaced
    pub locator: Option<String>,         // file path, JSON pointer, etc.
    pub other: BTreeMap<String, Value>,
}
```

A `message.error` envelope is a recoverable error — the run is still going. For a terminal error, emit `event.run.failed` instead.

### `event.log.line`

A structured log line. Useful when the plugin wraps an external tool whose output you want to surface in the run panel.

```rust
pub struct LogLinePayload {
    pub severity: Severity,
    pub message: String,
    pub logger: Option<String>,          // e.g. "sampler", "io"
    pub fields: BTreeMap<String, Value>, // structured context
    pub ts: Option<Timestamp>,           // falls back to envelope ts
    pub other: BTreeMap<String, Value>,
}
```

### `event.metric`

A standalone metric sample, when a metric update doesn't fit a progress tick.

```rust
pub struct MetricPayload {
    pub metric: ProgressMetric,
    pub ts: Option<Timestamp>,
    pub other: BTreeMap<String, Value>,
}
```

---

## Composition (stages and checkpoints)

These envelopes are for wrapper plugins that run other plugins and for long-running runs that checkpoint mid-flight. Most plugins don't emit them.

### `event.stage.started` / `event.stage.finished`

A stage is a named sub-phase of a composition run. The envelope's `run.stage_id` identifies which stage the envelope belongs to; the payloads add human-readable context.

```rust
pub struct StageStartedPayload {
    pub label: Option<String>,
    pub description: Option<String>,
    pub other: BTreeMap<String, Value>,
}

pub enum StageOutcome { Success, Failure, Skipped, Cancelled, Other }

pub struct StageFinishedPayload {
    pub outcome: StageOutcome,
    pub elapsed: Option<Duration>,
    pub error: Option<String>,           // present when outcome is Failure
    pub summary: Option<String>,
    pub other: BTreeMap<String, Value>,
}
```

### `event.checkpoint.committed`

Emitted when a resume-capable run has safely written a checkpoint to disk.

```rust
pub struct CheckpointCommittedPayload {
    pub checkpoint_id: String,
    pub path: PathRef,
    pub iter_committed: Option<u64>,
    pub digest: Option<ContentDigest>,
    pub exact: Option<bool>,             // matches restart.exact / restart.approximate
    pub other: BTreeMap<String, Value>,
}
```

`exact` mirrors the restart capability declared in the manifest: `true` for `restart.exact` plugins that can resume bit-identical, `false` for `restart.approximate`.

---

## Host-to-plugin: `control.*`

The host can write `control.*` envelopes to the plugin's stdin — currently `control.cancel`, `control.pause`, and `control.resume`. Only enable these if the manifest declares the corresponding capability (`control.cancel`, `control.pause_resume`, `stdin.control_channel`). The protocol semantics are specified in `kinds-1.md` §4; the payload types live in the `controls` module once the host-side transport lands. For now, most plugins will declare only `control.cancel` and respond by emitting `event.run.cancelled`.
