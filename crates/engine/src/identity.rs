//! Local peer identity (spec section 6): "Each user has a local keypair
//! (no account), generated on first install." No account, no server --
//! this keypair is what an `.apiworkspace` approver list references and
//! what signs conflict-review approvals once that feature exists.
//! Ed25519: small keys, fast, exactly the class of signature scheme this
//! use case calls for (attributing a merge decision, not securing
//! money).
//!
//! Nothing outside this module ever needs the secret key in raw form --
//! `sign` is the only operation that touches it.

use base64::engine::general_purpose::STANDARD_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};

use crate::error::EngineError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalIdentity {
    /// Base64, no padding -- diff-friendly, and directly copy-pasteable
    /// into another workspace's `.apiworkspace` approver block.
    pub public_key: String,
    /// Never leaves this machine: not present in `.apiworkspace`, never
    /// serialized to any file other than this identity's own.
    pub secret_key: String,
    /// What shows up next to this identity's approvals -- editable,
    /// unlike the key itself. Defaults to the OS account name at
    /// generation time, but carries no other connection to it.
    pub display_name: String,
}

impl LocalIdentity {
    pub fn generate(display_name: impl Into<String>) -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        LocalIdentity {
            public_key: STANDARD_NO_PAD.encode(signing_key.verifying_key().as_bytes()),
            secret_key: STANDARD_NO_PAD.encode(signing_key.to_bytes()),
            display_name: display_name.into(),
        }
    }

    fn signing_key(&self) -> Result<SigningKey, EngineError> {
        let bytes = STANDARD_NO_PAD
            .decode(&self.secret_key)
            .map_err(|e| EngineError::ParseFormat(format!("malformed identity secret key: {e}")))?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| EngineError::ParseFormat("identity secret key is not 32 bytes".to_string()))?;
        Ok(SigningKey::from_bytes(&bytes))
    }

    /// Signs `message`, returning a base64 (no-pad) signature. The only
    /// place this identity's secret key is ever touched.
    pub fn sign(&self, message: &[u8]) -> Result<String, EngineError> {
        let signature: Signature = self.signing_key()?.sign(message);
        Ok(STANDARD_NO_PAD.encode(signature.to_bytes()))
    }
}

/// Verifies a signature against a *public* key -- for checking someone
/// else's approval, never this machine's own (`LocalIdentity::sign`
/// covers that). Returns `Ok(false)` for a well-formed but non-matching
/// signature, `Err` only for malformed input (not valid base64, wrong
/// byte length).
pub fn verify(public_key_b64: &str, message: &[u8], signature_b64: &str) -> Result<bool, EngineError> {
    let pk_bytes = STANDARD_NO_PAD
        .decode(public_key_b64)
        .map_err(|e| EngineError::ParseFormat(format!("malformed public key: {e}")))?;
    let pk_bytes: [u8; 32] = pk_bytes
        .try_into()
        .map_err(|_| EngineError::ParseFormat("public key is not 32 bytes".to_string()))?;
    let verifying_key =
        VerifyingKey::from_bytes(&pk_bytes).map_err(|e| EngineError::ParseFormat(format!("invalid public key: {e}")))?;

    let sig_bytes = STANDARD_NO_PAD
        .decode(signature_b64)
        .map_err(|e| EngineError::ParseFormat(format!("malformed signature: {e}")))?;
    let sig_bytes: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| EngineError::ParseFormat("signature is not 64 bytes".to_string()))?;
    let signature = Signature::from_bytes(&sig_bytes);

    Ok(verifying_key.verify(message, &signature).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_distinct_keypairs_each_time() {
        let a = LocalIdentity::generate("Alice");
        let b = LocalIdentity::generate("Alice");
        assert_ne!(a.public_key, b.public_key);
        assert_ne!(a.secret_key, b.secret_key);
    }

    #[test]
    fn sign_and_verify_round_trip() {
        let identity = LocalIdentity::generate("Alice");
        let signature = identity.sign(b"approve merge abc123").unwrap();
        assert!(verify(&identity.public_key, b"approve merge abc123", &signature).unwrap());
    }

    #[test]
    fn verify_rejects_tampered_message() {
        let identity = LocalIdentity::generate("Alice");
        let signature = identity.sign(b"approve merge abc123").unwrap();
        assert!(!verify(&identity.public_key, b"approve merge XXXXXX", &signature).unwrap());
    }

    #[test]
    fn verify_rejects_wrong_public_key() {
        let identity = LocalIdentity::generate("Alice");
        let other = LocalIdentity::generate("Bob");
        let signature = identity.sign(b"approve merge abc123").unwrap();
        assert!(!verify(&other.public_key, b"approve merge abc123", &signature).unwrap());
    }

    #[test]
    fn verify_errors_cleanly_on_malformed_input() {
        assert!(verify("not-base64!!", b"msg", "also-not-base64!!").is_err());
    }
}
