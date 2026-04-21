// Fetch + Ed25519-verify the signed heartbeat.
//
// Envelope (as produced by open-choice-heartbeat/scripts/sign_heartbeat.mjs):
//   { version: 1, signature_alg: "ed25519", payload_b64: "...", signature: "..." }
//
// Payload bytes (after base64 decode):
//   { issued_at: "...", valid_until: "...", version: 1 }
//
// Verification is over raw payload bytes — no JSON canonicalization.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, VerifyingKey, SIGNATURE_LENGTH};
use serde::Deserialize;
use thiserror::Error;

pub const HEARTBEAT_URL: &str = "https://heartbeat.openchoice.app/heartbeat.json";
const FETCH_TIMEOUT: Duration = Duration::from_secs(8);

/// Ed25519 public key matching the private key held in the heartbeat repo's
/// `HEARTBEAT_SIGNING_KEY` Actions secret. Populated by
/// `node scripts/generate_keypair.mjs` (prints the `[u8; 32]` literal).
pub const HEARTBEAT_PUBLIC_KEY: [u8; 32] = [
    0xf5, 0x51, 0x64, 0x28, 0x3d, 0x3f, 0xee, 0x63,
    0xd5, 0x7f, 0x1b, 0x7f, 0x0e, 0x14, 0x30, 0x9e,
    0x6e, 0x2c, 0x1e, 0x32, 0x64, 0x18, 0xcf, 0x5c,
    0x06, 0xaf, 0x09, 0x55, 0x02, 0x74, 0x04, 0x00,
];

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("network: {0}")]
    Network(#[from] reqwest::Error),
    #[error("HTTP {0}")]
    Http(u16),
    #[error("envelope parse: {0}")]
    Envelope(#[from] serde_json::Error),
    #[error("base64 decode: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("unsupported envelope version {0}")]
    UnsupportedEnvelopeVersion(u32),
    #[error("unsupported signature algorithm {0}")]
    UnsupportedSignatureAlg(String),
    #[error("bad signature length (expected 64, got {0})")]
    BadSignatureLength(usize),
    #[error("signature verification failed")]
    BadSignature,
    #[error("heartbeat public key is not configured (placeholder all-zeros)")]
    PublicKeyNotConfigured,
    #[error("bad public key: {0}")]
    BadPublicKey(String),
    #[error("payload is expired (valid_until {0} is in the past)")]
    Expired(DateTime<Utc>),
    #[error("unsupported payload version {0}")]
    UnsupportedPayloadVersion(u32),
}

#[derive(Deserialize)]
struct Envelope {
    version: u32,
    signature_alg: String,
    payload_b64: String,
    signature: String,
}

#[derive(Deserialize, Debug)]
pub struct Payload {
    pub issued_at:   DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub version:     u32,
}

/// Fetch + verify + return the decoded payload. Any verification or network
/// failure becomes a `FetchError`.
pub fn fetch_and_verify() -> Result<Payload, FetchError> {
    fetch_and_verify_with(HEARTBEAT_URL, &HEARTBEAT_PUBLIC_KEY)
}

/// Testable variant; lets tests point at a fixture URL and public key.
pub fn fetch_and_verify_with(url: &str, public_key: &[u8; 32]) -> Result<Payload, FetchError> {
    if *public_key == [0u8; 32] {
        return Err(FetchError::PublicKeyNotConfigured);
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()?;
    let resp = client.get(url).send()?;
    if !resp.status().is_success() {
        return Err(FetchError::Http(resp.status().as_u16()));
    }
    let bytes = resp.bytes()?;
    verify_bytes(&bytes, public_key)
}

/// Pure verifier — used by tests and by `fetch_and_verify_with`.
pub fn verify_bytes(envelope_bytes: &[u8], public_key: &[u8; 32]) -> Result<Payload, FetchError> {
    let env: Envelope = serde_json::from_slice(envelope_bytes)?;
    if env.version != 1 {
        return Err(FetchError::UnsupportedEnvelopeVersion(env.version));
    }
    if env.signature_alg != "ed25519" {
        return Err(FetchError::UnsupportedSignatureAlg(env.signature_alg));
    }
    let payload_bytes = BASE64.decode(env.payload_b64.as_bytes())?;
    let sig_bytes = BASE64.decode(env.signature.as_bytes())?;
    if sig_bytes.len() != SIGNATURE_LENGTH {
        return Err(FetchError::BadSignatureLength(sig_bytes.len()));
    }
    let mut sig_arr = [0u8; SIGNATURE_LENGTH];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = Signature::from_bytes(&sig_arr);
    let vkey = VerifyingKey::from_bytes(public_key)
        .map_err(|e| FetchError::BadPublicKey(e.to_string()))?;
    vkey.verify_strict(&payload_bytes, &signature)
        .map_err(|_| FetchError::BadSignature)?;

    let payload: Payload = serde_json::from_slice(&payload_bytes)?;
    if payload.version != 1 {
        return Err(FetchError::UnsupportedPayloadVersion(payload.version));
    }
    if payload.valid_until <= Utc::now() {
        return Err(FetchError::Expired(payload.valid_until));
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand::rngs::OsRng;

    fn make_envelope(payload_json: &str, key: &SigningKey) -> Vec<u8> {
        let payload_b64 = BASE64.encode(payload_json.as_bytes());
        let sig = key.sign(payload_json.as_bytes());
        let sig_b64 = BASE64.encode(sig.to_bytes());
        let env = serde_json::json!({
            "version": 1,
            "signature_alg": "ed25519",
            "payload_b64": payload_b64,
            "signature": sig_b64,
        });
        serde_json::to_vec(&env).unwrap()
    }

    #[test]
    fn verify_ok() {
        let key = SigningKey::generate(&mut OsRng);
        let vk = key.verifying_key().to_bytes();
        let payload = format!(
            r#"{{"issued_at":"2026-04-20T00:00:00Z","valid_until":"{}","version":1}}"#,
            (Utc::now() + chrono::Duration::hours(48)).to_rfc3339()
        );
        let env = make_envelope(&payload, &key);
        let p = verify_bytes(&env, &vk).unwrap();
        assert_eq!(p.version, 1);
    }

    #[test]
    fn wrong_key_fails() {
        let key = SigningKey::generate(&mut OsRng);
        let other = SigningKey::generate(&mut OsRng);
        let payload = format!(
            r#"{{"issued_at":"2026-04-20T00:00:00Z","valid_until":"{}","version":1}}"#,
            (Utc::now() + chrono::Duration::hours(48)).to_rfc3339()
        );
        let env = make_envelope(&payload, &key);
        match verify_bytes(&env, &other.verifying_key().to_bytes()) {
            Err(FetchError::BadSignature) => {}
            other => panic!("expected BadSignature, got {other:?}"),
        }
    }

    #[test]
    fn expired_fails() {
        let key = SigningKey::generate(&mut OsRng);
        let vk = key.verifying_key().to_bytes();
        let payload = r#"{"issued_at":"2020-01-01T00:00:00Z","valid_until":"2020-01-02T00:00:00Z","version":1}"#;
        let env = make_envelope(payload, &key);
        match verify_bytes(&env, &vk) {
            Err(FetchError::Expired(_)) => {}
            other => panic!("expected Expired, got {other:?}"),
        }
    }

    #[test]
    fn placeholder_key_fails_fetch() {
        // Exercises the zero-key guard without going to the network.
        let err = fetch_and_verify_with("http://127.0.0.1:1/unused", &[0u8; 32])
            .expect_err("should reject");
        matches!(err, FetchError::PublicKeyNotConfigured);
    }
}
