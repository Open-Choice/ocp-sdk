# ocp-sdk

Rust SDK for the Open Choice Protocol (`ocp-json/1`). This repo defines the shared contract between plugin executables and the Open Choice host application.

## Crates

| Crate | Published as | Purpose |
|---|---|---|
| `ocp-types` | `ocp-types` | Protocol types, runtime event envelopes, registry types, trust status types |
| `oc-core` | `oc-core` | `.oce` task file parser; resolves aliases to plugin IDs, produces task JSON |
| `ocp-host` | `ocp-host` | Executable runner, `ToolProtocolClient` (all `api *` subcommands), exe-discovery registry |
| `oc-sign` | `oc-sign` | Developer CLI: `keygen`, `sign`, `pack` — produces `.ocplugin` zip archives |

## Who needs what

**Plugin authors** only need `ocp-types` — for the protocol types their binary serializes and deserializes.

**Host implementations** need `ocp-host` and `oc-core`, which pull in `ocp-types` transitively.

**Packaging** — install `oc-sign` once via `cargo install oc-sign` to sign and pack plugins into `.ocplugin` archives.

## Documentation

Full protocol specifications are in `docs/` and published to GitHub Pages via mdBook:

- ocp-json/1 protocol
- `.oce` file format
- Registry manifest format
- Trust model

## Development

Clone alongside plugin repos and the app so the `[patch.crates-io]` path overrides in each repo resolve correctly:

```
Desktop/git_projects/
  ocp-sdk/                ← this repo
  plugin-toy-calculator/
  plugin-julia-wrapper/
  plugin-rscript-wrapper/
```

```sh
cargo check --workspace
cargo test --workspace
```

## License

MIT — see [LICENSE](LICENSE) for the full text.

Copyright (c) 2026 The Open Choice Authors
