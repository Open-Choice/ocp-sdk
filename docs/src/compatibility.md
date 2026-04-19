# Compatibility Policy

Open Choice uses three independent version axes. They evolve separately and must not be conflated.

## Version axes

| Axis | Where declared | Current |
|---|---|---|
| Tool version | `manifest.json` → `version` field | Per-plugin (e.g. `1.1.1`) |
| Protocol version | `manifest.json` → `protocol.supported_versions[]` | `ocp-json/1` |
| `.oce` schema version | `schema_version` field in `.oce` files | `1` |

## Compatibility guarantees

- **ocp-json/1** is the only supported protocol version in this codebase. A future breaking change would introduce `ocp-json/2`.
- **`.oce` schema 1** is the only supported schema. Parsers must reject files with unknown `schema_version` values.
- **Tool versions** follow semver. The host does not enforce tool version constraints beyond what the `.oce` file pins.

## Host / plugin compatibility

The host declares which protocol versions it supports. A plugin declares which protocol versions it supports via `manifest.json` → `protocol.supported_versions[]`. The host must refuse to install or run a plugin if the intersection of supported protocol versions is empty.

A plugin may support multiple protocol versions (e.g. `["ocp-json/1", "ocp-json/2"]`). The host selects the highest mutually supported version.

## Self-reporting

Each plugin self-reports protocol compatibility through its manifest:

- `manifest.protocol.supported_versions` — list of supported protocol version strings
- `manifest.version` — the plugin's own release version (semver)
- `manifest.commands[]` — the commands this plugin exposes

Packaged plugins (`.ocplugin` format) are always treated as `app_aware: true` — they include all static inspection assets in the zip.
