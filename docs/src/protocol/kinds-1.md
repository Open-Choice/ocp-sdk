# ocp-json/1 — Kind Registry

This document is **normative**. It enumerates the standard envelope kinds defined by `ocp-json/1` and specifies the rules for vendor-defined kinds. It builds on `wire-format-1.md` and `envelope-1.md`.

The standard kind set is **frozen** as of `ocp-types-1.0.0`. New standard kinds may be added in `1.x` minor releases, but no existing standard kind may be renamed, removed, or have its semantics changed. See `CONTRIBUTING.md` for the governance rules.

## 1. Kind grammar

A `kind` value is a dot-separated namespaced string of the form:

```
<class>.<segment>+
```

where:

- `<class>` is one of `event`, `response`, `request`, `control` (matching the envelope's `class` field).
- `<segment>` is one or more path segments separated by dots.
- Each segment matches `^[a-z][a-z0-9_]*$` (lowercase ASCII, underscores allowed).
- Total length MUST NOT exceed 256 bytes.

A kind value MUST start with its class name, and the envelope's `class` field MUST match. An envelope with `"class": "event"` and `"kind": "response.validate"` is malformed and MUST be rejected by consumers.

## 2. Reserved namespaces

Within each class, the **first segment after the class** determines whether the kind is standard or vendor-defined.

| Pattern | Owner | Example |
|---|---|---|
| `<class>.<reserved-segment>.*` | `ocp-json/1` standard | `event.run.started`, `response.validate` |
| `<class>.<vendor>.*` where `<vendor>` matches the vendor identifier grammar | Third-party plugin | `event.maxdiff-pipeline.stage_started` |

The reserved segments for each class are listed in §§3–6 below. Vendor identifiers MUST NOT collide with reserved segments. The vendor identifier grammar is the same as for `ext` keys (see envelope spec §6.2): `^[a-z][a-z0-9-]*$`.

### 2.1 Reserved segment list

The following first segments are **reserved by `ocp-json/1`** and MUST NOT be used as vendor identifiers:

- Under `event.*`: `run`, `stage`, `checkpoint`, `artifact`, `message`, `log`, `metric`
- Under `response.*`: `validate`, `self_test`, `protocol`, `identity`, `capabilities`, `explain`, `examples`, `preview`
- Under `request.*`: `validate`, `self_test`, `protocol`, `identity`, `capabilities`, `explain`, `examples`, `preview`, `run`
- Under `control.*`: `cancel`, `pause`, `resume`, `heartbeat`, `deadline`

Any other first segment is available for vendor use, subject to the vendor identifier grammar.

## 3. Standard `event.*` kinds

Event kinds are emitted by the plugin during a `run` invocation. Each kind has a defined payload schema in the per-kind specifications (`events/<kind>.md`).

### 3.1 Run lifecycle

| Kind | Payload | Required emission | Purpose |
|---|---|---|---|
| `event.run.started` | `RunStartedPayload` | First event of every run | Marks the start of a run; carries seed, output dir, initial command |
| `event.run.heartbeat` | `RunHeartbeatPayload` | Optional, for long idle periods | Liveness ping when no other events would be emitted |
| `event.run.progress` | `RunProgressPayload` | Optional | Iteration / phase progress with metrics |
| `event.run.finished` | `RunFinishedPayload` | Last event of a successful run | Run completed successfully; final artifacts and summary |
| `event.run.failed` | `RunFailedPayload` | Last event of a failed run | Run terminated by error; partial artifacts and error details |
| `event.run.cancelled` | `RunCancelledPayload` | Last event of a cancelled run | Run terminated by `control.cancel`; partial artifacts |
| `event.run.paused` | `RunPausedPayload` | Optional, gated by `control.pause` capability | Run is suspended awaiting `control.resume` |
| `event.run.resumed` | `RunResumedPayload` | Optional, gated by `restart.exact` or `restart.approximate` capability | Run resumed from a checkpoint |

Every run produces exactly one terminal event from `{finished, failed, cancelled}`. Plugins MUST NOT exit cleanly without emitting one of these (catastrophic failures are signalled by non-zero exit code with no terminal event).

### 3.2 Composition

| Kind | Payload | Purpose |
|---|---|---|
| `event.stage.started` | `StageStartedPayload` | A composition stage begins (wrapper plugins only) |
| `event.stage.finished` | `StageFinishedPayload` | A composition stage ends (success, failure, or skip) |

Stage events are emitted by composition wrappers (plugins that internally orchestrate other plugins). Standalone plugins MUST NOT emit stage events. Wrappers MAY use stage events to demarcate logical phases visible in the host UI even when their child plugins don't speak the protocol.

### 3.3 Checkpoints

| Kind | Payload | Purpose |
|---|---|---|
| `event.checkpoint.committed` | `CheckpointCommittedPayload` | A persistent checkpoint has been written to disk |

Emitted by plugins that support `restart.exact` or `restart.approximate`. The payload identifies the checkpoint location and the iteration count it represents.

### 3.4 Artifacts

| Kind | Payload | Purpose |
|---|---|---|
| `event.artifact.created` | `ArtifactCreatedPayload` | A new file or object has been produced |
| `event.artifact.updated` | `ArtifactUpdatedPayload` | An existing artifact has been modified in place |

Artifact events MAY be emitted at any time during a run. The final `event.run.finished` envelope SHOULD include the complete artifact list in its payload, but consumers SHOULD also accumulate `event.artifact.*` envelopes throughout the run for live UI updates.

Artifacts are correlated by `artifact_id`, not by path. An artifact updated at stage 4 of a wrapper run SHOULD reuse the `artifact_id` from the `event.artifact.created` envelope at stage 1.

### 3.5 Messages

| Kind | Payload | Purpose |
|---|---|---|
| `event.message.warning` | `MessagePayload` | A non-fatal warning the user should see |
| `event.message.error` | `MessagePayload` | A recoverable error that did not terminate the run |

For terminal errors that DO terminate the run, use `event.run.failed`, not `event.message.error`. The distinction:

- `event.message.error` says "something went wrong but I am continuing." The run still produces a `finished` envelope eventually.
- `event.run.failed` says "I am stopping because of this error." It is the final envelope for the run.

### 3.6 Logs and metrics

| Kind | Payload | Purpose |
|---|---|---|
| `event.log.line` | `LogLinePayload` | Structured log line (gated by `events.log_line` capability) |
| `event.metric` | `MetricPayload` | Standalone metric sample (gated by `events.metric` capability) |

`event.log.line` is the structured alternative to writing to stderr. Use it when the host needs to filter, search, or correlate log lines. Plain stderr remains available for unstructured human output.

`event.metric` is for time-series metric samples that don't naturally piggyback on a `progress` event. For example, a long-running HB chain might emit `event.metric` every 10 seconds with current acceptance rate, regardless of iteration boundaries.

## 4. Standard `response.*` kinds

Response kinds are emitted exactly once per inspection invocation, followed by the plugin exiting.

| Kind | Triggered by | Payload | Purpose |
|---|---|---|---|
| `response.validate` | `exe api validate --command <c> --input-json <json>` | `ValidateResponsePayload` | Result of validating a params object against a command schema |
| `response.self_test` | `exe api self-test` | `SelfTestResponsePayload` | Result of running the plugin's internal health checks |

In `ocp-json/1` as currently practiced, `protocol`, `identity`, `capabilities`, `explain`, `examples`, and `preview` are served from static manifest assets at install time and are NOT emitted as live response envelopes by the binary. They are listed under reserved segments in §2.1 to prevent vendor collisions and to leave room for future hosts that re-introduce them as live RPC.

## 5. Standard `request.*` kinds

`request.*` kinds are reserved for future use (see envelope spec §7). The reserved set mirrors the response set:

- `request.validate`
- `request.self_test`
- `request.protocol`
- `request.identity`
- `request.capabilities`
- `request.explain`
- `request.examples`
- `request.preview`
- `request.run`

No `request.*` envelope is currently emitted in `ocp-json/1`. They are reserved so a future long-lived plugin transport can be added without protocol revision.

## 6. Standard `control.*` kinds

Control kinds are sent by the host to the plugin via stdin during a `run`. Each kind is gated by a capability flag in the static manifest; plugins that don't advertise the relevant capability MUST ignore (or never receive) the corresponding control envelope.

| Kind | Capability gate | Payload | Purpose |
|---|---|---|---|
| `control.cancel` | `control.cancel` | `ControlCancelPayload` | Stop the current run cleanly |
| `control.pause` | `control.pause` | `ControlPausePayload` | Suspend the current run |
| `control.resume` | `control.pause` | `ControlResumePayload` | Resume a paused run |
| `control.heartbeat` | `stdin.control_channel` (no specific gate) | `ControlHeartbeatPayload` | Liveness check from host |
| `control.deadline.extend` | `control.deadline` | `ControlDeadlinePayload` | Push back a soft deadline |

### 6.1 Cancellation semantics

When a plugin receives `control.cancel`:

1. The plugin SHOULD interrupt its work loop at the next safe interruption point.
2. The plugin SHOULD flush any in-progress checkpoints (if `restart.exact` is supported).
3. The plugin SHOULD emit any partial artifacts via `event.artifact.created`.
4. The plugin MUST emit `event.run.cancelled` as its terminal event.
5. The plugin MUST exit with status `0` (cancellation is not a failure).

The `ControlCancelPayload` MAY include a `deadline_ms` field. If present, it is a soft deadline: the plugin SHOULD complete its cancellation within that many milliseconds, after which the host MAY send SIGTERM/SIGKILL.

## 7. Vendor-defined kinds

Vendors MAY define their own kinds under their reserved namespace:

- `event.<vendor>.*`
- `response.<vendor>.*` (only when implementing custom inspection endpoints — rare)
- `request.<vendor>.*` (rare)
- `control.<vendor>.*` (rare)

### 7.1 Examples

Hypothetical wrapper plugin events:

```
event.maxdiff-pipeline.stage_started
event.maxdiff-pipeline.stage_finished
event.maxdiff-pipeline.bundled_component_loaded
```

Hypothetical HB-specific events:

```
event.numerious-hb.chain_state
event.numerious-hb.tuning_complete
event.numerious-hb.draws_committed
```

### 7.2 Rules for vendor kinds

1. Vendor kinds MUST follow the kind grammar in §1.
2. Vendor identifiers MUST NOT collide with reserved segments listed in §2.1.
3. Vendor kinds SHOULD NOT duplicate the semantics of standard kinds. If a vendor needs to emit a progress event, it SHOULD use `event.run.progress` and put vendor-specific data in `ext`, not invent `event.<vendor>.progress`.
4. Consumers that don't recognize a vendor kind MUST treat the envelope as opaque (preserve, optionally display, do not error).
5. Vendor kinds are NOT registered in this document. Vendors MAY publish their own kind documentation alongside their plugins.

### 7.3 Promotion path

If a vendor kind proves broadly useful, it MAY be promoted to a standard kind in a future `ocp-types-1.x` minor release. Promotion follows the same rules as `ext` field promotion (envelope spec §6.5):

1. The standard kind is added to this document.
2. The vendor kind is **deprecated but still accepted**. Producers SHOULD migrate within one minor release.
3. The vendor kind MAY remain registered indefinitely as an alias to preserve forward compatibility.

## 8. Standard output kinds

`OutputDescriptor.kind` and `ArtifactRecord.kind` use a parallel namespace to envelope kinds. The same rules apply: standard kinds (no vendor prefix) are reserved by `ocp-json/1`; vendor kinds use `<family>.<kind>`.

### 8.1 Reserved standard output kinds

| Kind | Media type | Description |
|---|---|---|
| `result.json` | `application/json` | Primary result file in JSON form |
| `result.csv` | `text/csv` | Primary result file in CSV form |
| `result.parquet` | `application/vnd.apache.parquet` | Primary result file in Parquet form |
| `run.log` | `text/plain` | Plain-text run log |
| `run.log.json` | `application/x-ndjson` | NDJSON-structured run log (mirror of the event stream) |
| `summary.md` | `text/markdown` | Human-readable summary in Markdown |
| `summary.json` | `application/json` | Machine-readable summary in JSON |
| `manifest.json` | `application/json` | Metadata manifest describing all artifacts in the run |
| `checkpoint` | `application/octet-stream` | Opaque checkpoint blob for restart |
| `diagnostic.json` | `application/json` | Diagnostic data (tuning info, convergence stats) |
| `diagnostic.png` | `image/png` | Diagnostic plot |
| `diagnostic.svg` | `image/svg+xml` | Diagnostic plot in vector form |
| `input.echo` | varies | Echo of the original input for reproducibility |
| `params.json` | `application/json` | Normalized parameter set actually used |

### 8.2 Vendor output kinds

Vendors MAY define their own output kinds under their family namespace:

```
maxdiff-turf.curve_csv
maxdiff-turf.ladder_csv
numerious-hb.draws_postcard
numerious-hb.chain_summary
```

Hosts that don't recognize a vendor output kind SHOULD render the artifact as a generic file with its declared media type.

## 9. Conformance

A producer claims kind conformance for `ocp-json/1` if:

1. Every envelope it emits has a `kind` value matching the grammar in §1.
2. Every standard kind it emits has a payload conforming to that kind's payload schema.
3. Every vendor kind it emits is namespaced under a vendor identifier the producer owns.
4. It does NOT emit any reserved-but-not-implemented standard kind (e.g., `request.run`).

A consumer claims kind conformance if:

1. It correctly parses every standard kind defined in §§3–6.
2. It tolerates unknown kinds (preserves and forwards or ignores; does not crash).
3. It correctly dispatches by `(class, first_segment_after_class)` rather than by full kind string match.

The `ocp-conformance` test corpus contains fixtures for every standard kind and a representative sample of unknown-kind handling cases.

## 10. Forbidden changes (governance reference)

The following changes to this document are **forbidden** once `ocp-types-1.0.0` is tagged:

1. Removing any standard kind listed in §§3–6.
2. Renaming any standard kind.
3. Changing the payload schema of any standard kind in a way that would invalidate previously-conforming producers.
4. Removing any reserved segment from §2.1.
5. Changing the kind grammar in §1.
6. Reserving a previously-unreserved segment in `event.*`, `response.*`, `request.*`, or `control.*` if doing so would collide with any known vendor identifier in active use.

Permitted changes are:

1. Adding new standard kinds to §§3–6 (additive).
2. Adding new standard output kinds to §8.1 (additive).
3. Tightening documentation (e.g., specifying a previously-unspecified payload field as required).
4. Promoting a vendor kind to a standard kind (with the original vendor kind remaining as an accepted alias).
