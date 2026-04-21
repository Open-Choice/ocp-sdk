// State file on disk: %APPDATA%\open-choice\state.json
//
// Shape:
//   { "state_b64": base64(JSON bytes), "mac_b64": base64(HMAC-SHA256(state_bytes)) }
//
// HMAC covers the exact state bytes (no canonicalization), same trick the
// heartbeat envelope uses. Load-time: recompute MAC, constant-time compare,
// mismatch → Tampered. Save-time: atomic tmp-then-rename.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use thiserror::Error;

use crate::derive::{self, DeriveError, HmacKey};

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("base64: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("hmac key derivation failed: {0}")]
    Derive(#[from] DeriveError),
    #[error("state file MAC did not match — tampered, copied, or salt/key rotated")]
    Tampered,
    #[error("state directory not available")]
    NoStateDir,
    #[error("unsupported state version {0}")]
    UnsupportedVersion(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseState {
    pub last_successful_check:   DateTime<Utc>,
    pub max_timestamp_ever_seen: DateTime<Utc>,
    pub heartbeat_valid_until:   DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emergency_bypass_until:  Option<DateTime<Utc>>,
    pub version: u32,
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    state_b64: String,
    mac_b64:   String,
}

/// `%APPDATA%\open-choice\state.json` on Windows (or the equivalent config
/// dir on other platforms — used only for tests).
pub fn default_path() -> Result<PathBuf, StateError> {
    let base = dirs::config_dir().ok_or(StateError::NoStateDir)?;
    Ok(base.join("open-choice").join("state.json"))
}

/// Load + HMAC-verify the state file. Missing file → `Ok(None)`. Tamper
/// (or key/salt rotation) → `Err(Tampered)`.
pub fn load() -> Result<Option<LicenseState>, StateError> {
    let path = default_path()?;
    let key = derive::derive()?;
    load_from(&path, &key)
}

/// Persist state after HMAC'ing it with the derived key.
pub fn save(state: &LicenseState) -> Result<(), StateError> {
    let path = default_path()?;
    let key = derive::derive()?;
    save_to(&path, &key, state)
}

// -- pure helpers -----------------------------------------------------------

pub fn load_from(path: &Path, key: &HmacKey) -> Result<Option<LicenseState>, StateError> {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let on_disk: OnDisk = serde_json::from_slice(&bytes)?;
    let state_bytes = BASE64.decode(on_disk.state_b64.as_bytes())?;
    let expected_mac = BASE64.decode(on_disk.mac_b64.as_bytes())?;
    let actual_mac = hmac(key, &state_bytes);
    if expected_mac.ct_eq(&actual_mac).unwrap_u8() != 1 {
        return Err(StateError::Tampered);
    }
    let state: LicenseState = serde_json::from_slice(&state_bytes)?;
    if state.version != 1 {
        return Err(StateError::UnsupportedVersion(state.version));
    }
    Ok(Some(state))
}

pub fn save_to(path: &Path, key: &HmacKey, state: &LicenseState) -> Result<(), StateError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let state_bytes = serde_json::to_vec(state)?;
    let mac_bytes = hmac(key, &state_bytes);
    let on_disk = OnDisk {
        state_b64: BASE64.encode(&state_bytes),
        mac_b64:   BASE64.encode(&mac_bytes),
    };
    let encoded = serde_json::to_vec_pretty(&on_disk)?;

    // Atomic tmp-then-rename.
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&encoded)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn hmac(key: &HmacKey, bytes: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).expect("32-byte HMAC key");
    mac.update(bytes);
    mac.finalize().into_bytes().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::derive_from;
    use chrono::TimeZone;
    use tempfile::tempdir;

    fn state(dt: DateTime<Utc>) -> LicenseState {
        LicenseState {
            last_successful_check:   dt,
            max_timestamp_ever_seen: dt,
            heartbeat_valid_until:   dt + chrono::Duration::hours(48),
            emergency_bypass_until:  None,
            version: 1,
        }
    }

    #[test]
    fn roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let key = derive_from(b"mid", &[1u8; 32]);
        let s = state(Utc.with_ymd_and_hms(2026, 4, 20, 0, 0, 0).unwrap());
        save_to(&path, &key, &s).unwrap();
        let loaded = load_from(&path, &key).unwrap().unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.last_successful_check, s.last_successful_check);
    }

    #[test]
    fn missing_file_is_none() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let key = derive_from(b"mid", &[1u8; 32]);
        assert!(load_from(&path, &key).unwrap().is_none());
    }

    #[test]
    fn tampering_fails_verification() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let key = derive_from(b"mid", &[1u8; 32]);
        let s = state(Utc.with_ymd_and_hms(2026, 4, 20, 0, 0, 0).unwrap());
        save_to(&path, &key, &s).unwrap();

        // Corrupt the state_b64 payload by flipping a byte inside the decoded bytes.
        let mut on_disk: OnDisk = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut bytes = BASE64.decode(&on_disk.state_b64).unwrap();
        bytes[0] ^= 0x01;
        on_disk.state_b64 = BASE64.encode(&bytes);
        fs::write(&path, serde_json::to_vec(&on_disk).unwrap()).unwrap();

        match load_from(&path, &key) {
            Err(StateError::Tampered) => {}
            other => panic!("expected Tampered, got {other:?}"),
        }
    }

    #[test]
    fn different_key_fails_verification() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("state.json");
        let k1 = derive_from(b"mid-1", &[1u8; 32]);
        let k2 = derive_from(b"mid-2", &[1u8; 32]);
        let s = state(Utc.with_ymd_and_hms(2026, 4, 20, 0, 0, 0).unwrap());
        save_to(&path, &k1, &s).unwrap();
        match load_from(&path, &k2) {
            Err(StateError::Tampered) => {}
            other => panic!("expected Tampered, got {other:?}"),
        }
    }
}
