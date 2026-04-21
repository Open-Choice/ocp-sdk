// HMAC key derivation for the state file.
//
// hmac_key = HKDF-SHA256(
//   ikm  = machine_id || salt,
//   salt = b"openchoice-state-v1",
//   info = b"state-file-mac",
//   length = 32)
//
// machine_id comes from HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid on
// Windows. salt is 32 random bytes stored in the OS keyring (see salt.rs).
// Copying state.json between installs fails HMAC verification because either
// the MachineGuid or the keyring salt is different.

use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;
use zeroize::Zeroize;

use crate::salt::{self, SaltError};

const HKDF_SALT: &[u8] = b"openchoice-state-v1";
const HKDF_INFO: &[u8] = b"state-file-mac";

#[derive(Debug, Error)]
pub enum DeriveError {
    #[error("salt load failed: {0}")]
    Salt(#[from] SaltError),
    #[error("machine id read failed: {0}")]
    MachineId(String),
}

/// 32-byte HMAC key, zeroized on drop.
pub struct HmacKey(pub [u8; 32]);

impl Drop for HmacKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl HmacKey {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Derive the state-file HMAC key from (MachineGuid, keyring salt).
pub fn derive() -> Result<HmacKey, DeriveError> {
    let machine_id = read_machine_id()?;
    let salt = salt::load_or_create()?;
    Ok(derive_from(machine_id.as_bytes(), salt.as_bytes()))
}

/// Pure derivation used by tests and by `derive()`.
pub fn derive_from(machine_id: &[u8], salt: &[u8; 32]) -> HmacKey {
    let mut ikm = Vec::with_capacity(machine_id.len() + 32);
    ikm.extend_from_slice(machine_id);
    ikm.extend_from_slice(salt);
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), &ikm);
    let mut okm = [0u8; 32];
    hk.expand(HKDF_INFO, &mut okm).expect("32 bytes fits in one HKDF block");
    ikm.zeroize();
    HmacKey(okm)
}

#[cfg(windows)]
fn read_machine_id() -> Result<String, DeriveError> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey("SOFTWARE\\Microsoft\\Cryptography")
        .map_err(|e| DeriveError::MachineId(e.to_string()))?;
    let guid: String = key
        .get_value("MachineGuid")
        .map_err(|e| DeriveError::MachineId(e.to_string()))?;
    Ok(guid)
}

#[cfg(not(windows))]
fn read_machine_id() -> Result<String, DeriveError> {
    // Non-Windows is out of scope for v1; the client is Windows-only.
    Err(DeriveError::MachineId(
        "non-windows platforms are not supported".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_is_deterministic() {
        let salt = [7u8; 32];
        let a = derive_from(b"machine-guid-abc", &salt);
        let b = derive_from(b"machine-guid-abc", &salt);
        assert_eq!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn different_machines_derive_different_keys() {
        let salt = [7u8; 32];
        let a = derive_from(b"machine-guid-abc", &salt);
        let b = derive_from(b"machine-guid-xyz", &salt);
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn different_salts_derive_different_keys() {
        let a = derive_from(b"machine-guid-abc", &[1u8; 32]);
        let b = derive_from(b"machine-guid-abc", &[2u8; 32]);
        assert_ne!(a.as_bytes(), b.as_bytes());
    }
}
