# ocp-json/1

Open Choice protocol version 1 uses JSON envelopes for inspection endpoints and newline-delimited JSON events for runtime streaming.

This page is the **umbrella overview**. The normative protocol is split across four companion documents, which together define `ocp-json/1`:

- [Wire Format](wire-format-1.md) — UTF-8, NDJSON framing, JSON profile, primitive types
- [Envelope](envelope-1.md) — envelope classes, universal fields, `RunContext`, extension bag
- [Kind Registry](kinds-1.md) — standard event/response/control kinds and vendor extension rules
- [Capability Registry](capabilities-1.md) — standard capability flags and negotiation rules

Where this page and the companion documents disagree, the companion documents win. This page exists to give a tour of the protocol surface, document the host Tauri commands, and describe the static package layout — content that is not yet covered by a normative spec file.

The protocol is **frozen** as of `ocp-types-1.0.0`. See [`CONTRIBUTING.md`](../../../CONTRIBUTING.md) for the governance rules.

## Plugin package layout

A plugin is distributed as a `.ocplugin` zip archive. The host reads static metadata from the package at install time — no binary is executed during installation. The layout is:

```
manifest.json                          # plugin identity, commands, protocol, capabilities
bin/<os>-<arch>/<executable>           # platform binary (e.g. bin/windows-x86_64/tool.exe)
signatures/manifest.sig                # Ed25519 signature over manifest.json
schemas/<command>.schema.json          # JSON Schema for each command's parameters
examples/<command>.json                # example parameter sets per command
help/<command>.json                    # field-level help text per command
outputs/<command>.json                 # output descriptor catalog per command
```

All static assets are cached in the host's SQLite database at install time.

## Manifest fields

`manifest.json` contains all plugin identity, protocol, and capability information:

```json
{
  "schema_version": "1",
  "plugin_id": "com.example.my-tool",
  "display_name": "My Tool",
  "version": "1.0.0",
  "publisher": "Example Org",
  "description": "...",
  "runtime": {
    "type": "native-sidecar",
    "entrypoints": [{"os": "windows", "arch": "x86_64", "path": "bin/windows-x86_64/my-tool.exe", "digest": {"algorithm": "sha256", "value": "..."}}]
  },
  "protocol": {"family": "ocp-json", "version": "1"},
  "commands": ["calculate"],
  "capabilities": ["events.progress", "control.cancel", "stdin.control_channel"],
  "sandbox": {"fs_read": [], "fs_write": ["plugin-workdir"], "network": false},
  "signing": {"key_id": "open-choice-2026", "signature_path": "signatures/manifest.sig", "algorithm": "ed25519"}
}
```

## Binary CLI surface

The installed binary implements only the endpoints that require live execution. The remaining inspection data is read from the package's static assets.

### Binary endpoints

| Invocation | Envelope kind | Payload type | Notes |
|---|---|---|---|
| `exe api validate --command <c> --input-json <json>` | `response.validate` | `ValidateResponsePayload` | Validates a params object against the command's schema |
| `exe api self-test` | `response.self_test` | `SelfTestResponsePayload` | Optional; runs live internal checks |
| `exe run <file.oce.run.tmp> --task <id> [--output-format ...]` | `event.*` | (one payload per kind) | NDJSON stream of run events |

The following endpoints were present in earlier protocol drafts but are now replaced by static package assets:

| Former invocation | Replaced by |
|---|---|
| `exe api protocol` | `manifest.protocol` |
| `exe api identity` | `manifest.plugin_id`, `display_name`, `version`, etc. |
| `exe api capabilities` | `manifest.commands[]` |
| `exe api schema --command <c>` | `schemas/<c>.schema.json` in zip |
| `exe api outputs --command <c>` | `outputs/<c>.json` in zip |
| `exe api examples` | `examples/<c>.json` in zip |
| `exe api explain --command <c>` | `help/<c>.json` in zip |

### Inspection response envelope

Every inspection response is a single `ocp-json/1` envelope with `class: "response"` written to stdout as one NDJSON line. See [`envelope-1.md`](envelope-1.md) for the full field set; a minimal `response.validate` envelope looks like:

```json
{
  "ocp": "1",
  "class": "response",
  "id": {"fmt": "ulid", "value": "01HQRZ..."},
  "ts": "2026-04-07T12:34:56.123456Z",
  "kind": "response.validate",
  "payload": {"ok": true, "issues": [], "normalized_params": null}
}
```

- `payload.ok: false` with a non-empty `issues` array means the endpoint returned a structured failure (e.g. bad input to `validate`). Exit code is still 0.
- Non-zero exit code means catastrophic failure; stdout may be empty or malformed.

## Runtime invocation

```
exe run <path/to/file.oce.run.tmp> --task <task-id> [--output-format protocol|human|quiet]
```

The exe reads the named task, executes it, and writes output to stdout. Stderr is for unstructured diagnostics only.

### `--output-format`

Controls how the exe formats its stdout during `run`. All compliant exes must support this flag.

| Value | Stdout content | When to use |
|---|---|---|
| `protocol` | NDJSON stream of `event.*` envelopes (one per line) | Always used by hosts |
| `human` | Pretty terminal output — step-by-step progress, artifact summary, timing | Default when stdout is a TTY |
| `quiet` | Single result line only | Scripts and CI |

**Auto-detection rule:** when `--output-format` is absent, the exe should detect whether stdout is a TTY:
- TTY → behave as `human`
- Pipe / redirect → behave as `protocol`

## Runtime event kinds

The full runtime event kind set is normative in [`kinds-1.md`](kinds-1.md). The standard `event.*` kinds at a glance:

- Run lifecycle: `event.run.started`, `event.run.heartbeat`, `event.run.progress`, `event.run.finished`, `event.run.failed`, `event.run.cancelled`, `event.run.paused`, `event.run.resumed`
- Composition: `event.stage.started`, `event.stage.finished`
- Checkpoints: `event.checkpoint.committed`
- Artifacts: `event.artifact.created`, `event.artifact.updated`
- Messages: `event.message.warning`, `event.message.error`
- Logs and metrics: `event.log.line`, `event.metric`

Each event is wrapped in the standard envelope shape defined in [`envelope-1.md`](envelope-1.md). The envelope's `class` is always `"event"` and its `kind` matches one of the names above (or a vendor-namespaced kind, per `kinds-1.md` §7).

## Host plugin commands (Tauri surface)

The Open Choice Tauri backend manages plugins via the commands below. JS calls these via `__TAURI__.core.invoke()`.

### Installation and management

| Tauri command | Notes |
|---|---|
| `preview_plugin_package` | Reads a `.ocplugin` zip and returns display data for the consent dialog; nothing is written |
| `install_plugin_package` | Installs a `.ocplugin` from a local path; caches all static assets at install time |
| `install_plugin_from_url` | Downloads a `.ocplugin` from an HTTPS URL, then installs it |
| `verify_plugin_installation` | Re-checks binary SHA-256 against the manifest |
| `set_plugin_enabled` | Enables or disables a plugin without removing it |
| `remove_plugin` | Deletes the plugin and all cached data |
| `unquarantine_plugin` | Restores a quarantined plugin and resets trust status |

### Discovery (served from SQLite cache)

| Tauri command | Returns |
|---|---|
| `list_installed_plugins` | All installed plugins with status |
| `inspect_plugin_protocol` | Protocol info from manifest (no binary invocation) |
| `load_plugin_identity` | Identity fields from manifest |
| `load_plugin_endpoints` | Endpoint list with summaries from cached `help/<cmd>.json` |
| `load_plugin_help` | Top-level help from cached `help/<cmd>.json` |
| `load_plugin_endpoint_help` | Per-endpoint help from cached `help/<endpoint>.json` |
| `load_plugin_templates` | Example templates from cached `examples/<cmd>.json` |
| `load_plugin_template` | Single template by id |
| `search_plugin_endpoints` | Full-text search across all installed plugin endpoints |
| `validate_plugin_command` | Invokes the binary `api validate` endpoint; checks a params object against the command schema |
| `run_plugin_self_test` | Invokes the binary `api self-test` endpoint; returns live health/dependency check results |

### Registry

| Tauri command | Returns |
|---|---|
| `fetch_plugin_registry` | Full verified registry manifest (`FetchRegistryResult` with `generated_at` and `plugins[]`) |
| `check_plugin_updates` | List of installed plugins (`PluginUpdateInfo[]`) for which the registry has a newer version |

## Vocabulary mapping

| Manifest / static asset term | Host UI term | Notes |
|---|---|---|
| `commands[]` | `endpoints` | Each command becomes an `EndpointDescriptor` |
| `help/<cmd>.json` | `help` / `endpoint_help` | Field-level help per command |
| `examples/<cmd>.json` | `templates` | Each example becomes a `TemplateSummary` |
| `manifest.display_name` + identity fields | `PluginIdentity` | Read from manifest at install |
| `manifest.protocol.supported_versions` | `ProtocolDescriptor` | `app_aware` is always true for packaged plugins |
