# ocp-json/1 — Capability Registry

This document is **normative**. It enumerates the standard capability flags defined by `ocp-json/1` and specifies the rules for vendor-defined capabilities. It builds on `wire-format-1.md`, `envelope-1.md`, and `kinds-1.md`.

The standard capability set is **frozen** as of `ocp-types-1.0.0`. New standard capabilities may be added in `1.x` minor releases (additive only); no existing standard capability may be renamed, removed, or have its semantics changed. See `CONTRIBUTING.md` for the governance rules.

## 1. Purpose of capabilities

Capabilities are the negotiation surface between hosts and plugins. They exist to permit `ocp-json/1` to grow without breaking older participants:

- A **plugin** advertises which optional protocol features it supports.
- A **host** reads the advertised set, decides which features to use, and adapts its dispatch accordingly.
- Anything that is not in the advertised set MUST be assumed unsupported. The host MUST NOT use unannounced features against a plugin, and the plugin MUST NOT emit unannounced features to the host.

A capability is therefore a **contract**: by listing it, the plugin commits to a specific behavioral guarantee documented in this file. Hosts can rely on those guarantees without further negotiation.

## 2. Where capabilities live

Per `ocp-json-1.md`, the discovery surface for `ocp-json/1` is the static `manifest.json` shipped inside the `.ocplugin` package, not a live binary endpoint.

- The plugin's `manifest.json` MUST contain a top-level `capabilities` array of capability identifiers (strings).
- The host reads `capabilities` at install time and at every load, before invoking any binary endpoint.
- The plugin binary MUST behave consistently with its declared capability set. Declaring a capability and then not honoring it is a protocol violation that fails the conformance suite.
- A plugin MAY advertise a strict superset of what it actually uses (it is permitted to claim a capability it never exercises), but MUST NOT exercise a capability it does not advertise.

The wire format does NOT carry a capability list inside any envelope. Capabilities are part of the static contract, not the runtime stream. A future `ocp-json/2` may introduce live capability negotiation; `ocp-json/1` deliberately does not.

## 3. Capability identifier grammar

A capability identifier is a dot-separated namespaced string of the form:

```
<namespace>.<feature>+
```

where:

- `<namespace>` is a short lowercase ASCII word, or a vendor identifier.
- `<feature>` is one or more dot-separated path segments.
- Each segment matches `^[a-z][a-z0-9_]*$` (lowercase ASCII, underscores allowed).
- Total length MUST NOT exceed 128 bytes.

Reserved standard namespaces (defined in this document) are listed in §4. Any other first segment is available for vendor use, subject to the vendor identifier grammar from `envelope-1.md` §6.2: `^[a-z][a-z0-9-]*$`. Vendor identifiers MUST NOT collide with reserved standard namespaces.

## 4. Reserved standard namespaces

The following first segments are **reserved by `ocp-json/1`** and MUST NOT be used as vendor identifiers:

- `control` — host-to-plugin control channel features
- `events` — optional event kinds the plugin may emit
- `composition` — wrapper / multi-plugin orchestration features
- `restart` — checkpoint and resume features
- `bundled` — bundled-component features
- `stdin` — stdin transport features
- `outputs` — optional output kinds the plugin produces
- `validation` — optional validation behaviors
- `params` — optional parameter handling behaviors

Each is described below. A capability not present in the plugin's manifest is, by definition, unsupported.

## 5. Standard capabilities

### 5.1 `control.*` — control channel features

| Capability | Implies | Host guarantee | Plugin obligation |
|---|---|---|---|
| `control.cancel` | Plugin accepts `control.cancel` envelopes during `run` | Host MAY send `control.cancel` to terminate a run cleanly | Plugin MUST emit `event.run.cancelled` as the terminal event and exit with status 0 (see `kinds-1.md` §6.1) |
| `control.pause` | Plugin accepts `control.pause` and `control.resume` envelopes | Host MAY send `control.pause` to suspend a run, then `control.resume` to continue | Plugin MUST emit `event.run.paused` on suspend and `event.run.resumed` on resume; MUST NOT advance work between the two |
| `control.deadline` | Plugin honors `control.deadline.extend` envelopes | Host MAY push back a soft deadline mid-run | Plugin SHOULD treat the new deadline as advisory; failure to meet it does not require any specific terminal event |

If `control.cancel` is absent, hosts MUST NOT send `control.cancel`. The only termination paths available are letting the run complete or sending OS-level signals (SIGTERM/SIGKILL) — neither of which is governed by this protocol.

If `control.pause` is absent, hosts MUST NOT send `control.pause` or `control.resume`. A plugin that supports pausing but not resuming is malformed; the two are bundled into a single capability.

`control.cancel` is **strongly recommended** for any plugin whose runs may exceed a few seconds. Hosts MAY warn users when invoking long-running plugins that lack `control.cancel`.

### 5.2 `events.*` — optional event kinds

| Capability | Implies | What it permits |
|---|---|---|
| `events.heartbeat` | Plugin may emit `event.run.heartbeat` | Host SHOULD treat heartbeat events as liveness pings and reset its idle-timeout clock |
| `events.progress` | Plugin may emit `event.run.progress` | Host SHOULD render progress information (iteration, phase, metrics) to the user |
| `events.metric` | Plugin may emit standalone `event.metric` envelopes | Host SHOULD route metric envelopes to its time-series UI / log |
| `events.log_line` | Plugin may emit `event.log.line` for structured logging | Host SHOULD render structured log lines in a filterable log view, in addition to (or instead of) plain stderr |
| `events.artifact_updates` | Plugin may emit `event.artifact.updated` after the initial `event.artifact.created` | Host MUST track artifact mutations by `artifact_id`, not just creations |
| `events.stage` | Plugin may emit `event.stage.started` and `event.stage.finished` | Host MAY render a per-stage UI; this capability is restricted to composition wrappers |

Hosts MUST tolerate the absence of any optional event capability. A plugin that does not advertise `events.progress` simply provides no progress feedback; the host MUST NOT treat its absence as an error.

A plugin that advertises `events.stage` but is not also a composition wrapper (i.e., does not advertise `composition.*`) is malformed.

### 5.3 `composition.*` — wrapper / orchestration features

| Capability | Implies | Behavior |
|---|---|---|
| `composition.wrapper` | The plugin internally invokes other plugins | Host SHOULD expect `event.stage.*` events and a populated `RunContext.run_chain` field on relayed events |
| `composition.nested` | The plugin's children may themselves be composition wrappers | Host SHOULD render `run_chain` as a tree of arbitrary depth, not a flat list of two |
| `composition.parallel` | The plugin may run multiple child plugins concurrently | Host SHOULD expect interleaved events from multiple stages and dispatch by `RunContext.stage_id` |
| `composition.relay` | The plugin forwards child events upstream rather than aggregating | Host SHOULD render relayed events with the originating tool clearly indicated; see `envelope-1.md` for `RunContext.originating_tool` |

A non-wrapper plugin MUST NOT advertise any `composition.*` capability. A wrapper plugin SHOULD advertise the most specific subset that describes its actual behavior, not the full set.

`composition.wrapper` is the base; the others are refinements. A plugin that advertises `composition.parallel` is implicitly also a `composition.wrapper`, but the explicit base capability MUST also be listed.

### 5.4 `restart.*` — checkpoint and resume features

| Capability | Implies | Guarantee |
|---|---|---|
| `restart.exact` | Plugin can resume from a checkpoint and produce bit-identical subsequent output | Host MAY persist checkpoint artifacts and restart the plugin from them with full reproducibility |
| `restart.approximate` | Plugin can resume from a checkpoint and produce statistically equivalent (but not bit-identical) output | Host MAY restart from a checkpoint but MUST NOT assume bit-identical results across resume |

These two capabilities are **mutually exclusive**. A plugin that advertises both is malformed. A plugin that advertises neither MUST NOT emit `event.checkpoint.committed` and MUST NOT accept any restart-from-checkpoint invocation.

When `restart.exact` or `restart.approximate` is set, the plugin:

1. MAY emit `event.checkpoint.committed` envelopes during `run`.
2. MUST honor a `--from-checkpoint <path>` flag on its `run` invocation (the exact flag spelling is part of `ocp-json-1.md`).
3. MUST emit `event.run.resumed` as one of its first events when starting from a checkpoint.

### 5.5 `bundled.*` — bundled-component features

| Capability | Implies | Behavior |
|---|---|---|
| `bundled.components` | The plugin ships other plugin binaries inside its `.ocplugin` package and invokes them as subprocesses | Host SHOULD treat bundled components as opaque dependencies; they are NOT separately discoverable, NOT separately versioned by the host, and NOT subject to independent trust decisions |
| `bundled.components.attested` | Same as `bundled.components`, but the wrapping plugin's manifest enumerates each bundled component with its own SHA-256 digest | Host MAY surface the bundled component list to the user for transparency, and MAY reject the package if any bundled component matches a separately blocked plugin hash |

`bundled.components.attested` is the recommended form. Plain `bundled.components` is permitted for legacy or internal-only plugins where attestation overhead is not justified.

A wrapper plugin that uses bundled components MUST advertise one of these capabilities. A plugin that does not advertise either MUST NOT execute any binary other than its own from inside its installation directory.

### 5.6 `stdin.*` — stdin transport features

| Capability | Implies | Host obligation |
|---|---|---|
| `stdin.control_channel` | Plugin reads NDJSON `control.*` envelopes from stdin during `run` | Host MUST keep stdin open for the duration of the run and MAY write `control.*` envelopes to it |

If absent, the host MUST close the plugin's stdin immediately after spawn (per `wire-format-1.md` §7.3). This capability is a prerequisite for any `control.*` capability; a plugin that advertises `control.cancel` MUST also advertise `stdin.control_channel`.

### 5.7 `outputs.*` — optional output behaviors

| Capability | Implies | Behavior |
|---|---|---|
| `outputs.streaming` | The plugin may emit `event.artifact.created` for an artifact whose file is still being written | Host MUST tolerate `PathRef` targets that exist but are not yet complete; SHOULD wait for `event.artifact.updated` or `event.run.finished` before consuming |
| `outputs.content_addressed` | The plugin uses `PathRef::ContentAddressed` variants for some outputs | Host MUST resolve content-addressed paths via the configured CAS store before reading |
| `outputs.deterministic` | Repeated runs with the same seed and inputs produce byte-identical output files | Host MAY use this as a cache key for memoization |

These are independent and may be combined. A plugin that advertises none of them produces only on-disk, fully-written, possibly-non-deterministic local files — the default behavior.

### 5.8 `validation.*` — optional validation behaviors

| Capability | Implies | Behavior |
|---|---|---|
| `validation.dry_run` | The plugin's `api validate` performs full input parsing and schema validation, not just shape checking | Host MAY rely on a passing `api validate` as a strong signal that `run` will not fail for input reasons |
| `validation.cost_estimate` | The plugin's `ValidateResponsePayload` includes a `cost_estimate` field with anticipated runtime / memory / cost | Host MAY display the estimate to the user before launching the run |
| `validation.warnings` | The plugin's `ValidateResponsePayload` may contain non-blocking warnings in addition to errors | Host SHOULD render warnings distinctly from errors and SHOULD NOT block run launch on warnings alone |

If `validation.dry_run` is absent, hosts MUST treat `api validate` as a best-effort shape check and MUST NOT rely on it to prevent `run` failures.

### 5.9 `params.*` — optional parameter handling behaviors

| Capability | Implies | Behavior |
|---|---|---|
| `params.normalization` | The plugin emits a `params.json` artifact with the normalized parameter set actually used during the run | Host MAY use this for run reproducibility and audit logging |
| `params.echo` | The plugin emits an `input.echo` artifact with the original input verbatim | Host MAY use this for full provenance reconstruction |
| `params.defaults_documented` | The plugin's static `examples/` and `help/` assets describe every parameter's default value | Host UI MAY surface defaults in form-rendering without inferring them from schema |

These are independent and additive. A plugin that advertises none of them is permitted; the host simply offers fewer reproducibility / UI features for it.

## 6. Capability dependency rules

Some capabilities imply other capabilities. The plugin's manifest MUST list every capability it relies on, including transitive ones — the host is not required to compute the closure.

| Capability | Requires |
|---|---|
| `control.cancel` | `stdin.control_channel` |
| `control.pause` | `stdin.control_channel` |
| `control.deadline` | `stdin.control_channel` |
| `events.stage` | At least one `composition.*` capability |
| `composition.parallel` | `composition.wrapper` |
| `composition.nested` | `composition.wrapper` |
| `composition.relay` | `composition.wrapper` |
| `bundled.components.attested` | `bundled.components` |

Hosts MUST validate the dependency closure at load time and SHOULD reject any plugin whose manifest is internally inconsistent (e.g., `control.cancel` without `stdin.control_channel`).

## 7. Vendor-defined capabilities

Vendors MAY define their own capabilities under their reserved namespace:

```
<vendor>.<feature>
```

### 7.1 Examples

Hypothetical wrapper plugin capabilities:

```
maxdiff-pipeline.curve_family_v2
maxdiff-pipeline.bundled_design_engine
```

Hypothetical HB plugin capabilities:

```
numerious-hb.gpu_acceleration
numerious-hb.distributed_chains
```

### 7.2 Rules for vendor capabilities

1. Vendor capabilities MUST follow the grammar in §3.
2. Vendor identifiers MUST NOT collide with the reserved namespaces in §4.
3. Vendor capabilities SHOULD NOT duplicate the semantics of standard capabilities. If a vendor needs to advertise progress support, it SHOULD use `events.progress`, not invent `<vendor>.progress`.
4. Hosts that don't recognize a vendor capability MUST silently ignore it. Unknown capabilities MUST NOT cause load failure.
5. Vendor capabilities are NOT registered in this document. Vendors MAY publish their own capability documentation alongside their plugins.

### 7.3 Promotion path

If a vendor capability proves broadly useful, it MAY be promoted to a standard capability in a future `ocp-types-1.x` minor release. Promotion follows the same rules as `ext` field promotion (envelope spec §6.5) and kind promotion (kinds spec §7.3):

1. The standard capability is added to this document.
2. The vendor capability is **deprecated but still accepted**. Plugins SHOULD migrate within one minor release.
3. The vendor capability MAY remain registered indefinitely as an alias to preserve forward compatibility. Hosts MUST treat the vendor alias and the standard name as equivalent.

## 8. Discovery and negotiation flow

The end-to-end flow at plugin load time:

1. Host opens the `.ocplugin` package and reads `manifest.json`.
2. Host reads the `capabilities` array from the manifest.
3. Host validates the capability dependency closure (§6). If inconsistent, load fails.
4. Host validates that no capability identifier violates the grammar (§3). Unknown identifiers are tolerated; malformed identifiers are not.
5. Host stores the capability set alongside the plugin's identity record for the lifetime of the install.
6. On every subsequent invocation (`api validate`, `api self-test`, `run`), the host consults the stored capability set to decide:
   - Whether to keep stdin open (`stdin.control_channel`)
   - Which `control.*` envelopes are valid to send
   - Which event kinds to expect
   - Whether to render stage UI, progress bars, structured logs, etc.
7. The plugin binary is NOT consulted for capability information at runtime. The static manifest is authoritative.

A consequence of this flow: changing a plugin's capabilities requires shipping a new `.ocplugin` package with an updated manifest. Capabilities cannot be toggled per-invocation.

## 9. Conformance

A producer claims capability conformance for `ocp-json/1` if:

1. Its `manifest.json` `capabilities` array contains only well-formed identifiers per §3.
2. Every standard capability it lists is honored at runtime per the guarantees in §5.
3. Its capability list is dependency-closed per §6.
4. It does NOT exercise any optional protocol feature whose capability it has not advertised.

A consumer claims capability conformance if:

1. It correctly parses the `capabilities` array from every `manifest.json` it loads.
2. It validates the dependency closure and rejects internally inconsistent manifests.
3. It tolerates unknown capability identifiers (preserves them but does not act on them).
4. It does NOT use any optional protocol feature against a plugin that has not advertised the corresponding capability.

The `ocp-conformance` test corpus contains fixtures for every standard capability and a representative sample of unknown-capability handling cases.

## 10. Forbidden changes (governance reference)

The following changes to this document are **forbidden** once `ocp-types-1.0.0` is tagged:

1. Removing any standard capability listed in §5.
2. Renaming any standard capability.
3. Changing the behavioral guarantee of any standard capability in a way that would invalidate previously-conforming plugins or hosts.
4. Removing any reserved namespace from §4.
5. Changing the capability identifier grammar in §3.
6. Adding a new dependency edge to §6 that would retroactively invalidate any previously-conforming manifest.
7. Reserving a previously-unreserved namespace if doing so would collide with any known vendor identifier in active use.
8. Moving capability discovery from the static manifest to a live binary endpoint (this is a v2 change, not a v1 change).

Permitted changes are:

1. Adding new standard capabilities to §5 (additive).
2. Adding new reserved namespaces to §4, provided they do not collide with known vendor identifiers.
3. Tightening documentation (e.g., specifying a previously-unspecified guarantee).
4. Promoting a vendor capability to a standard capability (with the original vendor name remaining as an accepted alias).
5. Adding new dependency edges to §6 only when the dependent capability is itself newly added in the same release.
