# Contributing to ocp-sdk

This document is the **governance contract** for `ocp-json/1` and the `ocp-types-v1` crate. Its purpose is to make the rules for changing the protocol explicit, durable, and resistant to drift.

The single most important rule:

> **`ocp-json/1` is designed to never change.** Once `ocp-types-v1-1.0.0` is tagged, the wire format, envelope shape, kind grammar, and capability mechanism are frozen. Additive evolution within `1.x` is permitted under tightly bounded rules. Anything that violates those rules requires `ocp-json/2`, which is a parallel protocol with a parallel crate, not a release of this one.

If a proposed change cannot be made additively, the answer is not "let's break compatibility just this once." The answer is "this is a v2 change; defer it."

## 1. Scope of this document

This document governs:

- The five normative spec files in `docs/src/protocol/`:
  - `wire-format-1.md`
  - `envelope-1.md`
  - `kinds-1.md`
  - `capabilities-1.md`
  - `ocp-json-1.md` (the umbrella overview)
- The `ocp-types-v1` crate (the canonical Rust implementation of `ocp-json/1`).
- The `ocp-conformance` crate (the operative test corpus).

It does not govern:

- Individual plugins (each plugin has its own contribution rules in its own repo).
- The host application (the Open Choice desktop app).
- Vendor extensions (`ext` keys, vendor-defined kinds, vendor-defined capabilities).

## 2. The freeze rule

`ocp-types-v1-1.0.0` is the freeze point. Before that tag, anything in the spec or crate may change. After that tag, the rules in §§3–6 apply for the entire lifetime of `1.x`.

The freeze rule applies to:

- Every byte the producer puts on the wire.
- Every parser invariant the consumer relies on.
- Every public type, field, and serde representation in `ocp-types-v1`.
- Every behavioral guarantee documented in the spec files.

The freeze rule does NOT apply to:

- Private implementation details inside `ocp-types-v1` that don't affect serialization.
- Helper functions, builder APIs, error types, and other ergonomics.
- Internal refactors that preserve the wire format and the public type surface.
- The conformance suite itself, which may grow as long as new tests don't reject previously-conforming inputs.

## 3. Forbidden changes

These changes are forbidden in `1.x`. Each is also reiterated in the relevant spec file. If a change is forbidden by any of the five spec files, it is forbidden here.

### 3.1 Wire format (`wire-format-1.md` §14)

1. Changing the encoding from UTF-8.
2. Changing the line terminator from LF.
3. Adding any required field name beginning with `_` or `x_`.
4. Removing the rule that consumers MUST preserve unknown fields.
5. Changing the wire format of `Timestamp`, `Duration`, `Identifier`, or `ContentDigest`.
6. Changing the `ocp` field's value or type.
7. Tightening any `MAY` or `SHOULD` to a `MUST` in a way that would invalidate previously-conforming implementations.
8. Loosening any `MUST` in a way that would permit previously-rejected input.
9. Changing the safe-integer range or the float-encoding requirement.
10. Changing the grammar of any identifier type.

### 3.2 Envelope (`envelope-1.md`)

1. Adding any new required top-level field to the envelope.
2. Removing or renaming any existing required top-level field.
3. Changing the set of envelope classes (`event`, `response`, `request`, `control`).
4. Changing the `RunContext` field set in any way that invalidates previously-conforming envelopes.
5. Changing the `Ext` extension bag mechanism.
6. Changing any wrapped primitive type's tag discriminant.

### 3.3 Kinds (`kinds-1.md` §10)

1. Removing any standard kind.
2. Renaming any standard kind.
3. Changing the payload schema of any standard kind in a way that would invalidate previously-conforming producers.
4. Removing any reserved segment.
5. Changing the kind grammar.
6. Reserving a previously-unreserved segment if doing so would collide with any known vendor identifier in active use.

### 3.4 Capabilities (`capabilities-1.md` §10)

1. Removing any standard capability.
2. Renaming any standard capability.
3. Changing the behavioral guarantee of any standard capability in a way that would invalidate previously-conforming plugins or hosts.
4. Removing any reserved namespace.
5. Changing the capability identifier grammar.
6. Adding a new dependency edge that would retroactively invalidate any previously-conforming manifest.
7. Reserving a previously-unreserved namespace if doing so would collide with any known vendor identifier in active use.
8. Moving capability discovery from the static manifest to a live binary endpoint.

### 3.5 Crate-level rules (`ocp-types-v1`)

1. Removing any public type, variant, field, or trait impl from the crate's public surface.
2. Renaming any public type, variant, or field that participates in serde.
3. Changing the serde representation of any public type (e.g., switching `#[serde(tag = "...")]` strategies).
4. Removing `#[serde(other)]` from any enum that has it.
5. Removing `#[serde(flatten)]` from any extension-bag field.
6. Adding `#[serde(deny_unknown_fields)]` to any wire type.
7. Changing the MSRV in a way that would prevent existing downstream consumers from compiling against the crate.

## 4. Permitted changes

These changes are permitted in `1.x`. They are the only ways the protocol may grow.

### 4.1 Additive spec changes

1. Adding new standard kinds to `kinds-1.md` §§3–6 (additive only).
2. Adding new standard output kinds to `kinds-1.md` §8.1 (additive only).
3. Adding new standard capabilities to `capabilities-1.md` §5 (additive only).
4. Adding new reserved namespaces to `capabilities-1.md` §4, provided they do not collide with any known vendor identifier in active use.
5. Adding new optional fields to existing payload schemas, where "optional" means: producers MAY emit the field, consumers MUST tolerate its absence, and the field is documented as optional.
6. Tightening documentation: clarifying ambiguous wording, specifying a previously-unspecified field as required *only when no producer in the wild emits it differently*, or adding examples.
7. Promoting a vendor extension (kind, capability, or `ext` field) to a standard one, provided the original vendor name remains accepted as an alias indefinitely.

### 4.2 Additive crate changes

1. Adding new public types, variants, or fields to existing types, provided they have serde defaults that round-trip cleanly with older serialized forms.
2. Adding new modules.
3. Adding new builder methods, helper functions, and ergonomic APIs.
4. Adding new trait impls (where they do not conflict with existing user code).
5. Bumping dependencies that do not affect the public type surface.
6. Internal refactors that do not affect serde output.

### 4.3 Conformance suite changes

1. Adding new test fixtures for newly-added standard kinds, capabilities, or fields.
2. Adding new test fixtures for previously-untested edge cases, provided the new tests reject inputs that the spec already forbids.
3. Adding new round-trip tests for existing types.
4. Adding new forward-compatibility tests (unknown-field preservation, unknown-kind handling, etc.).

The conformance suite MUST NOT be changed in a way that retroactively rejects an input that was previously accepted by both the spec and the suite. If an existing fixture is found to be wrong, the spec is the authority, and the fix is to remove or correct the fixture, not to change the rule.

## 5. The PR process

Every change to the spec files or the `ocp-types-v1` crate goes through the following process. There are no exceptions, including for the maintainer.

### 5.1 Classify the change

Before opening a PR, the author MUST classify the change:

- **Editorial**: typos, formatting, link fixes, clarifying wording that does not change semantics. No conformance impact.
- **Additive**: adds a new optional field, kind, capability, or type. Permitted under §4.
- **Tightening**: turns a `MAY` or `SHOULD` into a `MUST`, or restricts a previously-broad behavior. Permitted only when no producer in the wild does it differently. Requires evidence.
- **Breaking**: anything that would cause a previously-conforming producer or consumer to become non-conforming. **Forbidden** in `1.x`. Must be deferred to v2.

The PR description MUST state the classification explicitly. PRs that fail to classify are returned without review.

### 5.2 Spec changes

A spec change PR MUST:

1. Update the relevant `docs/src/protocol/*.md` file(s).
2. If the change is additive, update the conformance suite with at least one new fixture exercising the addition.
3. If the change touches anything in §3 (forbidden) above, the PR MUST be closed without merging.
4. Pass the conformance suite against the previous tagged release of `ocp-types-v1`. This proves the change is backward-compatible.

### 5.3 Crate changes

A crate change PR MUST:

1. Pass `cargo test --workspace`.
2. Pass the full conformance suite.
3. Pass `cargo semver-checks` (or equivalent) against the previously published version on crates.io.
4. If the change adds a new public type or field, include round-trip tests demonstrating forward and backward compatibility.

### 5.4 Conformance suite changes

A conformance suite change PR MUST:

1. Demonstrate that all existing fixtures still pass.
2. Demonstrate that any new fixture is consistent with the spec (cite the relevant section).
3. Run against at least two reference plugins (currently `plugin-toy-calculator` and `plugin-python-wrapper`).

### 5.5 Review

All PRs require approval from a maintainer. The maintainer's responsibility is to verify:

- The classification is honest.
- The change does not appear in §3 under any reading.
- The conformance suite is updated.
- The cross-references between spec files remain consistent.

If a maintainer is uncertain whether a change is breaking, the default answer is "it is breaking; defer to v2." Erring toward freeze preserves the protocol's value.

## 6. Versioning and release cadence

### 6.1 Version numbers

`ocp-types-v1` uses semantic versioning, but the meaning is constrained by the freeze rule:

- **Major** (`2.0.0`): Reserved for `ocp-json/2`. NEVER bumped within `ocp-json/1`.
- **Minor** (`1.x.0`): Additive changes per §4. Released when there is meaningful additive content to ship.
- **Patch** (`1.x.y`): Editorial changes, internal refactors, dependency bumps, conformance suite expansion. Released as needed.

The `ocp` field in every envelope remains the literal string `"1"` for the entire lifetime of `1.x`. It is NEVER bumped to `"1.1"`, `"1.2"`, etc. Sub-protocol versioning is carried by the capability set, not by the `ocp` field.

### 6.2 Release cadence

There is no fixed release cadence. The crate is released when:

- A new standard kind, capability, or output kind has been added and is ready to ship.
- A new public type or builder API has been added.
- A meaningful conformance suite expansion has landed.
- A dependency security update is needed.

There is no expectation of monthly or quarterly releases. The protocol is designed to be stable; releases are events, not a schedule.

### 6.3 Tagging discipline

Every release is tagged in git with `ocp-types-v1-<version>` (e.g., `ocp-types-v1-1.0.0`, `ocp-types-v1-1.1.0`). Tags are immutable. If a release is broken, the fix is a new tag, not a force-push.

Pre-1.0 tags (`0.x.y`) are NOT bound by the freeze rule. The freeze rule applies starting at `1.0.0`.

## 7. Spec vs. implementation

The spec files are **explanatory**. The conformance suite is **operative**.

When the spec and the suite disagree:

1. The suite's behavior is what implementations must conform to.
2. A bug report is filed against the spec.
3. The spec is updated to match the suite, OR the suite is updated to match the spec, depending on which side reflects the original intent.
4. Either fix is itself a PR that goes through §5.

The `ocp-types-v1` crate is one implementation of `ocp-json/1`, not the definition of it. A producer or consumer written in another language is wire-conformant if and only if it passes the conformance suite, regardless of whether it uses `ocp-types-v1`.

This separation exists because the protocol is intended to outlast any single implementation. If `ocp-types-v1` is rewritten, replaced, or abandoned, the spec and the suite remain authoritative.

## 8. Backporting

`ocp-json/1` does not have multiple supported branches. There is one active line, `1.x`, and patches go to its tip.

If a critical bug is discovered in a tagged release, the fix:

1. Lands on the main branch.
2. Is included in the next minor or patch release.
3. Is NOT backported to a separate maintenance branch for an older minor version.

Consumers stuck on an older `1.x` minor version can upgrade freely; the freeze rule guarantees no breaking changes between minor releases.

## 9. The v2 escape hatch

There is one and only one circumstance under which the wire format may change incompatibly: shipping `ocp-json/2`.

`ocp-json/2` is a hypothetical future protocol. It is not currently planned. If and when it is needed, the rules are:

1. It lives in a parallel spec directory (`docs/src/protocol/v2/`) and a parallel crate (`ocp-types-v2`).
2. Its envelopes carry `"ocp": "2"`, not `"ocp": "1"`.
3. Hosts and plugins may support both v1 and v2 simultaneously by inspecting the `ocp` field at parse time.
4. v1 is NOT deprecated when v2 ships. Both versions may coexist indefinitely.
5. The v1 spec and crate remain frozen forever; v2 has its own contributing rules.

The threshold for declaring a v2 effort is high. It must be justified by a concrete need that genuinely cannot be addressed additively within v1, not by aesthetic preference or accumulated minor regrets.

## 10. Authorship and attribution

The `ocp-json/1` protocol is authored collectively. Individual contributors are credited via git history. There is no separate AUTHORS file.

When citing the protocol in external documentation, refer to it as `ocp-json/1` and cite the spec file URL (e.g., `docs/src/protocol/envelope-1.md`). Do not cite the `ocp-types-v1` crate as the protocol definition; the spec is the protocol.

## 11. Questions and disputes

If you are unsure whether a change is permitted under §3 or §4, open an issue *before* writing code. The triage outcome may be:

- "This is additive; proceed under §4."
- "This is editorial; proceed under §5.1 with the editorial classification."
- "This is breaking; deferred to v2."
- "This is ambiguous; the spec needs clarification first."

If a maintainer and a contributor disagree about whether a change is breaking, the conservative interpretation wins. The cost of incorrectly forbidding an additive change is one delayed minor release. The cost of incorrectly allowing a breaking change is the entire freeze guarantee.
