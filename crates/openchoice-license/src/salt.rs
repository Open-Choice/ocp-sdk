// Keyring-backed 32-byte salt for the state-file HMAC derivation.
//
// First load generates + stores; subsequent loads read back. Bump the account
// suffix (`state-mac-salt-v1` → `-v2`) to rotate the derivation across all
// users — old state files will fail HMAC verification on next launch and
// force a fresh online check.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use keyring::Entry;
use rand::RngCore;
use thiserror::Error;
use zeroize::Zeroize;

const SERVICE: &str = "open-choice";
const ACCOUNT: &str = "state-mac-salt-v1";

#[derive(Debug, Error)]
pub enum SaltError {
    #[error("keyring access failed: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("stored salt is not valid base64: {0}")]
    Decode(#[from] base64::DecodeError),
    #[error("stored salt has wrong length (expected 32 bytes, got {0})")]
    WrongLength(usize),
}

/// Load the 32-byte salt from the OS keyring, generating + persisting it on
/// first call. Returned buffer is zeroized on drop.
pub fn load_or_create() -> Result<Salt, SaltError> {
    let entry = Entry::new(SERVICE, ACCOUNT)?;
    match entry.get_password() {
        Ok(encoded) => {
            let bytes = BASE64.decode(encoded.as_bytes())?;
            if bytes.len() != 32 {
                return Err(SaltError::WrongLength(bytes.len()));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Ok(Salt(arr))
        }
        Err(keyring::Error::NoEntry) => {
            let mut arr = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut arr);
            entry.set_password(&BASE64.encode(arr))?;
            Ok(Salt(arr))
        }
        Err(e) => Err(e.into()),
    }
}

/// 32 bytes zeroized on drop.
pub struct Salt(pub [u8; 32]);

impl Drop for Salt {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl Salt {
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
