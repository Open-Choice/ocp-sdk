# Open Choice Protocol SDK

The Open Choice Protocol SDK (`ocp-sdk`) is the shared Rust contract between plugin executables and the Open Choice host application. Every plugin and every host implementation links the same frozen wire-format crate (`ocp-types-v1`) so that NDJSON envelopes round-trip losslessly across the boundary.

## Crates

| Crate | Purpose |
|---|---|
| `ocp-types-v1` | The frozen `ocp-json/1` wire-format surface: `Envelope`, `Kind`, `RunContext`, event/response payloads, manifest types, capabilities. |
| `ocp-conformance` | Golden-file conformance tests that any `ocp-json/1` producer or consumer can run against its serialized envelopes. |
| `oc-core` | `.oce` task file parser; resolves aliases to plugin IDs, produces `oc-task-file/1` JSON. |
| `ocp-host` | Host-side utilities: `ToolProtocolClient` (wraps the `api *` subcommands), registry fetcher, project layout helpers. |
| `oc-sign` | Developer CLI: `keygen`, `sign`, `pack` — produces `.ocplugin` zip archives with Ed25519 signatures. |
| `oc-runner` | Runner executable used by the host and by integration tests to drive a plugin through a task file. |
| `oc-cli` | Standalone CLI for invoking plugins outside of the host app (validate, self-test, run). |

## Audience

**Plugin authors** depend on `ocp-types-v1` — the frozen wire types their binary serializes and deserializes. Start with the [Quickstart](guide/quickstart.md).

**Host implementations** (the app, test harnesses, CI) depend on `ocp-host` and `oc-core`, which pull in `ocp-types-v1` transitively. They can use `ocp-conformance` to validate their own envelope handling.

**Developer tooling** — install `oc-sign` once from `crates/oc-sign` to package and sign plugins.

## The `ocp-json/1` surface

Everything on the wire is a single `Envelope` type with four classes:

- `event.*` — streamed by plugins during a run (NDJSON on stdout).
- `response.*` — emitted once per inspection invocation (`api validate`, `api self-test`).
- `control.*` — sent by the host to the plugin on stdin.
- `request.*` — reserved for future bidirectional transports.

Every envelope carries an `ocp: "1"` version tag, a `Kind` (`event.run.started`, `response.validate`, …), a ULID `id`, a `ts` timestamp, an optional `RunContext`, and a kind-specific `payload`. Unknown fields are preserved verbatim through `#[serde(flatten)] other` slots, so future minor releases of `ocp-json/1` are forward-compatible with every tool that depends on `ocp-types-v1`.

## API reference

The full rustdoc API reference is published to [docs.rs](https://docs.rs/ocp-types-v1) on each crates.io release.
