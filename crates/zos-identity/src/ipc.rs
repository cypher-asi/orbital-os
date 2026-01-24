//! IPC protocol definitions for the Identity layer.
//!
//! Defines request/response types for inter-process communication.
//! Message constants are defined in `zos-ipc` (the single source of truth).

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::error::{CredentialError, KeyError, SessionError, UserError};
use crate::keystore::{LocalKeyStore, MachineKeyCapabilities, MachineKeyRecord};
use crate::session::SessionId;
use crate::types::{User, UserId, UserStatus};

// ============================================================================
// Serde helpers for u128 as hex string (for JavaScript interop)
// ============================================================================

/// Serde module for serializing/deserializing u128 as hex string (e.g., "0x123abc")
/// Also accepts numbers for backward compatibility.
mod u128_hex_string {
    use alloc::format;
    use core::fmt;
    use serde::{self, de, Deserializer, Serializer};

    pub fn serialize<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("0x{:032x}", value))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u128, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct U128Visitor;

        impl<'de> de::Visitor<'de> for U128Visitor {
            type Value = u128;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string like '0x...' or a number")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let s = s.trim_start_matches("0x").trim_start_matches("0X");
                u128::from_str_radix(s, 16).map_err(de::Error::custom)
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(v as u128)
            }

            fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(v)
            }
        }

        deserializer.deserialize_any(U128Visitor)
    }
}

// ============================================================================
// User Service Request/Response Types
// ============================================================================

/// Create user request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateUserRequest {
    /// Display name for the new user
    pub display_name: String,
}

/// Create user response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateUserResponse {
    /// Result containing the created user or an error
    pub result: Result<User, UserError>,
}

/// Get user request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetUserRequest {
    /// User ID to retrieve
    pub user_id: UserId,
}

/// Get user response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetUserResponse {
    /// Result containing the user or an error
    pub result: Result<Option<User>, UserError>,
}

/// List users request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListUsersRequest {
    /// Optional status filter
    pub status_filter: Option<UserStatus>,
}

/// List users response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListUsersResponse {
    /// List of users
    pub users: Vec<User>,
}

/// Delete user request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeleteUserRequest {
    /// User ID to delete
    pub user_id: UserId,
    /// Whether to delete the home directory
    pub delete_home: bool,
}

/// Delete user response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeleteUserResponse {
    /// Result of the operation
    pub result: Result<(), UserError>,
}

// ============================================================================
// Session Request/Response Types
// ============================================================================

/// Login challenge request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginChallengeRequest {
    /// User ID attempting to login
    pub user_id: UserId,
}

/// Login challenge response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginChallengeResponse {
    /// Challenge nonce to sign
    pub challenge: [u8; 32],
    /// Challenge expiry (nanos since epoch)
    pub expires_at: u64,
}

/// Login verify request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginVerifyRequest {
    /// User ID
    pub user_id: UserId,
    /// Signed challenge
    pub signature: Vec<u8>,
    /// Original challenge (for verification)
    pub challenge: [u8; 32],
}

/// Login verify response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginVerifyResponse {
    /// Result of the verification
    pub result: Result<LoginSuccess, SessionError>,
}

/// Successful login result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginSuccess {
    /// Created session ID
    pub session_id: SessionId,
    /// Session token for subsequent requests
    pub session_token: String,
    /// Session expiry time
    pub expires_at: u64,
}

/// Logout request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogoutRequest {
    /// Session ID to end
    pub session_id: SessionId,
}

/// Logout response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogoutResponse {
    /// Result of the operation
    pub result: Result<(), SessionError>,
}

/// Whoami request (query current session info).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WhoamiRequest {
    // Empty - uses caller's process context
}

/// Whoami response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WhoamiResponse {
    /// User ID (if authenticated)
    pub user_id: Option<UserId>,
    /// Session ID (if authenticated)
    pub session_id: Option<SessionId>,
    /// User display name
    pub display_name: Option<String>,
    /// Session capabilities
    pub capabilities: Vec<String>,
}

// ============================================================================
// Credential Request/Response Types
// ============================================================================

/// Attach email credential request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachEmailRequest {
    /// User ID
    pub user_id: UserId,
    /// Email address to attach
    pub email: String,
}

/// Attach email response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachEmailResponse {
    /// Result of the operation
    pub result: Result<AttachEmailSuccess, CredentialError>,
}

/// Successful email attachment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachEmailSuccess {
    /// Verification required?
    pub verification_required: bool,
    /// Verification code sent to email (in dev mode only)
    pub verification_code: Option<String>,
}

/// Get credentials request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetCredentialsRequest {
    /// User ID
    pub user_id: UserId,
    /// Optional filter by credential type
    pub credential_type: Option<crate::keystore::CredentialType>,
}

/// Get credentials response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetCredentialsResponse {
    /// List of credentials
    pub credentials: Vec<crate::keystore::LinkedCredential>,
}

// ============================================================================
// Key Management Request/Response Types
// ============================================================================

// ============================================================================
// Neural Key Generation Request/Response Types
// ============================================================================

/// A Shamir shard for Neural Key backup.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NeuralShard {
    /// Shard index (1-5)
    pub index: u8,
    /// Shard data as hex string
    pub hex: String,
}

/// Public identifiers derived from the Neural Key.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PublicIdentifiers {
    /// Identity-level signing public key (Ed25519, hex string)
    pub identity_signing_pub_key: String,
    /// Machine-level signing public key (Ed25519, hex string)
    pub machine_signing_pub_key: String,
    /// Machine-level encryption public key (X25519, hex string)
    pub machine_encryption_pub_key: String,
}

/// Generate Neural Key request.
///
/// Triggers full key generation on the service:
/// 1. Generate 32 bytes of secure entropy
/// 2. Derive Ed25519/X25519 keypairs
/// 3. Split entropy into 5 Shamir shards
/// 4. Store public keys to VFS
/// 5. Return shards + public identifiers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerateNeuralKeyRequest {
    /// User ID to generate keys for
    pub user_id: UserId,
}

/// Result of successful Neural Key generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NeuralKeyGenerated {
    /// Public identifiers (stored server-side)
    pub public_identifiers: PublicIdentifiers,
    /// Shamir shards (3-of-5) - returned to UI for backup, NOT stored
    pub shards: Vec<NeuralShard>,
    /// Timestamp when the key was created
    pub created_at: u64,
}

/// Generate Neural Key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerateNeuralKeyResponse {
    /// Result containing the generated key info or an error
    pub result: Result<NeuralKeyGenerated, KeyError>,
}

/// Recover Neural Key from shards request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoverNeuralKeyRequest {
    /// User ID to recover keys for
    pub user_id: UserId,
    /// At least 3 shards required for recovery
    pub shards: Vec<NeuralShard>,
}

/// Recover Neural Key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoverNeuralKeyResponse {
    /// Result containing the recovered key info or an error
    pub result: Result<NeuralKeyGenerated, KeyError>,
}

// ============================================================================
// Identity Key Registration Request/Response Types
// ============================================================================

/// Register identity key request.
///
/// Registers the public keys derived from a Neural Key.
/// Private keys are stored client-side only.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterIdentityKeyRequest {
    /// User ID to register keys for
    pub user_id: UserId,
    /// Identity-level signing public key (Ed25519)
    pub identity_signing_public_key: [u8; 32],
    /// Machine-level signing public key (Ed25519)
    pub machine_signing_public_key: [u8; 32],
    /// Machine-level encryption public key (X25519)
    pub machine_encryption_public_key: [u8; 32],
}

/// Register identity key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegisterIdentityKeyResponse {
    /// Result of the registration
    pub result: Result<(), KeyError>,
}

/// Get identity key request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetIdentityKeyRequest {
    /// User ID to get keys for
    pub user_id: UserId,
}

/// Get identity key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetIdentityKeyResponse {
    /// Result containing the key store (if it exists)
    pub result: Result<Option<LocalKeyStore>, KeyError>,
}

/// Create machine key request.
///
/// Creates a new machine key record. Requires identity key to be registered first.
/// The service will generate machine keys using entropy - no public keys needed in request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateMachineKeyRequest {
    /// User ID
    pub user_id: UserId,
    /// Optional human-readable machine name
    pub machine_name: Option<String>,
    /// Machine key capabilities
    pub capabilities: MachineKeyCapabilities,
}

/// Create machine key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateMachineKeyResponse {
    /// Result containing the created machine record
    pub result: Result<MachineKeyRecord, KeyError>,
}

/// List machine keys request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListMachineKeysRequest {
    /// User ID to list machines for
    pub user_id: UserId,
}

/// List machine keys response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListMachineKeysResponse {
    /// List of machine key records
    pub machines: Vec<MachineKeyRecord>,
}

/// Get machine key request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetMachineKeyRequest {
    /// User ID
    pub user_id: UserId,
    /// Machine ID to retrieve (hex string for JavaScript interop)
    #[serde(with = "u128_hex_string")]
    pub machine_id: u128,
}

/// Get machine key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetMachineKeyResponse {
    /// Result containing the machine record (if it exists)
    pub result: Result<Option<MachineKeyRecord>, KeyError>,
}

/// Revoke machine key request.
///
/// Cannot revoke the primary/current machine key.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevokeMachineKeyRequest {
    /// User ID
    pub user_id: UserId,
    /// Machine ID to revoke (hex string for JavaScript interop)
    #[serde(with = "u128_hex_string")]
    pub machine_id: u128,
}

/// Revoke machine key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RevokeMachineKeyResponse {
    /// Result of the revocation
    pub result: Result<(), KeyError>,
}

/// Rotate machine key request.
///
/// Rotates the keys for a machine, incrementing the epoch.
/// The service will generate new keys using entropy - no public keys needed in request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotateMachineKeyRequest {
    /// User ID
    pub user_id: UserId,
    /// Machine ID to rotate keys for (hex string for JavaScript interop)
    #[serde(with = "u128_hex_string")]
    pub machine_id: u128,
}

/// Rotate machine key response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RotateMachineKeyResponse {
    /// Result containing the updated machine record
    pub result: Result<MachineKeyRecord, KeyError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = CreateUserRequest {
            display_name: String::from("Test User"),
        };

        // This would need serde_json for full test, just check it compiles
        let _ = req.display_name.len();
    }

    #[test]
    fn test_key_request_types() {
        // Test RegisterIdentityKeyRequest
        let reg_req = RegisterIdentityKeyRequest {
            user_id: 1,
            identity_signing_public_key: [0u8; 32],
            machine_signing_public_key: [1u8; 32],
            machine_encryption_public_key: [2u8; 32],
        };
        assert_eq!(reg_req.user_id, 1);

        // Test GetIdentityKeyRequest
        let get_req = GetIdentityKeyRequest { user_id: 2 };
        assert_eq!(get_req.user_id, 2);

        // Test CreateMachineKeyRequest
        let create_req = CreateMachineKeyRequest {
            user_id: 3,
            machine_name: Some(String::from("My Laptop")),
            capabilities: MachineKeyCapabilities::default(),
        };
        assert_eq!(create_req.user_id, 3);
        assert!(create_req.machine_name.is_some());

        // Test ListMachineKeysRequest
        let list_req = ListMachineKeysRequest { user_id: 4 };
        assert_eq!(list_req.user_id, 4);

        // Test RevokeMachineKeyRequest
        let revoke_req = RevokeMachineKeyRequest {
            user_id: 5,
            machine_id: 100,
        };
        assert_eq!(revoke_req.user_id, 5);
        assert_eq!(revoke_req.machine_id, 100);

        // Test RotateMachineKeyRequest
        let rotate_req = RotateMachineKeyRequest {
            user_id: 6,
            machine_id: 200,
        };
        assert_eq!(rotate_req.user_id, 6);
        assert_eq!(rotate_req.machine_id, 200);
    }
}
