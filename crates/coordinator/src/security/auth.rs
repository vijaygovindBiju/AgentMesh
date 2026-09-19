//! Agent Authentication, API Key Hashing, and Verification.

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use agent_protocol::security::{AgentRole, PermissionBoundary};

/// Secure API Key generation and verification.
pub struct ApiKeyManager;

impl ApiKeyManager {
    /// Generates a cryptographically strong raw API key and its corresponding SHA-256 storage hash.
    /// Format: `am_ak_<64_hex_chars>`
    pub fn generate_key() -> (String, String) {
        let mut random_bytes = [0u8; 32];
        // Use system randomness or uuid v4 bytes combined
        let u1 = Uuid::new_v4();
        let u2 = Uuid::new_v4();
        random_bytes[..16].copy_from_slice(u1.as_bytes());
        random_bytes[16..].copy_from_slice(u2.as_bytes());

        let raw_token = hex::encode(random_bytes);
        let raw_key = format!("am_ak_{raw_token}");
        let hash = Self::hash_key(&raw_key);
        (raw_key, hash)
    }

    /// Computes the SHA-256 hash of an API key as a hexadecimal string.
    pub fn hash_key(raw_key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(raw_key.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Verifies a presented raw API key against a stored hash using constant-time comparison.
    /// Also supports legacy unhashed keys for backwards compatibility in test environments.
    pub fn verify_key(presented_key: &str, stored_hash_or_key: &str) -> bool {
        let computed_hash = Self::hash_key(presented_key);

        // 1. Check if stored is a matching SHA-256 hash
        let hash_matches = if stored_hash_or_key.len() == 64 {
            computed_hash.as_bytes().ct_eq(stored_hash_or_key.as_bytes()).into()
        } else {
            false
        };

        if hash_matches {
            return true;
        }

        // 2. Fallback for backwards compatibility with plain test keys (e.g. "cap_key_123")
        presented_key.as_bytes().ct_eq(stored_hash_or_key.as_bytes()).into()
    }

    /// Generates an ephemeral session token for an authenticated agent.
    pub fn generate_session_token(agent_id: Uuid) -> String {
        let rand_suffix = Uuid::new_v4().simple().to_string();
        format!("am_tok_{}_{}", agent_id.simple(), rand_suffix)
    }
}

/// Context of an authenticated agent.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub agent_id: Uuid,
    pub human_owner: String,
    pub role: AgentRole,
    pub permissions: PermissionBoundary,
    pub is_revoked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_key_generation_and_verification() {
        let (raw_key, hash) = ApiKeyManager::generate_key();
        assert!(raw_key.starts_with("am_ak_"));
        assert_eq!(hash.len(), 64);

        // Verification with correct key
        assert!(ApiKeyManager::verify_key(&raw_key, &hash));

        // Verification with invalid key
        assert!(!ApiKeyManager::verify_key("am_ak_invalid_random_string", &hash));
    }

    #[test]
    fn test_legacy_unhashed_key_verification_backwards_compat() {
        let plain_key = "legacy_test_key_12345";
        assert!(ApiKeyManager::verify_key(plain_key, plain_key));
        assert!(!ApiKeyManager::verify_key("wrong_key", plain_key));
    }

    #[test]
    fn test_session_token_generation() {
        let agent_id = Uuid::new_v4();
        let token = ApiKeyManager::generate_session_token(agent_id);
        assert!(token.starts_with("am_tok_"));
        assert!(token.contains(&agent_id.simple().to_string()));
    }
}
