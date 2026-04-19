# Packaging and Signing

A plugin is distributed as a `.ocplugin` file — a zip archive containing the binary, the manifest, a signature, and static assets. The `oc-sign` CLI assembles and signs it.

## Install `oc-sign`

From the `ocp-sdk` workspace:
```bash
cargo install --path crates/oc-sign
```

Or once published to crates.io:
```bash
cargo install oc-sign
```

---

## Step 1: Generate a signing key

```bash
oc-sign keygen --key-id my-key-2026
```

This creates `my-key-2026.key` (the 64-hex-char private key) and prints the public key hex to stdout.

**Keep the private key file secret.** Never commit it to version control.

To share your public key (for adding to a registry's `trusted_keys.json`), print it any time:
```bash
oc-sign pubkey --key-file my-key-2026.key
```

---

## Step 2: Build the release binary

```bash
cargo build --release
```

For a multi-platform release you would cross-compile here, producing binaries for each OS/arch combination listed in your manifest.

---

## Step 3: Pack the `.ocplugin`

```bash
oc-sign pack packaging/manifest.json \
  target/release/my-echo-plugin.exe \
  --key-file my-key-2026.key \
  --out my-echo-plugin-0.1.0-windows-x86_64.ocplugin
```

`oc-sign pack` does the following:
1. Reads the manifest template.
2. Computes the SHA-256 of the binary.
3. Locates the entrypoint matching the current OS/arch and writes the real hash into its `digest.value`. It does this structurally (walking the JSON), so only the intended field is mutated — other fields that happen to contain `"__PLACEHOLDER__"` are untouched.
4. Signs the patched manifest with your key, writing the signature to `signing.signature_path`.
5. Creates the zip with the binary at `runtime.entrypoints[*].path`, the patched manifest at `manifest.json`, the signature at `signing.signature_path`, and all contents of `static/` at `static/`.

---

## `.ocplugin` zip layout

```
manifest.json
signatures/manifest.sig
bin/windows-x86_64/my-echo-plugin.exe
static/
├── schemas/echo.schema.json
├── examples/echo.json
├── help/echo.json
└── outputs/echo.json
```

The paths inside the zip must match:
- `runtime.entrypoints[*].path` — for each binary
- `signing.signature_path` — for the signature file
- `static/<kind>/<command>.json` — for static assets (the host looks for these paths by convention)

---

## Verifying the package

Before distributing, install locally via **Plugins → Install from file** in Open Choice. The host will:
1. Verify the signature against your public key (must be in `trusted_keys.json` or developer mode must be on).
2. Verify the binary's SHA-256 against the entrypoint's `digest.value`.
3. Show the install consent dialog with your declared capabilities and sandbox scopes.

If the host shows `trust_status = "warning"` after install, your key is not in the host's `trusted_keys.json`. This is expected in development — enable developer mode to proceed.

---

## Multi-platform packaging

Include one entrypoint per platform in the manifest:

```json
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
  },
  {
    "os": "linux",
    "arch": "x86_64",
    "path": "bin/linux-x86_64/my-echo-plugin",
    "digest": { "algorithm": "sha256", "value": "__PLACEHOLDER__" }
  }
]
```

`oc-sign pack` currently fills in the hash for the binary matching the current platform. For multi-platform builds, compute the hashes separately and patch the manifest before signing, or use a CI workflow that builds on each target.

---

## CI workflow sketch

```yaml
# .github/workflows/release.yml
- name: Build (Windows x86_64)
  run: cargo build --release
  # cross-compile for other targets as needed

- name: Pack plugin
  run: |
    oc-sign pack packaging/manifest.json \
      target/release/my-echo-plugin.exe \
      --key-file ${{ secrets.PLUGIN_SIGNING_KEY_PATH }} \
      --out dist/my-echo-plugin-${{ github.ref_name }}-windows-x86_64.ocplugin

- name: Upload release artifact
  uses: softprops/action-gh-release@v1
  with:
    files: dist/*.ocplugin
```

Store the signing key as a GitHub Actions secret (`PLUGIN_SIGNING_KEY_PATH` pointing to a file written from the secret value). The key never appears in logs.
