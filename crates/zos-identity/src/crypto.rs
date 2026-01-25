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

use crate::error::KeyError;

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
    
    // Signing
    sign_message,
    verify_signature,
    
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
