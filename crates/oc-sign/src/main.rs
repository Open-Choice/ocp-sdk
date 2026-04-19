/// oc-sign — Open Choice plugin signing and packaging tool
///
/// Commands:
///
///   keygen --key-id <id>
///       Generate a new Ed25519 keypair. Prints the public key hex to stdout
///       (for adding to trusted_keys.json). Writes the private key hex to a
///       <key-id>.key file. KEEP THE .key FILE SECURE — never commit it.
///
///   sign <manifest-path> <sig-output-path> --key-file <path>
///       Sign a manifest.json file. Writes a 64-byte raw Ed25519 signature.
///
///   pubkey --key-file <path>
///       Print the public key hex for a given private key file.
///
///   pack <manifest-template> <binary-path> --key-file <path> --out <ocplugin-path>
///       Full packaging pipeline:
///         1. Hash the binary (SHA-256)
///         2. Parse the manifest template and structurally set the matching
///            entrypoint's `digest` field to `{ "algorithm": "sha256", "value": <hash> }`
///         3. Sign the patched manifest
///         4. Create a .ocplugin zip containing manifest.json, the binary, and
///            signatures/manifest.sig — all at the paths declared in the manifest
///
///   Example workflow:
///     # Generate keypair once:
///     cargo run -p oc-sign -- keygen --key-id open-choice-2026
///
///     # Package + sign a plugin:
///     cargo run -p oc-sign -- pack \
///         crates/toy-calculator/packaging/manifest.json \
///         target/release/toy-calculator.exe \
///         --key-file open-choice-2026.key \
///         --out dist/toy-calculator-0.1.0-windows-x86_64.ocplugin
use std::io::Write as IoWrite;
use std::path::Path;
use std::process;

use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage(&args[0]);
        process::exit(1);
    }

    let result = match args[1].as_str() {
        "keygen" => cmd_keygen(&args[2..]),
        "sign"   => cmd_sign(&args[2..]),
        "pubkey" => cmd_pubkey(&args[2..]),
        "pack"   => cmd_pack(&args[2..]),
        other => {
            eprintln!("error: unknown command '{}'", other);
            print_usage(&args[0]);
            process::exit(1);
        }
    };

    if let Err(msg) = result {
        eprintln!("error: {}", msg);
        process::exit(1);
    }
}

// ── keygen ────────────────────────────────────────────────────────────────────

fn cmd_keygen(args: &[String]) -> Result<(), String> {
    let key_id = flag_value(args, "--key-id")
        .ok_or("--key-id <id> is required")?;

    if key_id.is_empty() || key_id.contains(|c: char| c.is_whitespace() || c == '/') {
        return Err("--key-id must not contain whitespace or '/'".into());
    }

    let signing_key = SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();

    let private_hex = hex::encode(signing_key.to_bytes());
    let public_hex  = hex::encode(verifying_key.to_bytes());

    let key_file = format!("{}.key", key_id);
    std::fs::write(&key_file, &private_hex)
        .map_err(|e| format!("Failed to write private key file '{}': {}", key_file, e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to set permissions on '{}': {}", key_file, e))?;
    }

    println!("==> Keypair generated for '{}'", key_id);
    println!();
    println!("Private key written to: {}", key_file);
    println!("KEEP THIS FILE SECURE. Do not commit it to version control.");
    println!("Add it to .gitignore: echo '*.key' >> .gitignore");
    println!();
    println!("Add the following entry to trusted_keys.json in the app:");
    println!("{{");
    println!("  \"key_id\": \"{}\",", key_id);
    println!("  \"trust_tier\": \"first_party\",");
    println!("  \"public_key_hex\": \"{}\"", public_hex);
    println!("}}");

    Ok(())
}

// ── sign ──────────────────────────────────────────────────────────────────────

fn cmd_sign(args: &[String]) -> Result<(), String> {
    let positional = positional_args(args, &["--key-file"]);
    if positional.len() < 2 {
        return Err("usage: sign <manifest-path> <sig-output-path> --key-file <path>".into());
    }
    let manifest_path = Path::new(positional[0].as_str());
    let sig_path      = Path::new(positional[1].as_str());
    let key_file      = flag_value(args, "--key-file").ok_or("--key-file <path> is required")?;

    let signing_key = load_signing_key(Path::new(&key_file))?;
    let manifest_bytes = std::fs::read(manifest_path)
        .map_err(|e| format!("Failed to read '{}': {}", manifest_path.display(), e))?;

    // Refuse to sign anything that isn't valid JSON. Without this check, a
    // typo'd, truncated, or zero-byte manifest would still produce a
    // syntactically-valid Ed25519 signature that the host then trusts. The
    // verifier later requires the manifest to parse anyway, so failing fast
    // here turns a hard-to-diagnose runtime error into a clear sign-time one.
    if manifest_bytes.is_empty() {
        return Err(format!(
            "Refusing to sign '{}': file is empty.",
            manifest_path.display()
        ));
    }
    serde_json::from_slice::<serde_json::Value>(&manifest_bytes).map_err(|e| {
        format!(
            "Refusing to sign '{}': not valid JSON: {}",
            manifest_path.display(),
            e
        )
    })?;

    let signature = signing_key.sign(&manifest_bytes);
    let sig_bytes = signature.to_bytes();

    if let Some(parent) = sig_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create '{}': {}", parent.display(), e))?;
        }
    }

    std::fs::write(sig_path, sig_bytes)
        .map_err(|e| format!("Failed to write signature to '{}': {}", sig_path.display(), e))?;

    println!("==> Signed '{}' → '{}'", manifest_path.display(), sig_path.display());
    Ok(())
}

// ── pubkey ────────────────────────────────────────────────────────────────────

fn cmd_pubkey(args: &[String]) -> Result<(), String> {
    let key_file = flag_value(args, "--key-file").ok_or("--key-file <path> is required")?;
    let signing_key = load_signing_key(Path::new(&key_file))?;
    println!("{}", hex::encode(signing_key.verifying_key().to_bytes()));
    Ok(())
}

// ── pack ──────────────────────────────────────────────────────────────────────

fn cmd_pack(args: &[String]) -> Result<(), String> {
    // Positional args: <manifest-template> <binary-path>
    // Both `--key-file` and `--out` consume their next argument; the parser
    // must skip those values, otherwise an invocation like
    //   oc-sign pack --key-file k.key --out p.ocplugin manifest.json bin.exe
    // would silently treat `k.key` as the manifest template path.
    let positional = positional_args(args, &["--key-file", "--out"]);
    if positional.len() < 2 {
        return Err(
            "usage: pack <manifest-template> <binary-path> --key-file <path> --out <ocplugin-path>".into()
        );
    }
    let template_path = positional[0].as_str();
    let binary_path   = positional[1].as_str();
    let key_file      = flag_value(args, "--key-file").ok_or("--key-file <path> is required")?;
    let out_path      = flag_value(args, "--out").ok_or("--out <ocplugin-path> is required")?;

    // 1. Read binary and compute SHA-256.
    println!("==> Hashing binary '{}'...", binary_path);
    let binary_bytes = std::fs::read(binary_path)
        .map_err(|e| format!("Failed to read binary '{}': {}", binary_path, e))?;
    let hash = {
        let mut hasher = Sha256::new();
        hasher.update(&binary_bytes);
        hex::encode(hasher.finalize())
    };
    println!("    SHA-256: {}", hash);

    // 2. Patch manifest template — set the real SHA-256 on the matching
    //    entrypoint via a *structural* JSON edit.
    //
    //    The earlier implementation did a literal text replace of
    //    `"__PLACEHOLDER__"` anywhere in the file. That's fragile: if any
    //    other field (description, snippet body, release notes, test fixture)
    //    happened to contain the same literal, it would be silently corrupted.
    //    Worse, a malicious template could embed `"__PLACEHOLDER__"` in a
    //    non-hash field to confuse signing. Walk the JSON structure instead
    //    and write the hash into the entrypoint's `digest.value` exactly
    //    where `ocp_types_v1::manifest::RuntimeEntrypoint` expects it.
    println!("==> Patching manifest...");
    let template_text = std::fs::read_to_string(template_path)
        .map_err(|e| format!("Failed to read manifest template '{}': {}", template_path, e))?;
    let mut manifest: serde_json::Value = serde_json::from_str(&template_text)
        .map_err(|e| format!("Manifest template is not valid JSON: {}", e))?;

    // Locate (and mutate) the entrypoint matching the current host os/arch.
    // Hold the binary entry path for later zip placement.
    let binary_entry_path: String = {
        let entrypoints = manifest
            .get_mut("runtime")
            .and_then(|r| r.get_mut("entrypoints"))
            .and_then(|e| e.as_array_mut())
            .ok_or_else(|| "Manifest is missing runtime.entrypoints array".to_string())?;

        let entry = entrypoints
            .iter_mut()
            .find(|ep| {
                ep.get("os").and_then(|v| v.as_str()) == Some(current_os())
                    && ep.get("arch").and_then(|v| v.as_str()) == Some(current_arch())
            })
            .ok_or_else(|| {
                format!(
                    "No entrypoint found for {}-{} in manifest",
                    current_os(),
                    current_arch()
                )
            })?;

        let path = entry
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Matching entrypoint is missing 'path'".to_string())?
            .to_string();

        // Set digest structurally as a tagged {algorithm, value} object —
        // overwrites whatever was there (placeholder or stale value). This is
        // the only field we mutate. The tagged shape (vs. a bare "sha256"
        // string) is what lets future minor releases introduce additional
        // hash algorithms without breaking existing consumers.
        entry["digest"] = serde_json::json!({
            "algorithm": "sha256",
            "value":     hash.clone(),
        });
        path
    };

    let plugin_id = manifest
        .get("plugin_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let version = manifest
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let signing = manifest.get("signing").cloned();
    let sig_entry_path = signing
        .as_ref()
        .and_then(|s| s.get("signature_path"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    println!("    Plugin : {} v{}", plugin_id, version);
    println!("    Binary : {}", binary_entry_path);

    // Re-serialize the patched manifest exactly once. The same byte sequence
    // is used both for signing and for the zip entry, so the signature is
    // guaranteed to verify against what consumers see on disk.
    let patched_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| format!("Failed to serialize patched manifest: {}", e))?;

    // 3. Sign the manifest.
    let signing_key = load_signing_key(Path::new(&key_file))?;
    let manifest_bytes = patched_bytes.as_slice();
    let signature = signing_key.sign(manifest_bytes);
    let sig_bytes = signature.to_bytes();

    let key_id = signing
        .as_ref()
        .and_then(|s| s.get("key_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    println!("==> Signing manifest with key '{}'...", key_id);

    // 4. Create the .ocplugin zip.
    println!("==> Packaging → '{}'...", out_path);
    if let Some(parent) = Path::new(&out_path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create output directory: {}", e))?;
        }
    }

    let out_file = std::fs::File::create(&out_path)
        .map_err(|e| format!("Failed to create '{}': {}", out_path, e))?;
    let mut zip = zip::ZipWriter::new(out_file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // manifest.json
    zip.start_file("manifest.json", options)
        .map_err(|e| format!("Zip error: {}", e))?;
    zip.write_all(manifest_bytes)
        .map_err(|e| format!("Zip write error: {}", e))?;

    // binary
    zip.start_file(&binary_entry_path, options)
        .map_err(|e| format!("Zip error: {}", e))?;
    zip.write_all(&binary_bytes)
        .map_err(|e| format!("Zip write error: {}", e))?;

    // signature (only if the manifest declared a signature_path)
    if let Some(sig_path) = &sig_entry_path {
        zip.start_file(sig_path, options)
            .map_err(|e| format!("Zip error: {}", e))?;
        zip.write_all(&sig_bytes)
            .map_err(|e| format!("Zip write error: {}", e))?;
        println!("    Signature: {} ({})", sig_path, key_id);
    } else {
        println!("    Warning: no signing section in manifest — package is unsigned.");
    }

    zip.finish().map_err(|e| format!("Failed to finalize zip: {}", e))?;

    let size_kb = std::fs::metadata(&out_path).map(|m| m.len() / 1024).unwrap_or(0);
    println!();
    println!("==> Done!");
    println!("    File   : {}", out_path);
    println!("    Size   : {} KB", size_kb);
    println!("    Signed : {}", if sig_entry_path.is_some() { "yes" } else { "no" });
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn load_signing_key(path: &Path) -> Result<SigningKey, String> {
    let hex_str = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read key file '{}': {}", path.display(), e))?;
    let hex_str = hex_str.trim();

    if hex_str.len() != 64 {
        return Err(format!(
            "Key file '{}' must contain exactly 64 hex chars (32 bytes), got {}.",
            path.display(), hex_str.len()
        ));
    }

    let bytes = hex::decode(hex_str)
        .map_err(|_| format!("Key file '{}' contains invalid hex.", path.display()))?;
    let bytes_32: [u8; 32] = bytes.try_into()
        .map_err(|_| "Key bytes conversion failed.".to_string())?;
    Ok(SigningKey::from_bytes(&bytes_32))
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

/// Collect positional arguments from `args`, properly skipping flags AND
/// the values consumed by flags listed in `flags_with_values`.
///
/// The previous implementation used `args.iter().filter(|a| !a.starts_with('-'))`
/// which is *wrong*: the value of `--key-file <path>` does not start with `-`,
/// so it gets harvested as a positional. That bug let `oc-sign sign --key-file
/// k.key manifest.json sig.bin` silently treat `k.key` as the manifest path
/// and overwrite `manifest.json` with the signature bytes — destroying the
/// user's manifest and signing the wrong content.
fn positional_args<'a>(args: &'a [String], flags_with_values: &[&str]) -> Vec<&'a String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a.starts_with('-') {
            // Known flag that consumes a value: skip both.
            if flags_with_values.contains(&a.as_str()) {
                i += 2;
            } else {
                // Unknown flag: skip just the flag itself. (We accept the
                // imprecision; oc-sign has no boolean flags today, but if a
                // future addition lacks a value the worst case is we leak
                // its absent value into positionals — still no silent
                // file-overwrite, since the new flag would be unknown to
                // every command.)
                i += 1;
            }
            continue;
        }
        out.push(a);
        i += 1;
    }
    out
}

fn current_os() -> &'static str {
    #[cfg(target_os = "windows")] { "windows" }
    #[cfg(target_os = "macos")]   { "macos" }
    #[cfg(target_os = "linux")]   { "linux" }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    { "unknown" }
}

fn current_arch() -> &'static str {
    #[cfg(target_arch = "x86_64")]  { "x86_64" }
    #[cfg(target_arch = "aarch64")] { "aarch64" }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    { "unknown" }
}

fn print_usage(prog: &str) {
    eprintln!("Usage:");
    eprintln!("  {} keygen --key-id <id>", prog);
    eprintln!("  {} sign <manifest-path> <sig-output-path> --key-file <path>", prog);
    eprintln!("  {} pubkey --key-file <path>", prog);
    eprintln!("  {} pack <manifest-template> <binary-path> --key-file <path> --out <ocplugin-path>", prog);
}

#[cfg(test)]
mod positional_args_tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_string()).collect()
    }

    #[test]
    fn flag_at_end_old_behaviour_still_works() {
        let args = s(&["manifest.json", "sig.bin", "--key-file", "private.key"]);
        let pos = positional_args(&args, &["--key-file"]);
        assert_eq!(pos.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                   vec!["manifest.json", "sig.bin"]);
    }

    #[test]
    fn flag_at_start_does_not_leak_value_into_positionals() {
        // This is the regression test for the historical bug: the value of
        // --key-file must NOT show up in the positional list.
        let args = s(&["--key-file", "private.key", "manifest.json", "sig.bin"]);
        let pos = positional_args(&args, &["--key-file"]);
        assert_eq!(pos.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                   vec!["manifest.json", "sig.bin"]);
    }

    #[test]
    fn flag_in_middle_does_not_leak_value() {
        let args = s(&["manifest.json", "--key-file", "private.key", "sig.bin"]);
        let pos = positional_args(&args, &["--key-file"]);
        assert_eq!(pos.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                   vec!["manifest.json", "sig.bin"]);
    }

    #[test]
    fn pack_handles_two_value_flags() {
        let args = s(&[
            "--key-file", "k.key",
            "--out", "p.ocplugin",
            "manifest.json", "binary.exe",
        ]);
        let pos = positional_args(&args, &["--key-file", "--out"]);
        assert_eq!(pos.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                   vec!["manifest.json", "binary.exe"]);
    }

    #[test]
    fn pack_handles_interleaved_flags_and_positionals() {
        let args = s(&[
            "manifest.json",
            "--key-file", "k.key",
            "binary.exe",
            "--out", "p.ocplugin",
        ]);
        let pos = positional_args(&args, &["--key-file", "--out"]);
        assert_eq!(pos.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                   vec!["manifest.json", "binary.exe"]);
    }

    #[test]
    fn no_flags_returns_all_positionals() {
        let args = s(&["a", "b", "c"]);
        let pos = positional_args(&args, &["--key-file"]);
        assert_eq!(pos.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                   vec!["a", "b", "c"]);
    }

    #[test]
    fn empty_args_returns_empty() {
        let args: Vec<String> = vec![];
        let pos = positional_args(&args, &["--key-file"]);
        assert!(pos.is_empty());
    }

    #[test]
    fn positional_that_starts_with_dash_is_skipped_as_unknown_flag() {
        // This is a known limitation: a positional argument that starts with
        // `-` (e.g., a filename like `-weird.json`) gets treated as an
        // unknown flag and skipped. The pinned test documents the behaviour
        // so anyone who needs to support such filenames in the future has
        // a starting point. The fix would be to require `--` separators.
        let args = s(&["-weird.json", "sig.bin"]);
        let pos = positional_args(&args, &["--key-file"]);
        assert_eq!(pos.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                   vec!["sig.bin"]);
    }
}
