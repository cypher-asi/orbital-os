//! Cryptographic operations for Zero-ID integration.
//!
//! This module wraps the canonical zid-crypto library and exposes
//! the functions needed by the identity service.
//!
//! # Security Invariants
//!
//! When reconstructing a Neural Key from shards, callers MUST use
//! [`combine_shards_verified`] to ensure the reconstructed key matches
//! the stored identity. This prevents attacks where arbitrary shards
//! are used to derive unauthorized machine keys.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use crate::error::KeyError;
use crate::keystore::{EncryptedShard, KeyDerivation};

pub use zid_crypto::{
    // Key types
    Ed25519KeyPair,
    MachineKeyPair,
    MachineKeyCapabilities as ZidMachineKeyCapabilities,
    NeuralKey,
    KeyScheme,
    
    // Key derivation
    derive_identity_signing_keypair,
    derive_machine_keypair_with_scheme,
    derive_machine_seed,
    derive_machine_signing_seed,
    derive_machine_encryption_seed,
    
    // Signing
    sign_message,
    verify_signature,
    
    // Challenge types
    Challenge,
    EntityType,
    
    // Canonical message builders
    canonicalize_identity_creation_message,
    canonicalize_enrollment_message,
    canonicalize_challenge,
    
    // Shamir secret sharing
    split_neural_key,
    combine_shards,
    NeuralShard as ZidNeuralShard,
};

// Re-export MachineKeyPair construction methods
// Note: These are inherent methods on MachineKeyPair, already accessible via the type export above

// Re-export for convenience
pub type IdentityKeypair = Ed25519KeyPair;

/// Helper to construct Uuid from bytes (avoids importing uuid in zos-apps)
pub fn uuid_from_bytes(bytes: &[u8; 16]) -> uuid::Uuid {
    uuid::Uuid::from_bytes(*bytes)
}

/// Reconstruct a Neural Key from shards with identity verification.
///
/// This is the **only** safe way to reconstruct a Neural Key for operations
/// that derive machine keys or perform other privileged actions.
///
/// # Security
///
/// This function enforces a critical security invariant: the reconstructed
/// Neural Key MUST derive the same identity signing public key that is stored
/// in the user's LocalKeyStore. Without this check, an attacker could provide
/// arbitrary shards to derive unauthorized machine keys.
///
/// # Arguments
///
/// * `shards` - At least 3 of the 5 Shamir shards
/// * `user_id` - The user ID (used for identity key derivation)
/// * `expected_identity_pubkey` - The stored identity signing public key from LocalKeyStore
///
/// # Errors
///
/// * `KeyError::InsufficientShards` - Fewer than 3 shards provided
/// * `KeyError::InvalidShard` - Shard data is malformed or reconstruction failed
/// * `KeyError::NeuralKeyMismatch` - Reconstructed key doesn't match stored identity
/// * `KeyError::DerivationFailed` - Identity key derivation failed
pub fn combine_shards_verified(
    shards: &[ZidNeuralShard],
    user_id: u128,
    expected_identity_pubkey: &[u8; 32],
) -> Result<NeuralKey, KeyError> {
    // Validate minimum shard count
    if shards.len() < 3 {
        return Err(KeyError::InsufficientShards);
    }

    // Reconstruct the Neural Key from shards (pure crypto operation)
    let neural_key = combine_shards(shards)
        .map_err(|e| KeyError::InvalidShard(alloc::format!("Shard reconstruction failed: {:?}", e)))?;

    // Derive the identity signing keypair from the reconstructed Neural Key
    let identity_uuid = uuid::Uuid::from_u128(user_id);
    let (derived_pubkey, _keypair) = derive_identity_signing_keypair(&neural_key, &identity_uuid)
        .map_err(|_| KeyError::DerivationFailed)?;

    // CRITICAL: Verify the derived public key matches the stored identity
    if &derived_pubkey != expected_identity_pubkey {
        return Err(KeyError::NeuralKeyMismatch);
    }

    Ok(neural_key)
}

// ============================================================================
// Password Validation and Shard Encryption
// ============================================================================

/// Minimum password length (12 characters for strong security)
pub const MIN_PASSWORD_LENGTH: usize = 12;

/// Validate password meets security requirements.
///
/// # Requirements
/// - Minimum 12 characters
///
/// # Returns
/// - `Ok(())` if password is valid
/// - `Err(KeyError::InvalidPassword)` with reason if invalid
pub fn validate_password(password: &str) -> Result<(), KeyError> {
    if password.len() < MIN_PASSWORD_LENGTH {
        return Err(KeyError::InvalidPassword(alloc::format!(
            "Password must be at least {} characters",
            MIN_PASSWORD_LENGTH
        )));
    }
    Ok(())
}

/// Derive an encryption key from a password using Argon2id.
///
/// # Parameters
/// - `password`: User-provided password
/// - `kdf`: Key derivation parameters (includes salt)
///
/// # Returns
/// - 32-byte AES-256 key
fn derive_key_from_password(password: &str, kdf: &KeyDerivation) -> Result<[u8; 32], KeyError> {
    use argon2::{Algorithm, Argon2, Params, Version};

    if kdf.algorithm != "Argon2id" {
        return Err(KeyError::CryptoError(alloc::format!(
            "Unsupported KDF algorithm: {}",
            kdf.algorithm
        )));
    }

    // Build Argon2id parameters
    let params = Params::new(
        kdf.memory_cost,      // memory cost in KB
        kdf.time_cost,        // iterations
        kdf.parallelism,      // parallelism
        Some(32),             // output length
    ).map_err(|e| KeyError::CryptoError(alloc::format!("Invalid KDF params: {:?}", e)))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), &kdf.salt, &mut key)
        .map_err(|e| KeyError::CryptoError(alloc::format!("Key derivation failed: {:?}", e)))?;

    Ok(key)
}

/// Encrypt a Neural Shard with password-derived key (Argon2id + AES-256-GCM).
///
/// # Parameters
/// - `shard`: The shard to encrypt (hex string from zid-crypto)
/// - `shard_index`: Original shard index (1-5)
/// - `password`: User-provided password
/// - `kdf`: Key derivation parameters (salt must be pre-generated)
///
/// # Returns
/// - `EncryptedShard` containing ciphertext, nonce, and auth tag
///
/// # Security
/// - Uses Argon2id with 2MB memory cost for password hardening (WASM-compatible)
/// - Uses AES-256-GCM for authenticated encryption
/// - Each shard gets a unique random nonce
pub fn encrypt_shard(
    shard_hex: &str,
    shard_index: u8,
    password: &str,
    kdf: &KeyDerivation,
) -> Result<EncryptedShard, KeyError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    // Derive encryption key from password
    let key = derive_key_from_password(password, kdf)?;

    // Create AES-256-GCM cipher
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| KeyError::CryptoError(alloc::format!("Cipher init failed: {:?}", e)))?;

    // Generate random nonce (12 bytes)
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce)
        .map_err(|e| KeyError::CryptoError(alloc::format!("Nonce generation failed: {:?}", e)))?;

    // Encrypt the shard hex string
    let plaintext = shard_hex.as_bytes();
    let ciphertext_with_tag = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext)
        .map_err(|e| KeyError::CryptoError(alloc::format!("Encryption failed: {:?}", e)))?;

    // AES-GCM appends the 16-byte tag to the ciphertext
    let tag_start = ciphertext_with_tag.len() - 16;
    let ciphertext = ciphertext_with_tag[..tag_start].to_vec();
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&ciphertext_with_tag[tag_start..]);

    Ok(EncryptedShard {
        index: shard_index,
        ciphertext,
        nonce,
        tag,
    })
}

/// Decrypt an encrypted shard with password-derived key.
///
/// # Parameters
/// - `encrypted`: The encrypted shard
/// - `password`: User-provided password
/// - `kdf`: Key derivation parameters used during encryption
///
/// # Returns
/// - Decrypted shard hex string
///
/// # Errors
/// - `KeyError::DecryptionFailed` if password is wrong or data is corrupted
pub fn decrypt_shard(
    encrypted: &EncryptedShard,
    password: &str,
    kdf: &KeyDerivation,
) -> Result<String, KeyError> {
    use aes_gcm::{
        aead::{Aead, KeyInit},
        Aes256Gcm, Nonce,
    };

    // Derive encryption key from password
    let key = derive_key_from_password(password, kdf)?;

    // Create AES-256-GCM cipher
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| KeyError::CryptoError(alloc::format!("Cipher init failed: {:?}", e)))?;

    // Reconstruct ciphertext with tag for decryption
    let mut ciphertext_with_tag = encrypted.ciphertext.clone();
    ciphertext_with_tag.extend_from_slice(&encrypted.tag);

    // Decrypt
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&encrypted.nonce), ciphertext_with_tag.as_slice())
        .map_err(|_| KeyError::DecryptionFailed)?;

    // Convert plaintext back to string
    String::from_utf8(plaintext)
        .map_err(|_| KeyError::DecryptionFailed)
}

/// Generate random salt for key derivation.
///
/// # Returns
/// - 32-byte random salt
pub fn generate_kdf_salt() -> Result<[u8; 32], KeyError> {
    let mut salt = [0u8; 32];
    getrandom::getrandom(&mut salt)
        .map_err(|e| KeyError::CryptoError(alloc::format!("Salt generation failed: {:?}", e)))?;
    Ok(salt)
}

/// Create default KDF parameters with a random salt.
///
/// Uses Argon2id with minimal parameters for WASM compatibility:
/// - Memory: 64 KB (minimum recommended)
/// - Iterations: 3 (minimum recommended)
/// - Parallelism: 1
///
/// WARNING: These are minimal parameters due to WASM performance constraints.
/// Argon2 in WASM runs 10-100x slower than native. In production, consider
/// using Web Crypto API's PBKDF2 for better browser performance.
pub fn create_kdf_params() -> Result<KeyDerivation, KeyError> {
    Ok(KeyDerivation {
        algorithm: String::from("Argon2id"),
        salt: generate_kdf_salt()?,
        time_cost: 3,
        memory_cost: 64, // 64 KB (WASM minimum)
        parallelism: 1,
    })
}

/// Select which shards to encrypt (randomly select 2 of 5).
///
/// # Returns
/// - Tuple of (encrypted_indices, external_indices) where each is sorted
///   - encrypted_indices: 2 indices of shards to encrypt (stored in keystore)
///   - external_indices: 3 indices of shards to show user (for paper backup)
pub fn select_shards_to_encrypt() -> Result<(Vec<u8>, Vec<u8>), KeyError> {
    // Create list of indices 1-5 and shuffle with Fisher-Yates
    let mut indices: Vec<u8> = (1..=5).collect();
    for i in (1..indices.len()).rev() {
        let mut rand_bytes = [0u8; 4];
        getrandom::getrandom(&mut rand_bytes)
            .map_err(|e| KeyError::CryptoError(alloc::format!("Random selection failed: {:?}", e)))?;
        let rand = u32::from_le_bytes(rand_bytes) as usize;
        let j = rand % (i + 1);
        indices.swap(i, j);
    }

    // First 2 are encrypted, remaining 3 are external
    let encrypted_indices: Vec<u8> = indices[..2].to_vec();
    let mut external_indices: Vec<u8> = indices[2..].to_vec();

    // Sort for consistent display
    external_indices.sort();

    Ok((encrypted_indices, external_indices))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_password_short() {
        assert!(validate_password("short").is_err());
        assert!(validate_password("12345678901").is_err()); // 11 chars
    }

    #[test]
    fn test_validate_password_ok() {
        assert!(validate_password("123456789012").is_ok()); // 12 chars
        assert!(validate_password("this-is-a-very-long-password").is_ok());
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let shard_hex = "deadbeef1234567890abcdef";
        let password = "secure-password-123";
        let kdf = create_kdf_params().unwrap();

        let encrypted = encrypt_shard(shard_hex, 1, password, &kdf).unwrap();
        assert_eq!(encrypted.index, 1);
        assert!(!encrypted.ciphertext.is_empty());

        let decrypted = decrypt_shard(&encrypted, password, &kdf).unwrap();
        assert_eq!(decrypted, shard_hex);
    }

    #[test]
    fn test_decrypt_wrong_password() {
        let shard_hex = "deadbeef1234567890abcdef";
        let password = "secure-password-123";
        let wrong_password = "wrong-password-456";
        let kdf = create_kdf_params().unwrap();

        let encrypted = encrypt_shard(shard_hex, 1, password, &kdf).unwrap();
        let result = decrypt_shard(&encrypted, wrong_password, &kdf);
        assert!(matches!(result, Err(KeyError::DecryptionFailed)));
    }

    #[test]
    fn test_select_shards_to_encrypt() {
        let (encrypted, external) = select_shards_to_encrypt().unwrap();
        
        // Should have 2 encrypted and 3 external
        assert_eq!(encrypted.len(), 2);
        assert_eq!(external.len(), 3);
        
        // All indices should be 1-5
        for &i in &encrypted {
            assert!(i >= 1 && i <= 5);
        }
        for &i in &external {
            assert!(i >= 1 && i <= 5);
        }
        
        // No overlap
        for &e in &encrypted {
            assert!(!external.contains(&e));
        }
    }
}
