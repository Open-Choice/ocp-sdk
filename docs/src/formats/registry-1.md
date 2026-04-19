# Registry Format — Version 1

The Open Choice plugin registry is a signed JSON manifest hosted in the `Open-Choice/registry` GitHub repository. It is data, not code — the app must never scrape GitHub HTML pages to discover releases.

## manifest.json structure

```json
{
  "schema_version": "1",
  "generated_at": "2026-01-01T00:00:00Z",
  "signature": "<base64-encoded Ed25519 signature>",
  "signer_key_id": "oc-registry-2026",
  "plugins": [
    {
      "plugin_id": "com.open-choice.toy-calculator",
      "display_name": "Toy Calculator",
      "description": "Demo plugin for testing the OCP protocol.",
      "publisher": "Open Choice",
      "categories": ["utilities"],
      "latest_version": "1.0.0",
      "versions": [
        {
          "version": "1.0.0",
          "download_url": "https://github.com/Open-Choice/plugin-toy-calculator/releases/download/v1.0.0/toy-calculator-windows-x86_64.ocplugin",
          "artifact_sha256": "abcdef0123456789...",
          "signer_key_id": "open-choice-2026",
          "min_app_version": null,
          "released_at": "2026-01-01T00:00:00Z"
        }
      ]
    }
  ]
}
```

## Signing

The registry manifest is signed by a dedicated `oc-registry-2026` Ed25519 key that is included in the app's `trusted_keys.json` with `trust_tier: "first_party"`.

The canonical form signed is:

```js
JSON.stringify({ generated_at, plugins, schema_version })
```

Keys are alphabetically ordered; the string is compact (no extra whitespace). The resulting signature is base64-encoded and stored in the `signature` field.

The host verifies the signature against `oc-registry-2026` before parsing registry content.

## Per-artifact integrity

Each entry in `versions[]` carries:
- `artifact_sha256` — hex-encoded SHA-256 of the `.ocplugin` zip
- `signer_key_id` — key ID in the app's `trusted_keys.json` used to verify the plugin's bundled `manifest.sig`
- `download_url` — HTTPS URL where the `.ocplugin` zip can be fetched
- `min_app_version` — optional minimum host version required to run this plugin version
- `released_at` — optional ISO-8601 release timestamp

The host verifies both the registry manifest signature (via `oc-registry-2026`) and the plugin's own `manifest.sig` (via the per-version `signer_key_id`) before installing.

## Update flow

1. App fetches `https://registry.openchoice.app/manifest.json`.
2. App verifies manifest signature with `oc-registry-2026`.
3. App presents available plugins; user selects one.
4. App downloads artifact from `versions[0].download_url`, verifies `artifact_sha256`.
5. App installs into local SQLite plugin cache.

## Pending submissions

Plugin authors submit via `scripts/add-plugin.js` which writes a draft to `plugins-pending/`. A GitHub Actions workflow runs `scripts/sign-manifest.js` on merge to regenerate and sign `manifest.json`.
