use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;

use crate::errors::RunnerError;

const TRUSTED_KEYS_JSON: &str = include_str!("../resources/trusted_keys.json");
const BLOCKED_PLUGINS_JSON: &str = include_str!("../resources/blocked_plugins.json");

// ── Trusted keys ──────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TrustedKeysFile {
    keys: Vec<TrustedKeyEntry>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TrustedKeyEntry {
    pub key_id: String,
    pub trust_tier: String,
    pub public_key_hex: String,
}

pub struct TrustedKeyStore {
    keys: Vec<TrustedKeyEntry>,
}

impl TrustedKeyStore {
    pub fn load() -> Result<Self, RunnerError> {
        let file: TrustedKeysFile = serde_json::from_str(TRUSTED_KEYS_JSON).map_err(|e| {
            RunnerError::internal(format!("Embedded trusted_keys.json is invalid: {}", e))
        })?;
        Ok(Self { keys: file.keys })
    }

    pub fn lookup(&self, key_id: &str) -> Option<&TrustedKeyEntry> {
        self.keys.iter().find(|k| k.key_id == key_id)
    }

    /// Returns `Ok(trust_tier)` if signature is valid and key is trusted.
    pub fn verify(&self, key_id: &str, message: &[u8], signature_bytes: &[u8]) -> Result<String, RunnerError> {
        let entry = self.lookup(key_id).ok_or_else(|| {
            RunnerError::untrusted_publisher(format!(
                "Signing key '{}' is not in the trusted key store.", key_id
            ))
        })?;

        let pub_key_bytes = hex::decode(&entry.public_key_hex).map_err(|_| {
            RunnerError::internal(format!(
                "Trusted key '{}' has an invalid public_key_hex value — build error.", key_id
            ))
        })?;

        if pub_key_bytes.len() != 32 {
            return Err(RunnerError::internal(format!(
                "Trusted key '{}' public key is {} bytes, expected 32 — build error.",
                key_id, pub_key_bytes.len()
            )));
        }

        let key_array: [u8; 32] = pub_key_bytes.try_into().map_err(|_| {
            RunnerError::internal("Failed to convert public key bytes to array.")
        })?;

        let verifying_key = VerifyingKey::from_bytes(&key_array).map_err(|e| {
            RunnerError::signature_verification_failed(format!(
                "Failed to construct verifying key for '{}': {}", key_id, e
            ))
        })?;

        if signature_bytes.len() != 64 {
            return Err(RunnerError::signature_verification_failed(format!(
                "Signature is {} bytes, expected 64.", signature_bytes.len()
            )));
        }

        let sig_array: [u8; 64] = signature_bytes.try_into().map_err(|_| {
            RunnerError::signature_verification_failed("Failed to convert signature bytes to array.")
        })?;

        let signature = Signature::from_bytes(&sig_array);

        verifying_key.verify(message, &signature).map_err(|e| {
            RunnerError::signature_verification_failed(format!(
                "Ed25519 signature verification failed for key '{}': {}", key_id, e
            ))
        })?;

        Ok(entry.trust_tier.clone())
    }
}

// ── Revocation list ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BlockedPluginsFile {
    blocked: Vec<BlockedEntry>,
}

#[derive(Debug, Deserialize)]
struct BlockedEntry {
    plugin_id: String,
    max_version: Option<String>,
    reason: String,
}

pub struct RevocationList {
    entries: Vec<BlockedEntry>,
}

impl RevocationList {
    pub fn load() -> Result<Self, RunnerError> {
        let file: BlockedPluginsFile = serde_json::from_str(BLOCKED_PLUGINS_JSON).map_err(|e| {
            RunnerError::internal(format!("Embedded blocked_plugins.json is invalid: {}", e))
        })?;
        Ok(Self { entries: file.blocked })
    }

    /// Returns `Some(reason)` if the plugin/version is revoked, `None` if allowed.
    pub fn check(&self, plugin_id: &str, version: &str) -> Option<&str> {
        for entry in &self.entries {
            if entry.plugin_id != plugin_id {
                continue;
            }
            match &entry.max_version {
                None => return Some(&entry.reason),
                Some(max_ver) => {
                    if version_le(version, max_ver) {
                        return Some(&entry.reason);
                    }
                }
            }
        }
        None
    }
}

fn normalize_version(v: &str) -> &str {
    let v = v.split('+').next().unwrap_or(v);
    v.split('-').next().unwrap_or(v)
}

fn version_le(v: &str, max: &str) -> bool {
    let v = normalize_version(v);
    let max = normalize_version(max);
    let v_parts: Vec<u64> = v.split('.').filter_map(|s| s.parse().ok()).collect();
    let m_parts: Vec<u64> = max.split('.').filter_map(|s| s.parse().ok()).collect();
    let len = v_parts.len().max(m_parts.len());
    for i in 0..len {
        let vp = v_parts.get(i).copied().unwrap_or(0);
        let mp = m_parts.get(i).copied().unwrap_or(0);
        if vp < mp { return true; }
        if vp > mp { return false; }
    }
    true
}
