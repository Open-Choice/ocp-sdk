# .oce File Format — Schema Version 1

`.oce` files are TOML task definitions that bind tool references to commands and parameters.

## Normative fields

- `schema_version` must be `"1"`.
- `kind` must be `task`, `workflow`, or `bundle`.
- `tooling.protocol_version` must be explicit.
- `[[tools]]` entries define executable identity, pinning metadata, and resolution paths.
- `[[tasks]]` entries bind a task id to `tool_ref`, `command`, and `[tasks.params]`.
- `includes = ["..."]` is the bundle composition mechanism.

## Example

```toml
schema_version = "1"
kind            = "task"

[tooling]
protocol_version = "ocp-json/1"

[[tools]]
id      = "my-tool"
family  = "my-tool"
name    = "my-tool"
version = "1.0.0"
sha256  = "abcdef..."   # optional; verified before launch if present

[[tasks]]
id       = "run-default"
tool_ref = "my-tool"
command  = "compute"

[tasks.params]
input = "data.csv"
```

## Resolution rules

1. Includes are loaded depth-first.
2. Tool IDs and task IDs must remain unique across merged files.
3. Relative paths resolve from the containing `.oce` file.

## Runtime file

At execution time the host writes a `.oce.run.tmp` file — a resolved, self-contained snapshot of the task — and passes it to the executable:

```
exe run <path/to/file.oce.run.tmp> --task <task-id>
```

The temporary file is deleted after the run completes.
