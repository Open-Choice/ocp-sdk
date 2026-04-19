# ocp-json/1 — Envelope Specification

This document is **normative**. It defines the structure of every NDJSON line in an `ocp-json/1` stream: the envelope. It builds on `wire-format-1.md` and is built upon by `kinds-1.md`, `capabilities-1.md`, and the per-kind payload schemas.

The envelope structure is **frozen forever** as of `ocp-json/1`. New fields may be added under the rules in `CONTRIBUTING.md`, but the existing fields, their semantics, and their JSON shape will not change for the lifetime of `ocp-json/1`.

## 1. Overview

Every NDJSON line in an `ocp-json/1` stream is a single JSON object called an **envelope**. The envelope has a fixed set of header fields shared by all messages, plus a class-specific body. There are exactly four envelope classes:

| Class | Direction | Used for |
|---|---|---|
| `event` | plugin → host | Asynchronous events emitted during a run (progress, artifacts, lifecycle, logs, metrics, errors) |
| `response` | plugin → host | One-shot replies to inspection invocations (`api validate`, `api self-test`) |
| `request` | host → plugin | Reserved for future structured request transport (currently expressed via CLI args) |
| `control` | host → plugin | Live control messages over stdin (cancel, pause, resume) |

The class is determined by the `class` field. Consumers dispatch on `class` first, then on `kind`.

## 2. Universal envelope fields

Every envelope, regardless of class, MUST contain the following fields:

| Field | Type | Required | Description |
|---|---|---|---|
| `ocp` | string | Yes | Wire format version. MUST be `"1"` for `ocp-json/1`. |
| `class` | string | Yes | Envelope class. One of `event`, `response`, `request`, `control`. |
| `id` | Identifier | Yes | Globally unique identifier for this envelope. ULID by default. |
| `ts` | Timestamp | Yes | Wall-clock time at which the envelope was emitted. |
| `kind` | string | Yes | Namespaced kind discriminator. See `kinds-1.md`. |
| `ext` | object | No | Vendor-namespaced extension bag (see §6). May be omitted if empty. |

The combination `(class, kind)` uniquely determines the shape of the envelope's class-specific fields and payload.

### 2.1 Example

```json
{
  "ocp": "1",
  "class": "event",
  "id": { "fmt": "ulid", "value": "01HXYZ7K8RKZBC6DPVQVT9N5MN" },
  "ts": "2026-04-07T12:34:56.123456Z",
  "kind": "event.run.progress",
  "run": {
    "run_id": { "fmt": "ulid", "value": "01HXYZ7K8RKZBC6DPVQVT9N5M0" },
    "task_id": "stage_03_turf",
    "originating_tool": { "family": "maxdiff-turf", "name": "maxdiff-turf", "version": "0.1.0" }
  },
  "payload": {
    "phase": "running",
    "iteration": { "completed": 250, "target": 1000 },
    "elapsed": { "seconds": 12, "nanos": 345000000 },
    "metrics": [
      { "name": "best_value", "value": 0.847 }
    ]
  }
}
```

## 3. The `event` class

`event` envelopes are emitted by the plugin during a `run` invocation. They form the runtime event stream.

### 3.1 Required fields

| Field | Type | Required | Description |
|---|---|---|---|
| `ocp` | string | Yes | (universal) |
| `class` | `"event"` | Yes | (universal) |
| `id` | Identifier | Yes | (universal) |
| `ts` | Timestamp | Yes | (universal) |
| `kind` | string | Yes | A `kind` value from the `event.*` namespace. See `kinds-1.md`. |
| `run` | RunContext | Yes | Composition envelope. See §5. |
| `payload` | object | Conditional | Kind-specific payload. Required for kinds that define a payload. |
| `ext` | object | No | (universal) |

### 3.2 Ordering and reliability

- Events MUST be emitted in causal order: `event.run.started` before any other event for that run, `event.run.finished` (or `failed` / `cancelled`) as the last event, and so on.
- Events are not numbered or acknowledged. The host receives them in stdout order, which is the producer's emit order, which is the causal order by construction.
- Events MUST NOT be retransmitted. If a producer wants to update an artifact's metadata, it emits a fresh `event.artifact.updated` envelope, not a re-emission of the original `event.artifact.created`.

## 4. The `response` class

`response` envelopes are emitted by the plugin in reply to inspection invocations (`api validate`, `api self-test`). Exactly one response envelope is emitted per invocation, followed by the plugin exiting.

### 4.1 Required fields

| Field | Type | Required | Description |
|---|---|---|---|
| `ocp` | string | Yes | (universal) |
| `class` | `"response"` | Yes | (universal) |
| `id` | Identifier | Yes | (universal) |
| `ts` | Timestamp | Yes | (universal) |
| `kind` | string | Yes | A `kind` value from the `response.*` namespace. |
| `ok` | boolean | Yes | Whether the response represents success (`true`) or a structured failure (`false`). |
| `tool` | ToolRef | Yes | Identity of the tool emitting the response. |
| `payload` | object | Conditional | Kind-specific payload. Required for kinds that define a payload. |
| `issues` | ValidationIssue[] | No | Diagnostic issues (warnings, errors) attached to the response. May be omitted if empty. |
| `ext` | object | No | (universal) |

### 4.2 The `ok` field

- `ok: true` means the response represents a successful execution of the inspection endpoint.
- `ok: false` means the response represents a **structured failure** — the endpoint ran to completion but the result indicates a problem (e.g., `api validate` was called with input that fails schema validation). The `issues` array MUST contain at least one entry with severity `error`.
- Process exit code is independent of `ok`: a structured failure exits with status `0`. Non-zero exit code indicates a **catastrophic failure** where the response envelope may be missing or malformed.

### 4.3 The `tool` field

`tool` is a ToolRef identifying the plugin emitting the response. Its shape:

```json
{
  "family": "maxdiff-turf",
  "name": "maxdiff-turf",
  "version": "0.1.0"
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `family` | string | Yes | Tool family identifier. Multiple tools may share a family (e.g., `model-hb` family contains `hb-cli`, `hb-cv`, ...). |
| `name` | string | Yes | Tool name within the family. |
| `version` | string | Yes | Semantic version. |

ToolRef does NOT carry bundled-component information; that lives in the static manifest.

## 5. The `RunContext` sub-object

`RunContext` carries everything an envelope needs to know about which run it belongs to, who produced it, and where it sits in any composition hierarchy. It is **only** present on `event` envelopes (and on `response` envelopes that pertain to a specific run, though most response kinds are run-independent).

### 5.1 Structure

```json
{
  "run": {
    "run_id": { "fmt": "ulid", "value": "01HXYZ7K8RKZBC6DPVQVT9N5M0" },
    "task_id": "stage_03_turf",
    "parent_run_id": { "fmt": "ulid", "value": "01HXYZ7K8RKZBC6DPVQVT9N5M1" },
    "run_chain": [
      { "fmt": "ulid", "value": "01HXYZ7K8RKZBC6DPVQVT9N5M2" }
    ],
    "stage_id": "03_turf",
    "originating_tool": {
      "family": "maxdiff-turf",
      "name": "maxdiff-turf",
      "version": "0.1.0"
    },
    "run_metadata": {
      "seed": { "value": 42, "generator": "xoshiro256pp", "deterministic": true },
      "host_platform": "windows-x86_64",
      "host_application": { "name": "research-scaffolding", "version": "0.3.1" }
    },
    "ext": {}
  }
}
```

### 5.2 Required fields

| Field | Type | Required | Description |
|---|---|---|---|
| `run_id` | Identifier | Yes | Identifier for this run. Globally unique. |
| `task_id` | string | Yes | Task identifier from the `.scaffolding` task file. See wire format §10.2. |
| `originating_tool` | ToolRef | Yes | Identity of the tool that produced this message. For relayed messages from a wrapper, this is the **child** tool, not the wrapper. |

### 5.3 Composition fields

| Field | Type | Required | Description |
|---|---|---|---|
| `parent_run_id` | Identifier | No | The immediate parent's `run_id`, if this run is nested inside another. Absent for top-level runs. |
| `run_chain` | Identifier[] | No | Full ancestry chain, **root first**. The current `run_id` is NOT in this list. The immediate parent IS in this list (and is also `parent_run_id`). For top-level runs, this is empty or absent. |
| `stage_id` | string | No | Composition stage identifier set by wrappers. Wrapper scope only; standalone runs leave this absent. See wire format §10.3. |

#### 5.3.1 Composition example: 3-deep wrapping

A run that is two layers deep (outermost wrapper → middle wrapper → child stage):

```json
{
  "run": {
    "run_id": { "fmt": "ulid", "value": "01H...child" },
    "task_id": "stage_03_turf",
    "parent_run_id": { "fmt": "ulid", "value": "01H...middle" },
    "run_chain": [
      { "fmt": "ulid", "value": "01H...outermost" },
      { "fmt": "ulid", "value": "01H...middle" }
    ],
    "stage_id": "03_turf",
    "originating_tool": { "family": "maxdiff-turf", "name": "maxdiff-turf", "version": "0.1.0" }
  }
}
```

The host can render the run tree by grouping events by `run_chain[0]` (outermost root) and recursively descending.

### 5.4 Run metadata

```json
{
  "run_metadata": {
    "seed": { "value": 42, "generator": "xoshiro256pp", "deterministic": true },
    "host_platform": "windows-x86_64",
    "host_application": { "name": "research-scaffolding", "version": "0.3.1" },
    "ext": {}
  }
}
```

| Field | Type | Required | Description |
|---|---|---|---|
| `seed` | RunSeedInfo | No | Random seed information for deterministic runs. |
| `host_platform` | string | No | Platform identifier (`windows-x86_64`, `linux-x86_64`, `darwin-aarch64`). |
| `host_application` | object | No | Information about the host application (name + version). |
| `ext` | object | No | Vendor extension bag. |

`run_metadata` SHOULD only appear on `event.run.started` envelopes; downstream events MAY omit it to reduce envelope size. Hosts MUST NOT require it on every event.

## 6. The `Ext` extension bag

`ext` is a vendor-namespaced JSON object attached to every public type in the protocol. It is the universal mechanism for extending the protocol without modifying its standard fields.

### 6.1 Structure

```json
{
  "ext": {
    "maxdiff-pipeline.stage_dir": "./runs/curve_family/03_turf",
    "maxdiff-pipeline.parent_pipeline_version": "0.1.0",
    "numerious.cost_estimate_usd": 0.0024
  }
}
```

### 6.2 Key naming rules

- Every key in an `ext` object MUST be of the form `<vendor>.<field>` where `<vendor>` is a vendor identifier and `<field>` is a vendor-defined field name.
- `<vendor>` MUST match the regular expression `^[a-z][a-z0-9-]*$` (lowercase ASCII letters, digits, and hyphens; starting with a letter).
- `<field>` MUST match the regular expression `^[a-z][a-z0-9_.]*$` (lowercase ASCII letters, digits, underscores, and dots; starting with a letter).
- Vendor identifiers SHOULD be DNS-compatible to avoid collisions across organizations.
- The `ocp-json/1` specification reserves the vendor identifier `ocp` for protocol-internal use; vendors MUST NOT use `ocp.*` keys.
- Keys MUST NOT exceed 256 bytes.

### 6.3 Value rules

- `ext` values MAY be any valid JSON: strings, numbers, booleans, arrays, nested objects, or null.
- Producers SHOULD prefer simple scalar values for searchability.
- Producers MAY embed structured objects under nested `ext` fields, in which case any `ext` sub-object MUST follow the same naming rules recursively.
- The `ext` field of an `Ext` object MUST itself be omitted; nesting `ext` inside `ext` is forbidden.

### 6.4 Round-trip rule

- Consumers MUST preserve every key in `ext` when round-tripping a message, even if the consumer does not recognize the vendor or the field.
- Consumers SHOULD NOT log warnings for unknown `ext` keys; they are extensions, not errors.

### 6.5 Promotion path

A vendor extension that proves broadly useful MAY be promoted to a standard field in a future `ocp-types-1.x` minor release. When this happens:

1. The standard field is added to the appropriate type with `#[serde(default)]` (or its language equivalent).
2. The corresponding `ext` key is **deprecated but still accepted**. Producers SHOULD migrate to the standard field within one minor release.
3. The `ext` key is **never removed**. Removing it would break round-trip preservation for older producers.

This rule preserves the load-bearing forward-compatibility property: old producers continue working forever.

## 7. The `request` class

`request` envelopes are reserved for future use. In `ocp-json/1` as currently practiced, all host→plugin requests are expressed via CLI arguments (`exe api validate --command ...`), not via NDJSON envelopes. The `request` class exists in the wire format so that a future host transport (e.g., a long-lived plugin daemon spoken to over a Unix socket) can be added without a protocol revision.

### 7.1 Required fields (when used)

| Field | Type | Required | Description |
|---|---|---|---|
| `ocp` | string | Yes | (universal) |
| `class` | `"request"` | Yes | (universal) |
| `id` | Identifier | Yes | (universal) |
| `ts` | Timestamp | Yes | (universal) |
| `kind` | string | Yes | A `kind` value from the `request.*` namespace. |
| `payload` | object | Conditional | Request-specific payload. |
| `ext` | object | No | (universal) |

Plugins that do not implement the long-lived transport MUST ignore any `request` envelope received on stdin and SHOULD log a warning to stderr.

## 8. The `control` class

`control` envelopes are sent by the host to the plugin via stdin during a `run`. They are the live control channel for cancellation, pausing, and similar operations.

### 8.1 Required fields

| Field | Type | Required | Description |
|---|---|---|---|
| `ocp` | string | Yes | (universal) |
| `class` | `"control"` | Yes | (universal) |
| `id` | Identifier | Yes | (universal) |
| `ts` | Timestamp | Yes | (universal) |
| `kind` | string | Yes | A `kind` value from the `control.*` namespace. |
| `payload` | object | Conditional | Control-specific payload. |
| `ext` | object | No | (universal) |

### 8.2 Capability gating

Plugins MUST NOT honor any control envelope unless they advertise the `stdin.control_channel` capability in their static manifest. Plugins that do not advertise this capability SHOULD have stdin closed by the host immediately after spawn (see wire format §7.3).

A plugin that advertises `stdin.control_channel` MAY further opt into specific control kinds via additional capabilities:

- `control.cancel` → permits the plugin to receive `control.cancel` envelopes
- `control.pause` → permits `control.pause` and `control.resume`

Hosts MUST NOT send a control envelope of a kind the plugin has not opted into.

## 9. The `RawEnvelope` round-trip type (informative)

This section is informative; it describes a parser pattern, not a wire format requirement.

For consumers that need to **forward** envelopes losslessly without fully parsing their payloads — most importantly, composition wrappers that relay child events to a parent host — the recommended pattern is to parse into a `RawEnvelope` type that captures the universal header fields and stashes the rest of the JSON object verbatim:

```rust
pub struct RawEnvelope {
    pub ocp: String,
    pub class: String,
    pub id: Identifier,
    pub ts: Timestamp,
    pub kind: Kind,

    pub run: Option<RunContext>,
    pub payload: Option<serde_json::Value>,

    /// Catches any other top-level fields the consumer doesn't recognize.
    /// This is what makes round-tripping safe across protocol versions.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,

    pub ext: Ext,
}
```

The `other` field absorbs any envelope-level field added in a future protocol version. The wrapper preserves it during relay without knowing what it means.

Wrapper relay is then a small, kind-agnostic transformation:

```rust
fn relay(child_line: &str, wrapper: &RunContext, stage: &str) -> String {
    let mut env: RawEnvelope = serde_json::from_str(child_line).unwrap();
    if let Some(run) = env.run.as_mut() {
        run.run_chain.insert(0, wrapper.run_id.clone());
        run.parent_run_id = Some(wrapper.run_id.clone());
        run.stage_id = Some(stage.to_string());
        // originating_tool is intentionally NOT modified — it stays the child's identity
    }
    serde_json::to_string(&env).unwrap()
}
```

This transformation works for any kind, including kinds the wrapper does not recognize, because it never reads the `payload` field.

## 10. Forward compatibility rules

These rules apply to consumers of envelopes. Together they make `ocp-json/1` permanently extensible.

1. **Unknown top-level fields** in an envelope MUST be preserved during round-trip. Use the `#[serde(flatten)]` (or equivalent) catch-all pattern shown in §9.
2. **Unknown `kind` values** MUST NOT cause a parse error. Consumers that don't recognize a kind SHOULD treat the envelope as opaque and either ignore or forward it.
3. **Unknown `class` values** MUST NOT cause a parse error. Consumers that don't recognize a class SHOULD discard the envelope and continue reading subsequent envelopes.
4. **Unknown `ext` keys** MUST be preserved during round-trip and MUST NOT cause warnings.
5. **Unknown enum variants** in any wrapped/tagged field (`ContentDigest.algo`, `Identifier.fmt`, `PathRef.kind`, etc.) MUST be preserved during round-trip. Consumers that need to use the value MAY emit a warning and skip the affected field, but MUST NOT abort processing of the envelope.
6. **Missing optional fields** MUST be treated as if absent (typically `None` or `null`), never as an error.
7. **`deny_unknown_fields`** (or its language equivalent) MUST NOT be used on any wire-format type. This is the single most common source of forward-compatibility breakage and is therefore explicitly forbidden by the conformance suite.

## 11. The wrapped primitive types

The following types appear throughout the envelope and its payloads. Each is wrapped in a tagged JSON shape so that future formats can be added without breaking existing consumers.

### 11.1 `Identifier`

```json
{ "fmt": "ulid", "value": "01HXYZ7K8RKZBC6DPVQVT9N5MN" }
```

| Field | Type | Required | Description |
|---|---|---|---|
| `fmt` | string | Yes | Identifier format. One of `ulid`, `uuid`, `opaque`. |
| `value` | string | Yes | Identifier string in the format indicated. |

Defined formats:
- `ulid` — 26-char Crockford base32, time-sortable. Default for `ocp-json/1`.
- `uuid` — 36-char RFC 4122 UUID, lowercase, hyphenated.
- `opaque` — arbitrary string, 1–256 bytes, matching `^[A-Za-z0-9_.-]+$`.

Future formats may be added via the additive enum-variant rule.

### 11.2 `Timestamp`

A bare JSON string in RFC 3339 form with mandatory `Z` suffix and microsecond precision (or higher). See wire format §9.1.

```json
"2026-04-07T12:34:56.123456Z"
```

This is the only primitive that is NOT wrapped in a tagged object. The string format is sufficient because RFC 3339 with `Z` suffix is unambiguous and stable.

### 11.3 `Duration`

```json
{ "seconds": 12345, "nanos": 678900000 }
```

See wire format §9.2.

### 11.4 `ContentDigest`

```json
{ "algo": "sha-256", "digest": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855" }
```

| Field | Type | Required | Description |
|---|---|---|---|
| `algo` | string | Yes | Hash algorithm. One of `sha-256`, `sha-384`, `sha-512`, `blake3`. |
| `digest` | string | Yes | Lowercase hex digest. Length depends on algorithm. |

Future algorithms may be added via the additive enum-variant rule. Consumers that don't recognize an algorithm MUST preserve the digest verbatim and MAY treat trust decisions involving it as inconclusive.

### 11.5 `PathRef`

```json
{ "kind": "local", "path": "C:/Users/example/runs/01HXYZ.../result.json" }
```

Variants:

```json
{ "kind": "local", "path": "/abs/path/to/file" }
{ "kind": "url", "url": "https://example.com/data.csv" }
{ "kind": "run-relative", "path": "03_turf/result.json" }
{ "kind": "content-addressed", "digest": { "algo": "sha-256", "digest": "..." }, "hint": "result.json" }
```

| Variant | Field | Type | Required | Description |
|---|---|---|---|---|
| `local` | `path` | string | Yes | Absolute filesystem path, forward slashes only. |
| `url` | `url` | string | Yes | HTTP(S) URL. |
| `run-relative` | `path` | string | Yes | Path relative to the run's output directory, forward slashes only. |
| `content-addressed` | `digest` | ContentDigest | Yes | Content hash for retrieval from a content store. |
| `content-addressed` | `hint` | string | No | Optional human-readable file name for display. |

Future variants may be added via the additive enum-variant rule.

### 11.6 `ToolRef`

```json
{ "family": "maxdiff-turf", "name": "maxdiff-turf", "version": "0.1.0" }
```

See §4.3. Carries no `ext` bag (it is a stable identity, not a message).

## 12. Conformance

A producer or consumer claims envelope conformance for `ocp-json/1` if and only if it passes every test in the `envelope/` section of the `ocp-conformance` test corpus. The corpus is the operative definition of conformance; this document is the explanatory definition.

## 13. Forbidden changes (governance reference)

The following changes to this document are **forbidden** once `ocp-types-1.0.0` is tagged:

1. Removing any field listed in any "Required fields" table.
2. Removing any envelope class.
3. Removing any wrapped primitive type or any of its currently-defined variants.
4. Renaming any field on the wire (the JSON name; Rust binding names may change freely).
5. Changing the `ext` key naming rules.
6. Tightening any "MAY" or "SHOULD" rule into a "MUST" in a way that would invalidate previously-conforming implementations.
7. Adding any required field to any envelope class. (New optional fields are permitted.)
8. Changing the meaning of `ok` in `response` envelopes.
9. Changing the structure of `RunContext` in any way other than adding new optional fields.
10. Adding `deny_unknown_fields` (or equivalent) to any wire-format type.

Permitted changes are listed in `CONTRIBUTING.md`.
