//! Cryptographic key storage for the Identity layer.
//!
//! Provides types and operations for Zero-ID key management.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::types::UserId;

/// Local storage for user cryptographic material (public keys).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalKeyStore {
    /// User ID this key store belongs to
    pub user_id: UserId,

    /// Identity-level signing public key (Ed25519)
    pub identity_signing_public_key: [u8; 32],

    /// Machine-level signing public key (Ed25519)
    pub machine_signing_public_key: [u8; 32],

    /// Machine-level encryption public key (X25519)
    pub machine_encryption_public_key: [u8; 32],

    /// Key scheme in use
    pub key_scheme: KeyScheme,

    /// Machine key capabilities
    pub capabilities: MachineKeyCapabilities,

    /// Key epoch (increments on rotation)
    pub epoch: u64,

    /// Post-quantum signing public key (if PqHybrid scheme)
    pub pq_signing_public_key: Option<Vec<u8>>,

    /// Post-quantum encryption public key (if PqHybrid scheme)
    pub pq_encryption_public_key: Option<Vec<u8>>,
}

impl LocalKeyStore {
    /// Path where public keys are stored.
    pub fn storage_path(user_id: UserId) -> String {
        alloc::format!("/home/{:032x}/.zos/identity/public_keys.json", user_id)
    }

    /// Create a new key store with the given keys.
    pub fn new(
        user_id: UserId,
        identity_signing_public_key: [u8; 32],
        machine_signing_public_key: [u8; 32],
        machine_encryption_public_key: [u8; 32],
    ) -> Self {
        Self {
            user_id,
            identity_signing_public_key,
            machine_signing_public_key,
            machine_encryption_public_key,
            key_scheme: KeyScheme::default(),
            capabilities: MachineKeyCapabilities::default(),
            epoch: 1,
            pq_signing_public_key: None,
            pq_encryption_public_key: None,
        }
    }
}

/// Cryptographic key scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyScheme {
    /// Ed25519 signing + X25519 encryption (default)
    Ed25519X25519,

    /// Hybrid: Ed25519/X25519 + Dilithium/Kyber (post-quantum)
    PqHybrid,
}

impl Default for KeyScheme {
    fn default() -> Self {
        Self::Ed25519X25519
    }
}

/// Capabilities of machine-level keys.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachineKeyCapabilities {
    /// Can sign authentication challenges
    pub can_authenticate: bool,

    /// Can encrypt/decrypt local data
    pub can_encrypt: bool,

    /// Can sign messages on behalf of user
    pub can_sign_messages: bool,

    /// Can authorize new machines
    pub can_authorize_machines: bool,

    /// Can revoke other machines
    pub can_revoke_machines: bool,

    /// Expiry time (None = no expiry)
    pub expires_at: Option<u64>,
}

impl Default for MachineKeyCapabilities {
    fn default() -> Self {
        Self {
            can_authenticate: true,
            can_encrypt: true,
            can_sign_messages: false,
            can_authorize_machines: false,
            can_revoke_machines: false,
            expires_at: None,
        }
    }
}

impl MachineKeyCapabilities {
    /// Create capabilities with all permissions.
    pub fn full() -> Self {
        Self {
            can_authenticate: true,
            can_encrypt: true,
            can_sign_messages: true,
            can_authorize_machines: true,
            can_revoke_machines: true,
            expires_at: None,
        }
    }

    /// Check if the capabilities are expired.
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at.map_or(false, |exp| now >= exp)
    }
}

/// Encrypted private key storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedPrivateKeys {
    /// Encryption algorithm used
    pub algorithm: String,

    /// Key derivation function parameters
    pub kdf: KeyDerivation,

    /// Encrypted key bundle
    pub ciphertext: Vec<u8>,

    /// Nonce/IV for decryption
    pub nonce: [u8; 12],

    /// Authentication tag
    pub tag: [u8; 16],
}

impl EncryptedPrivateKeys {
    /// Path where encrypted keys are stored.
    pub fn storage_path(user_id: UserId) -> String {
        alloc::format!("/home/{:032x}/.zos/identity/private_keys.enc", user_id)
    }
}

/// Key derivation parameters.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyDerivation {
    /// KDF algorithm (e.g., "Argon2id")
    pub algorithm: String,

    /// Salt for KDF
    pub salt: [u8; 32],

    /// Time cost (iterations)
    pub time_cost: u32,

    /// Memory cost (KB)
    pub memory_cost: u32,

    /// Parallelism
    pub parallelism: u32,
}

impl Default for KeyDerivation {
    fn default() -> Self {
        Self {
            algorithm: String::from("Argon2id"),
            salt: [0u8; 32],
            time_cost: 3,
            memory_cost: 65536, // 64 MB
            parallelism: 1,
        }
    }
}

/// Per-machine key record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachineKeyRecord {
    /// Machine ID
    pub machine_id: u128,

    /// Machine-specific signing public key
    pub signing_public_key: [u8; 32],

    /// Machine-specific encryption public key
    pub encryption_public_key: [u8; 32],

    /// When this machine was authorized
    pub authorized_at: u64,

    /// Who authorized this machine (user_id or machine_id)
    pub authorized_by: u128,

    /// Machine capabilities
    pub capabilities: MachineKeyCapabilities,

    /// Human-readable machine name
    pub machine_name: Option<String>,

    /// Last seen timestamp
    pub last_seen_at: u64,
}

impl MachineKeyRecord {
    /// Path where machine key is stored.
    pub fn storage_path(user_id: UserId, machine_id: u128) -> String {
        alloc::format!(
            "/home/{:032x}/.zos/identity/machine/{:032x}.json",
            user_id, machine_id
        )
    }
}

/// Linked external credentials.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CredentialStore {
    /// User ID
    pub user_id: UserId,

    /// Linked credentials
    pub credentials: Vec<LinkedCredential>,
}

impl CredentialStore {
    /// Path where credentials are stored.
    pub fn storage_path(user_id: UserId) -> String {
        alloc::format!(
            "/home/{:032x}/.zos/credentials/credentials.json",
            user_id
        )
    }

    /// Create a new empty credential store.
    pub fn new(user_id: UserId) -> Self {
        Self {
            user_id,
            credentials: Vec::new(),
        }
    }

    /// Add a credential.
    pub fn add(&mut self, credential: LinkedCredential) {
        self.credentials.push(credential);
    }

    /// Find credentials by type.
    pub fn find_by_type(&self, cred_type: CredentialType) -> Vec<&LinkedCredential> {
        self.credentials
            .iter()
            .filter(|c| c.credential_type == cred_type)
            .collect()
    }

    /// Get the primary credential of a type.
    pub fn get_primary(&self, cred_type: CredentialType) -> Option<&LinkedCredential> {
        self.credentials
            .iter()
            .find(|c| c.credential_type == cred_type && c.is_primary)
    }
}

/// A linked external credential.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LinkedCredential {
    /// Credential type
    pub credential_type: CredentialType,

    /// Credential value (email address, phone number, etc.)
    pub value: String,

    /// Whether this credential is verified
    pub verified: bool,

    /// When the credential was linked
    pub linked_at: u64,

    /// When verification was completed
    pub verified_at: Option<u64>,

    /// Is this the primary credential of its type?
    pub is_primary: bool,
}

/// Types of linkable credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialType {
    /// Email address
    Email,
    /// Phone number
    Phone,
    /// OAuth provider (value = provider:subject)
    OAuth,
    /// WebAuthn passkey (value = credential ID)
    WebAuthn,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_store_paths() {
        let user_id = 0x12345678_9abcdef0_12345678_9abcdef0u128;
        let path = LocalKeyStore::storage_path(user_id);
        assert!(path.ends_with("/public_keys.json"));
    }

    #[test]
    fn test_machine_capabilities() {
        let caps = MachineKeyCapabilities::default();
        assert!(caps.can_authenticate);
        assert!(caps.can_encrypt);
        assert!(!caps.can_sign_messages);
        assert!(!caps.is_expired(1000));

        let full = MachineKeyCapabilities::full();
        assert!(full.can_authorize_machines);
        assert!(full.can_revoke_machines);
    }

    #[test]
    fn test_credential_store() {
        let mut store = CredentialStore::new(1);

        store.add(LinkedCredential {
            credential_type: CredentialType::Email,
            value: String::from("user@example.com"),
            verified: true,
            linked_at: 1000,
            verified_at: Some(2000),
            is_primary: true,
        });

        let emails = store.find_by_type(CredentialType::Email);
        assert_eq!(emails.len(), 1);

        let primary = store.get_primary(CredentialType::Email);
        assert!(primary.is_some());
        assert_eq!(primary.unwrap().value, "user@example.com");
    }
}
