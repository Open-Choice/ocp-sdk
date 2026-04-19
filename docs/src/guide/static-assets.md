# Static Assets

Your `.ocplugin` package bundles four static files per command. The host reads these directly from the zip at install time — your binary is never invoked for them.

```
static/
├── schemas/<command>.schema.json
├── examples/<command>.json
├── help/<command>.json
└── outputs/<command>.json
```

All four are optional. Missing files are treated as empty — the host falls back gracefully, showing no schema, no examples, no help, and no output definitions. In practice you should always provide at least `examples` and `help`.

---

## `schemas/<command>.schema.json`

A [JSON Schema](https://json-schema.org/) (draft 2020-12) describing the parameters for your command. The host uses this to power the form UI in the task editor.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "echo parameters",
  "type": "object",
  "required": ["message", "output_dir"],
  "properties": {
    "message": {
      "type": "string",
      "description": "The text to echo."
    },
    "output_dir": {
      "type": "string",
      "description": "Directory where result.txt will be written."
    }
  }
}
```

Keep this in sync with your `api validate` implementation. If the schema and the runtime validation disagree, users will see confusing errors.

---

## `examples/<command>.json`

An array of example task templates shown in the Help panel. Each example becomes a "Use this template" button that inserts an `.oce` snippet into the editor.

```json
[
  {
    "template_id": "echo-hello",
    "title": "Echo hello",
    "summary": "A simple hello-world echo.",
    "category": "starter",
    "oce_text": "[[\"my-echo-plugin::echo\"]]\nmessage = \"Hello, world!\"\noutput_dir = \"./outputs/echo\"\n",
    "message": "Hello, world!",
    "output_dir": "./outputs/echo"
  },
  {
    "template_id": "echo-multiline",
    "title": "Echo a longer message",
    "summary": "Demonstrates the message field with more content.",
    "category": "starter",
    "oce_text": "[[\"my-echo-plugin::echo\"]]\nmessage = \"Open Choice makes plugins easy.\"\noutput_dir = \"./outputs/echo\"\n",
    "message": "Open Choice makes plugins easy.",
    "output_dir": "./outputs/echo"
  }
]
```

### Required fields

| Field | Type | Description |
|-------|------|-------------|
| `template_id` | string | Unique stable ID. Used as a cache key — do not change it after release. |
| `title` | string | Shown in the Help panel template list. |
| `summary` | string | One-line description shown below the title. |
| `category` | string | `"starter"` or any label you choose — used for grouping. |
| `oce_text` | string | The `.oce` TOML content inserted when the user clicks "Use template". |

The remaining fields (e.g. `message`, `output_dir`) mirror the parameter values in `oce_text` as plain JSON. They are not required but let tooling parse examples structurally.

### `oce_text` format

The `oce_text` value is verbatim `.oce` TOML. Use the plugin's short name (last segment of `plugin_id`) as the alias:

```
[["my-echo-plugin::echo"]]\nmessage = "Hello"\noutput_dir = "./outputs/echo"\n
```

The trailing `\n` ensures the file ends with a newline when saved.

---

## `help/<command>.json`

Endpoint help content displayed in the Help panel when the user selects a command.

```json
{
  "command": "echo",
  "summary": "Echoes a message to a text file in the output directory.",
  "fields": [
    {
      "name": "message",
      "description": "The text to write. Any Unicode string is accepted.",
      "required": true,
      "accepted_values": []
    },
    {
      "name": "output_dir",
      "description": "Directory where result.txt is written. Created if it does not exist.",
      "required": true,
      "accepted_values": []
    }
  ],
  "usage": "Use this plugin to verify your Open Choice setup or as a starter template.",
  "output_notes": ["result.txt contains the echoed message verbatim."],
  "notes": ["output_dir is created automatically if it does not exist."]
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `command` | string | The command name this help block describes. |
| `summary` | string | Shown at the top of the help page. |
| `fields` | array | Per-parameter help entries (see below). |
| `usage` | string | Optional free-form usage notes. |
| `output_notes` | array of strings | What output files are produced and what they contain. |
| `notes` | array of strings | General tips, caveats, or conventions. |

Each entry in `fields`:

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Parameter name, matching the schema property key. |
| `description` | string | Full explanation of what the parameter does. |
| `required` | bool | Whether the parameter is required. |
| `accepted_values` | array of strings | Enum values if applicable; empty array otherwise. |

---

## `outputs/<command>.json`

Declares the output files your command produces. The host uses this to display artifact labels, configure open actions, and build the output catalog.

```json
{
  "command": "echo",
  "outputs": [
    {
      "kind": "result.txt",
      "file_name": "result.txt",
      "relative_path_pattern": "{output_dir}/result.txt",
      "required": true,
      "media_type": "text/plain",
      "description": "The echoed message.",
      "model_filters": [],
      "tags": ["result"]
    }
  ],
  "event_kinds": [
    "event.run.started",
    "event.artifact.created",
    "event.message.warning",
    "event.message.error",
    "event.run.finished"
  ]
}
```

The `kind` string in each output descriptor must match the `kind` field on the `ArtifactRecord` your plugin emits in `event.artifact.created` envelopes. This is how the host maps a runtime artifact to its declared metadata. Use values from `kinds-1.md` §8 (`result.csv`, `summary.md`, `run.log`, …) when they fit, or vendor-namespace (`<vendor>.<name>`) for domain-specific artifacts.

### `event_kinds`

List the `event.*` kinds your plugin emits during a run. The host uses this to size the event log and pre-seed the UI. Common values:

- `"event.run.started"` — always
- `"event.run.progress"` — if you emit progress ticks (requires the `events.progress` capability)
- `"event.run.heartbeat"` — if you emit liveness pings (requires `events.heartbeat`)
- `"event.artifact.created"` — for each output file
- `"event.artifact.updated"` — only when the run overwrites existing artifacts (requires `events.artifact_updates`)
- `"event.message.warning"` — non-terminal warnings
- `"event.message.error"` — non-terminal errors
- `"event.run.finished"` / `"event.run.failed"` / `"event.run.cancelled"` — terminal envelope

See [Events](events.md) for the full kind reference and the payload types.
