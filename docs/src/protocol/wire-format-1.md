# ocp-json/1 — Wire Format

This document is **normative**. It defines the byte-level rules every `ocp-json/1` producer and consumer must obey, independent of any particular language implementation. The Rust crate `ocp-types` is one implementation of this format; conformance is defined by the `ocp-conformance` test suite, not by `ocp-types`.

The wire format is **frozen forever** as of `ocp-json/1`. Any change that violates these rules requires `ocp-json/2` and is governed by the rules in `CONTRIBUTING.md`. Additive evolution within `1.x` is permitted; everything below is the foundation that enables it.

## 1. Scope

The wire format governs three streams:

| Stream | Direction | Content | Specified by |
|---|---|---|---|
| Inspection response | binary stdout, one-shot | A single JSON object (the response envelope) | This document + `envelope-1.md` |
| Runtime event stream | binary stdout, continuous | NDJSON event envelopes | This document + `envelope-1.md` |
| Control channel | binary stdin, continuous | NDJSON control envelopes | This document + `envelope-1.md` |

The wire format does **not** govern:

- The `.ocplugin` package format (see the manifest specification).
- The `manifest.json` schema (see the manifest specification).
- Static assets in the package (`schemas/`, `help/`, `examples/`, `outputs/`, `signatures/`).
- The contents of stderr — stderr is for unstructured human-readable diagnostics, is never parsed by hosts, and has no protocol guarantees.

## 2. Encoding

- **Character encoding**: UTF-8 only. Producers MUST NOT emit any other encoding. Consumers MUST reject any input that is not valid UTF-8.
- **Byte order mark**: Producers MUST NOT emit a BOM (`U+FEFF`) at the start of any stream or any line. Consumers MUST reject input containing a BOM.
- **Normalization**: Strings are not required to be in any particular Unicode normalization form. Consumers MUST NOT normalize strings during parsing or round-tripping.

## 3. Framing

The runtime event stream and control channel are **NDJSON** (newline-delimited JSON). The inspection response is a single JSON object, optionally followed by a single trailing newline.

### 3.1 Line terminator

- Producers MUST terminate each NDJSON line with a single LF byte (`\n`, `0x0A`).
- Producers MUST NOT emit CRLF (`\r\n`), even on Windows.
- Consumers MUST tolerate CRLF for robustness: a `\r` byte immediately preceding a `\n` MUST be treated as part of the line terminator and discarded.
- Consumers MUST NOT treat any other byte as a line terminator.

### 3.2 Empty lines

- Producers SHOULD NOT emit empty lines.
- Consumers MUST silently skip empty lines (lines containing zero bytes between terminators).

### 3.3 Comment lines

- Producers MUST NOT emit comment lines in production output.
- Consumers MUST silently skip lines whose first non-whitespace byte is `#` (`0x23`). This rule exists exclusively to permit human-edited fixture files in the conformance suite and developer tools; it MUST NOT be relied upon in plugin output.

### 3.4 Maximum frame size

- A single NDJSON frame (one line, including its terminator) MUST NOT exceed **16 MiB** (16 × 1024 × 1024 bytes).
- Producers SHOULD keep frames under **1 MiB** as a soft target.
- Consumers MUST reject any frame exceeding 16 MiB by closing the stream and emitting a wire error.
- Payloads that would exceed the soft target SHOULD be written to disk as artifacts and referenced by `PathRef` rather than embedded inline.

### 3.5 Maximum nesting depth

- JSON object/array nesting MUST NOT exceed **64 levels**.
- Consumers MUST reject deeper nesting to prevent stack-exhaustion attacks.

### 3.6 Maximum string length

- Individual JSON string values MUST NOT exceed **1 MiB**.
- Longer text MUST be written to disk as an artifact.

## 4. JSON profile

The wire format restricts JSON in several ways beyond RFC 8259.

### 4.1 Number range

- JSON numbers used as integers MUST fit within the IEEE 754 double-precision safe-integer range: `[-(2^53 - 1), 2^53 - 1]`.
- Producers needing to express integers outside this range MUST encode them as JSON strings, and the field's documented type MUST indicate this (`string<u64>`, `string<i128>`, etc.).
- Producers MUST NOT emit `NaN`, `Infinity`, or `-Infinity` as JSON numbers (these are not valid JSON anyway, but some producers emit them in violation of the spec). Consumers MUST reject any input containing them.
- Floating-point numbers MUST round-trip via the Ryu algorithm (or any algorithm producing the shortest decimal that round-trips to the same `f64`). Producers using other algorithms risk producing wire output that differs across implementations of the same logical value.

### 4.2 String content

- All JSON strings MUST be valid UTF-8 (already required by §2).
- Strings MUST NOT contain unescaped control characters in the range `U+0000` through `U+001F`. Producers MUST escape them using JSON's `\u00XX` form.
- Strings MUST NOT contain unpaired UTF-16 surrogates. Producers MUST emit non-BMP characters using JSON's `\uXXXX\uXXXX` surrogate pair form OR using direct UTF-8 encoding.

### 4.3 Object keys

- Object keys MUST be unique within a single object. Consumers MUST reject any object with duplicate keys.
- Object key order is **unspecified**. Consumers MUST NOT depend on key order for semantics. Producers MAY emit keys in any order; round-trip tests in the conformance suite tolerate key reordering.
- Object keys MUST NOT exceed 256 bytes after UTF-8 encoding.

### 4.4 Trailing whitespace

- A line MAY contain trailing whitespace before its line terminator (spaces, tabs).
- Consumers MUST silently ignore trailing whitespace.
- Producers SHOULD NOT emit trailing whitespace.

## 5. Field naming

### 5.1 Convention

- All field names defined by `ocp-json/1` use **`snake_case`** (lowercase ASCII letters, digits, and underscores).
- Field names defined by `ocp-json/1` MUST match the regular expression `^[a-z][a-z0-9_]*$`.
- Field names MUST NOT begin with a digit.

### 5.2 Reserved name spaces

- Field names beginning with `_` (single underscore) are **reserved for implementation use** and `ocp-json/1` will never define a top-level field with such a name. Implementations MAY use them for internal annotations, debugging hints, or pre-release prototyping.
- Field names beginning with `x_` (the literal characters `x` and `_`) are **reserved for vendor experimentation** and `ocp-json/1` will never define a top-level field with such a name. Vendors SHOULD prefer the namespaced extension bag (`ext`, defined in `envelope-1.md`) over `x_` prefixes for any field intended to outlive a single experiment.

### 5.3 Forward compatibility rule

- Consumers MUST silently preserve any object key they do not recognize when round-tripping a message. Consumers MUST NOT use `deny_unknown_fields` (or its equivalent in any language) on any wire-format type.
- This rule is the load-bearing property that makes `ocp-json/1` permanently extensible. Violating it breaks forward compatibility and will fail the conformance suite.

## 6. Versioning

### 6.1 Wire format version

The wire format is versioned by a single field at the root of every envelope:

```json
{ "ocp": "1", ... }
```

- The `ocp` field MUST be the literal string `"1"` for the entire lifetime of `ocp-json/1`. It is never bumped to `"1.1"`, `"1.x"`, or anything else.
- A future incompatible wire format will use `"ocp": "2"` and is, by definition, `ocp-json/2`. It will be specified in a parallel document and is out of scope for this specification.
- The presence of `"ocp": "1"` is a sufficient (and necessary) signal that the rules in this document apply.

### 6.2 Sub-protocol versions

Higher-level concerns (event payload schemas, capability flags, kind registries) evolve under their own version numbers documented in `envelope-1.md`, `kinds-1.md`, and `capabilities-1.md`. Those versions can grow within the `ocp-json/1` lifetime via additive changes; they do not appear in the wire format itself.

### 6.3 Conformance level

A producer or consumer that obeys every rule in §§2–5 of this document is **wire-conformant** for `ocp-json/1`. Wire conformance is the minimum bar for interoperability. Higher conformance bars (envelope, kinds, capabilities) are layered on top and specified separately.

## 7. Streams and channels

### 7.1 Stream conventions

| File descriptor | Direction | Format | Required |
|---|---|---|---|
| `stdout` | plugin → host | NDJSON envelopes (`event` or `response` class) | Yes |
| `stdin` | host → plugin | NDJSON envelopes (`control` class) | Optional, opt-in via capability |
| `stderr` | plugin → host | Unstructured UTF-8 text | Optional, no protocol guarantees |

### 7.2 stdout

- `stdout` MUST be flushed after every NDJSON line written. Plugins that buffer their stdout will appear hung to the host.
- For `api validate` and `api self-test`, `stdout` contains exactly one JSON object (the response envelope) followed by exactly one LF terminator. The plugin then exits.
- For `run`, `stdout` contains a sequence of NDJSON event envelopes, terminated when the plugin exits. The final envelope before exit SHOULD be one of `event.run.finished`, `event.run.failed`, or `event.run.cancelled`.

### 7.3 stdin

- A plugin that does not advertise the `stdin.control_channel` capability in its manifest MUST NOT read from stdin during `run`. Hosts that interact with such plugins MUST close the plugin's stdin immediately after spawn.
- A plugin that advertises the capability MUST read NDJSON envelopes from stdin in a manner that does not block its main work loop (typically: a dedicated reader thread). Each line is a `control` class envelope.
- The control channel is half-duplex from the host's perspective. The plugin MAY emit corresponding events on stdout (e.g., emit `event.run.cancelled` after receiving `control.cancel`) but the protocol does not require any particular acknowledgement frame.
- When the host closes stdin (EOF), the plugin SHOULD interpret it as "no further control messages will be sent" and continue running normally.

### 7.4 stderr

- `stderr` is **outside the protocol**. Plugins MAY write any UTF-8 text to it for human-readable logging, debugging, or progress messages.
- Hosts MUST NOT parse stderr as structured data.
- Hosts MAY display stderr verbatim in a log view, capture it to a file, or discard it entirely.
- Plugins that need structured logging visible to the host MUST use `event.log.line` envelopes on stdout, not stderr.

## 8. Output format selection

The `run` invocation supports an `--output-format` flag:

```
exe run <task-file> --task <task-id> [--output-format protocol|human|quiet]
```

Three values are defined:

| Value | stdout content | stdin behavior |
|---|---|---|
| `protocol` | NDJSON event envelopes (this specification) | Per §7.3 |
| `human` | Human-readable terminal output (no protocol guarantees) | stdin closed |
| `quiet` | Single result line per the plugin's discretion (no protocol guarantees) | stdin closed |

When `--output-format` is absent, the plugin MUST auto-detect:

- If `stdout` is a TTY, behave as `human`.
- If `stdout` is a pipe or file, behave as `protocol`.

Hosts MUST always pass `--output-format protocol` explicitly when invoking plugins programmatically; auto-detection is for human terminal use only.

The wire format rules in this document apply **only** when the active output format is `protocol`. The `human` and `quiet` formats are unstructured and have no protocol guarantees.

## 9. Time and durations

### 9.1 Wall-clock timestamps

- Wire format: RFC 3339 string with mandatory `Z` suffix and microsecond precision.
- Pattern: `YYYY-MM-DDTHH:MM:SS.ffffffZ`
- Example: `"2026-04-07T12:34:56.123456Z"`
- Producers MUST emit microsecond precision (six fractional digits). Producers MAY emit additional precision (nanoseconds) by appending three more digits: `"2026-04-07T12:34:56.123456789Z"`. Consumers MUST accept both forms.
- Producers MUST NOT emit timezone offsets other than `Z`. `+00:00` is forbidden. Local times are forbidden.
- Producers MUST NOT emit RFC 3339 truncations (no fractional seconds, two-digit fractional seconds, etc.).
- Consumers MUST reject any timestamp string that does not match the pattern.

### 9.2 Durations

- Durations are encoded as a JSON object, not a single number, to avoid precision loss for sub-second values and to permit values longer than `f64` can express precisely.
- Wire format:
  ```json
  { "seconds": 12345, "nanos": 678900000 }
  ```
- The `seconds` field is a non-negative JSON integer (within safe-integer range).
- The `nanos` field is a non-negative JSON integer in `[0, 999_999_999]`. Producers MUST normalize: if `nanos >= 1_000_000_000`, increment `seconds` and reduce `nanos` accordingly.
- The `nanos` field MAY be omitted if zero; consumers MUST treat absence as zero.
- Producers MUST NOT use a single `f64` seconds field. Existing code that emits `elapsed_sec: 12.345` is wire-conformant under earlier drafts but MUST be migrated before `ocp-types-1.0.0`.

### 9.3 Monotonic clocks

- The wire format does not directly carry monotonic clock readings; durations are derived by the producer from its own monotonic clock and emitted as `Duration` objects.
- Producers SHOULD use a monotonic clock for measuring durations, not subtracting wall-clock timestamps. Wall clocks can move backward; durations should not.

## 10. Identifiers

### 10.1 Format

- All identifiers (`run_id`, `task_id`, `frame_id`, `chunk_id`, `artifact_id`) use the wrapped form documented in `envelope-1.md`. The default identifier format for `ocp-json/1` is **ULID** (Crockford base32, 26 characters, time-sortable).
- ULID grammar: `^[0-9A-HJKMNP-TV-Z]{26}$` (Crockford base32; excludes `I`, `L`, `O`, `U` to avoid visual ambiguity).
- Producers MUST emit ULIDs in uppercase.
- Consumers MUST accept lowercase ULIDs by uppercasing during parse, for robustness.

### 10.2 Task identifiers

- `task_id` values are not ULIDs; they are user-meaningful strings drawn from `.scaffolding` task files.
- `task_id` MUST match the regular expression `^[A-Za-z0-9][A-Za-z0-9_.\-]{0,255}$` (1–256 ASCII characters, starting with alphanumeric, containing only alphanumerics and `_`, `.`, `-`).
- This restriction permits task ids to be safely used as filesystem path components and URL path segments.

### 10.3 Stage identifiers

- `stage_id` values, when set by composition wrappers, MUST follow the same grammar as `task_id`.
- `stage_id` SHOULD use the convention `NN_<name>` where `NN` is a zero-padded ordinal (e.g., `01_design`, `02_wrangle`, `03_estimate`). This is a SHOULD not a MUST; wrappers may use any conformant identifier.

## 11. Paths

### 11.1 Path encoding

- Filesystem paths embedded in NDJSON envelopes MUST use forward slashes (`/`) as separators, even on Windows.
- Producers running on Windows MUST translate native backslash paths to forward-slash form before emission.
- Consumers running on Windows MUST translate forward-slash paths back to native form before passing them to filesystem APIs.
- Path strings MUST be valid UTF-8. Filesystems that permit non-UTF-8 path bytes (some Linux configurations) MUST percent-encode the offending bytes; consumers MUST decode percent escapes before filesystem use.

### 11.2 Path types

The protocol defines a `PathRef` type with multiple variants (`Local`, `Url`, `RunRelative`, `ContentAddressed`); see `envelope-1.md`. The wire format rules above apply to the path string carried inside any `Local` or `RunRelative` variant.

## 12. Hash and digest values

- Hash values are encoded as lowercase hexadecimal strings inside a `ContentDigest` tagged enum (see `envelope-1.md`).
- For SHA-256: 64 hex characters, lowercase, matching `^[0-9a-f]{64}$`.
- Producers MUST NOT emit base64-encoded hashes, mixed-case hex, or other forms.
- Consumers MUST reject hash values that do not match the documented pattern for their algorithm tag.

## 13. Conformance

A producer or consumer claims wire-format conformance for `ocp-json/1` if and only if it passes every test in the `wire-format/` section of the `ocp-conformance` test corpus.

The test corpus is the **operative definition** of conformance. This document is the **explanatory** definition. If they ever disagree, the corpus wins, and a corpus fix is filed against this document.

## 14. Forbidden changes (for governance reference)

The following changes to this document are **forbidden** once `ocp-types-1.0.0` is tagged:

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

Permitted changes are listed in `CONTRIBUTING.md`.

## 15. Open questions deferred to `envelope-1.md`

This document does not specify:

- The set of fields inside the envelope (`class`, `kind`, `id`, `ts`, `run`, `payload`, `ext`, etc.).
- The four envelope classes (`event`, `response`, `request`, `control`).
- Standard event kinds, response kinds, request kinds, control kinds.
- The `RunContext` composition envelope.
- The `Ext` extension bag mechanism.
- The capability flag mechanism.

All of those are specified in `envelope-1.md` (Step 2 of the v1 freeze plan).
