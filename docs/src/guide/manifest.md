# Manifest reference

`packaging/manifest.json` describes your plugin to the Open Choice host. It is verified at install time and cached in the host's database.

The canonical type for the manifest is [`ocp_types_v1::manifest::Manifest`](https://docs.rs/ocp-types-v1). Everything below matches that type field-for-field — the v1 crate is the source of truth.

## Full example

```json
{
  "schema_version": "1",
  "plugin_id": "com.example.my-echo-plugin",
  "display_name": "My Echo Plugin",
  "version": "0.1.0",
  "publisher": "Your Name",
  "description": "Echoes a message to a text file.",
  "runtime": {
    "type": "native-sidecar",
    "entrypoints": [
      {
        "os": "windows",
        "arch": "x86_64",
        "path": "bin/windows-x86_64/my-echo-plugin.exe",
        "digest": { "algorithm": "sha256", "value": "__PLACEHOLDER__" }
      },
      {
        "os": "macos",
        "arch": "aarch64",
        "path": "bin/macos-aarch64/my-echo-plugin",
        "digest": { "algorithm": "sha256", "value": "__PLACEHOLDER__" }
      }
    ]
  },
  "protocol": { "family": "ocp-json", "version": "1" },
  "commands": ["echo"],
  "capabilities": [
    "events.progress",
    "control.cancel",
    "stdin.control_channel"
  ],
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

---

## Top-level fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `schema_version` | string | yes | Always `"1"` for `ocp-json/1`. |
| `plugin_id` | string | yes | Globally unique identifier. Reverse-DNS format recommended. |
| `display_name` | string | yes | Human-readable name shown in the UI. |
| `version` | string | yes | Semantic version: `"1.0.0"`. |
| `publisher` | string | no | Your name or organization. |
| `description` | string | no | One or two sentences shown in the plugin browser. |
| `runtime` | object | yes | Native-sidecar entrypoints. See [§ runtime](#runtime). |
| `protocol` | object | yes | Protocol family and version. Fixed: `{ family: "ocp-json", version: "1" }`. |
| `commands` | string[] | yes | Command names this plugin implements. At least one. |
| `capabilities` | string[] | no | Standard capability identifiers. See [§ capabilities](#capabilities). |
| `sandbox` | object | no | Filesystem and network declarations. See [§ sandbox](#sandbox). |
| `signing` | object | no | Signing metadata. See [§ signing](#signing). |
| `bundled_components` | array | no | Only if you declare `bundled.components.attested`. See [§ bundled components](#bundled-components). |
| `ext` | object | no | Vendor extension bag. Keys must be `<vendor>.<field>`. |

Unknown top-level fields are preserved verbatim on round-trip, so adding future fields doesn't break existing tooling.

### `plugin_id` naming

- Use reverse-DNS format: `com.yourname.plugin-name`.
- Lowercase letters, digits, hyphens, and dots only.
- The last segment (after the final `.`) becomes the **implied alias** users can reference in `.oce` files without defining an alias first.

---

## `runtime`

```json
"runtime": {
  "type": "native-sidecar",
  "entrypoints": [ /* one per OS/arch */ ]
}
```

`type` must be `"native-sidecar"` — the only supported runtime in v1.

Each entry in `entrypoints`:

| Field | Type | Description |
|-------|------|-------------|
| `os` | string | `"windows"` \| `"macos"` \| `"linux"`. |
| `arch` | string | `"x86_64"` \| `"aarch64"`. |
| `path` | string | Path inside the `.ocplugin` zip, forward slashes only. |
| `digest` | object | `ContentDigest`: `{ "algorithm": "sha256", "value": "<64-char hex>" }`. `oc-sign pack` fills in the value. |

Supply one entry per OS/arch combination you support. The host picks the match for the current platform at install time and refuses to install if none match.

The `digest` is carried as a tagged object (not a bare `"sha256": "..."` string) so that future minor releases can introduce a new hash algorithm without breaking existing consumers. All shipping plugins use `"algorithm": "sha256"`.

---

## `protocol`

```json
"protocol": { "family": "ocp-json", "version": "1" }
```

Both fields are fixed. Do not change them.

---

## `capabilities`

An array of capability identifier strings declaring which optional features of the `ocp-json/1` protocol your plugin implements. See `capabilities-1.md` for the full registry; the most common ones are:

| Capability | Meaning |
|------------|---------|
| `events.progress` | You emit `event.run.progress` envelopes. |
| `events.heartbeat` | You emit `event.run.heartbeat` envelopes during quiet phases. |
| `events.artifact_updates` | You emit `event.artifact.updated` envelopes (not just `.created`). |
| `events.log_line` | You emit structured `event.log.line` envelopes. |
| `events.metric` | You emit standalone `event.metric` envelopes. |
| `events.stage` | You emit `event.stage.started` / `event.stage.finished` envelopes. |
| `control.cancel` | You respond to `control.cancel` on stdin by terminating with `event.run.cancelled`. |
| `control.pause` | You support `control.pause` / `control.resume`. |
| `control.deadline` | You respect a deadline provided via `control.deadline`. |
| `stdin.control_channel` | You read NDJSON control envelopes from stdin. Required for any `control.*` capability. |
| `composition.wrapper` | You invoke other plugins as child runs. |
| `composition.relay` | You relay a child's events upstream under your own run context. |
| `restart.exact` | You support exact bit-identical restart from a checkpoint. |
| `restart.approximate` | You support approximate restart from a checkpoint. |
| `bundled.components` | You ship additional signed binaries in the same package. |
| `bundled.components.attested` | Same, with per-component digests in `bundled_components`. |
| `outputs.deterministic` | Re-running with the same seed produces identical artifacts. |
| `outputs.content_addressed` | You write artifacts to content-addressed paths. |
| `outputs.streaming` | You emit artifact envelopes incrementally as files are written. |
| `validation.dry_run` | `api validate` runs enough of the work to estimate feasibility. |
| `validation.warnings` | `api validate` may return non-empty `issues` with `severity: warning`. |
| `params.normalization` | `api validate` returns `normalized_params` different from the input. |

Capabilities have dependency rules — for example, `control.cancel` implies `stdin.control_channel` — and the host validates the closure at install time. You may also declare vendor-namespaced capabilities (`<vendor>.<feature>`) for features outside the standard registry; the host shows them in the consent dialog but doesn't grant any special permissions based on them.

The constants live in `ocp_types_v1::capabilities::standard` if you want them from Rust.

---

## `sandbox`

Declares what system resources your plugin needs. The host shows these in the install consent dialog; they are metadata for the consent flow, not runtime enforcement.

```json
"sandbox": {
  "fs_read":  [],
  "fs_write": ["plugin-workdir"],
  "network":  false
}
```

| Field | Type | Description |
|-------|------|-------------|
| `fs_read` | string[] | Filesystem scopes the plugin needs to read. |
| `fs_write` | string[] | Filesystem scopes the plugin needs to write. |
| `network` | bool | `true` if the plugin makes outbound network requests. |

### Filesystem scopes

| Scope | Meaning |
|-------|---------|
| `"plugin-workdir"` | The plugin's own working directory only. |
| `"script-dir"` | The directory containing the script being run. |
| `"anywhere"` | Unrestricted filesystem access. |

Most plugins use `"fs_write": ["plugin-workdir"]`. Wrappers that produce output relative to user-chosen paths need `"anywhere"`.

Omit `sandbox` entirely for plugins that don't need any filesystem or network access beyond the implicit runtime needs.

---

## `signing`

```json
"signing": {
  "key_id": "my-key-2026",
  "signature_path": "signatures/manifest.sig",
  "algorithm": "ed25519"
}
```

| Field | Description |
|-------|-------------|
| `key_id` | ID of the signing key, registered in the host's `trusted_keys.json`. |
| `signature_path` | Path inside the `.ocplugin` zip where `oc-sign pack` writes the signature. |
| `algorithm` | Signing algorithm. Currently always `"ed25519"`. |

Required for installation on a locked-down host. Unsigned packages can only be installed in developer mode and receive `trust_status = "warning"`.

---

## `bundled_components`

Only populated when the plugin declares `bundled.components.attested`. Each entry describes an additional signed binary shipped in the same `.ocplugin` package:

```json
"bundled_components": [
  {
    "component_id": "design-maxdiff",
    "display_name": "Design MaxDiff",
    "version": "0.5.2",
    "path": "components/design-maxdiff/design-maxdiff.exe",
    "digest": { "algorithm": "sha256", "value": "c3..." }
  }
]
```

The host verifies each component's digest the same way it verifies the main entrypoint.

---

## Forward compatibility

Every manifest type in `ocp-types-v1` carries an `other` slot that preserves unknown top-level fields. This means:

- Future minor releases can add optional fields without breaking installed plugins.
- Tools that read manifests should round-trip them verbatim rather than re-serializing only the known subset.
- Vendor extensions belong in `ext` (namespaced by vendor), never as ad-hoc top-level fields.
