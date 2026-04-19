# Trust Model — Version 1

Open Choice uses an explicit trust model: every executable that runs must be either verified by a known key or explicitly approved by the user.

## Rules

1. **Path transparency** — The app must show the exact executable path it will run before executing.
2. **Hash verification** — If a `.oce` file pins `sha256`, the app must verify it before launch. Mismatch blocks execution by default.
3. **Unknown binary confirmation** — Executables without a trust record require the user to confirm or create an explicit trust record.
4. **Registry data is still data** — Registry metadata (manifest, plugin entries) is treated as data, never as executable logic.

## Trust tiers

| Tier | Who | What it means |
|---|---|---|
| `first_party` | Open Choice organization keys | Trusted without user confirmation for registry and app-signed plugins |
| `known_publisher` | Third-party plugin author keys in `trusted_keys.json` | Displayed to user; requires first-time confirmation |
| `self_signed` | Plugin with a key not present in `trusted_keys.json` | Developer mode only; requires explicit user approval |
| `unsigned` | Plugin with no signature at all | Developer mode only; requires explicit user approval |

## Key storage

Trusted public keys are stored in `src-tauri/resources/trusted_keys.json` (bundled with the app). Each entry:

```json
{
  "key_id": "open-choice-2026",
  "public_key_hex": "...",
  "trust_tier": "first_party"
}
```

Keys use Ed25519. The hex string is 64 characters (32 raw bytes).

## Revocation

The revocation list is a JSON document embedded in the app binary at build time (`src-tauri/resources/blocked_plugins.json`). It lists specific plugin ID + version combinations that have been revoked. The app checks all installed plugins against this list at startup and immediately quarantines any matches — no network request is required.

## Plugin verification flow

1. Download `.ocplugin` zip.
2. Extract `manifest.json` and `signatures/manifest.sig`.
3. Verify `manifest.sig` against the plugin's declared `public_key_id` in `trusted_keys.json`.
4. Verify binary SHA-256 matches the value in `manifest.json`.
5. If all checks pass, cache static zip assets into SQLite:
   - `schemas/<command>.schema.json` → `plugin_content_cache` (kind `schema`)
   - `examples/<command>.json` → `plugin_content_cache` (kind `examples`)
   - `help/<command>.json` → `plugin_content_cache` (kind `help`)
   - `outputs/<command>.json` → `plugin_content_cache` (kind `outputs`)
   - `manifest.snippets[]` → `snippets` table
6. Mark installation as verified; otherwise reject and alert the user.
