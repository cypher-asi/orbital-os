//! Cryptographic key storage for the Identity layer.
//!
//! Provides types and operations for Zero-ID key management.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::types::UserId;

// ============================================================================
// Serde helpers for u128 as hex string (for JavaScript interop)
// ============================================================================

/// Serde module for serializing/deserializing u128 as hex string (e.g., "0x123abc")
/// Also accepts numbers for backward compatibility with existing stored data.
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

/// Serde module for serializing/deserializing Option<Vec<u8>> as hex string.
/// This is used for large byte vectors (like PQ keys) to avoid massive JSON arrays.
mod option_bytes_hex {
    use alloc::format;
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::fmt;
    use serde::{self, de, Deserializer, Serializer};

    pub fn serialize<S>(value: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(bytes) => {
                let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                serializer.serialize_some(&hex)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OptionBytesVisitor;

        impl<'de> de::Visitor<'de> for OptionBytesVisitor {
            type Value = Option<Vec<u8>>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string, byte array, or null")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(None)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserializer.deserialize_any(BytesVisitor).map(Some)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(None)
            }
        }

        struct BytesVisitor;

        impl<'de> de::Visitor<'de> for BytesVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string or byte array")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // Parse hex string
                let s = s.trim_start_matches("0x").trim_start_matches("0X");
                let mut bytes = Vec::with_capacity(s.len() / 2);
                let mut chars = s.chars();
                while let (Some(h), Some(l)) = (chars.next(), chars.next()) {
                    let byte = u8::from_str_radix(&format!("{}{}", h, l), 16)
                        .map_err(de::Error::custom)?;
                    bytes.push(byte);
                }
                Ok(bytes)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                // Parse array of numbers (backward compatibility)
                let mut bytes = Vec::new();
                while let Some(b) = seq.next_element::<u8>()? {
                    bytes.push(b);
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_option(OptionBytesVisitor)
    }
}

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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_bytes_hex"
    )]
    pub pq_signing_public_key: Option<Vec<u8>>,

    /// Post-quantum encryption public key (if PqHybrid scheme)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_bytes_hex"
    )]
    pub pq_encryption_public_key: Option<Vec<u8>>,

    /// Timestamp when the key was created (milliseconds since Unix epoch)
    #[serde(default)]
    pub created_at: u64,
}

impl LocalKeyStore {
    /// Path where public keys are stored.
    pub fn storage_path(user_id: UserId) -> String {
        alloc::format!("/home/{}/.zos/identity/public_keys.json", user_id)
    }

    /// Create a new key store with the given keys.
    pub fn new(
        user_id: UserId,
        identity_signing_public_key: [u8; 32],
        machine_signing_public_key: [u8; 32],
        machine_encryption_public_key: [u8; 32],
        created_at: u64,
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
            created_at,
        }
    }
}

/// Cryptographic key scheme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyScheme {
    /// Ed25519 signing + X25519 encryption (default)
    #[serde(alias = "Ed25519X25519")]
    #[default]
    Classical,

    /// Hybrid: Ed25519/X25519 + ML-DSA-65/ML-KEM-768 (post-quantum)
    #[serde(alias = "PqHybrid")]
    PqHybrid,
}

// ============================================================================
// Machine Key Capabilities (string array format)
// ============================================================================

/// Capability string constants for machine keys.
pub mod capability {
    /// Can sign authentication challenges
    pub const AUTHENTICATE: &str = "AUTHENTICATE";
    /// Can sign messages on behalf of user
    pub const SIGN: &str = "SIGN";
    /// Can encrypt/decrypt data
    pub const ENCRYPT: &str = "ENCRYPT";
    /// Can unwrap SVK (Storage Vault Key)
    pub const SVK_UNWRAP: &str = "SVK_UNWRAP";
    /// Can participate in MLS messaging
    pub const MLS_MESSAGING: &str = "MLS_MESSAGING";
    /// Can perform vault operations
    pub const VAULT_OPERATIONS: &str = "VAULT_OPERATIONS";
    /// Can authorize new machines
    pub const AUTHORIZE_MACHINES: &str = "AUTHORIZE_MACHINES";
    /// Can revoke other machines
    pub const REVOKE_MACHINES: &str = "REVOKE_MACHINES";
}

/// Capabilities of machine-level keys as a string array.
///
/// Modern format: `["AUTHENTICATE", "SIGN", "ENCRYPT", ...]`
///
/// Supports deserialization from legacy boolean struct format for backward compatibility.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(from = "CapabilitiesFormat")]
pub struct MachineKeyCapabilities {
    /// List of capability strings
    pub capabilities: Vec<String>,
    /// Expiry time (None = no expiry)
    #[serde(default)]
    pub expires_at: Option<u64>,
}

/// Internal format for deserializing capabilities from either format.
#[derive(Deserialize)]
#[serde(untagged)]
enum CapabilitiesFormat {
    /// New format: string array (wrapped in object with optional expires_at)
    Modern(ModernCapabilities),
    /// Legacy format: boolean struct
    Legacy(LegacyCapabilities),
    /// Direct string array (simplest case)
    Direct(Vec<String>),
}

#[derive(Deserialize)]
struct ModernCapabilities {
    capabilities: Vec<String>,
    #[serde(default)]
    expires_at: Option<u64>,
}

#[derive(Deserialize)]
struct LegacyCapabilities {
    can_authenticate: bool,
    can_encrypt: bool,
    can_sign_messages: bool,
    can_authorize_machines: bool,
    can_revoke_machines: bool,
    expires_at: Option<u64>,
}

impl From<CapabilitiesFormat> for MachineKeyCapabilities {
    fn from(format: CapabilitiesFormat) -> Self {
        match format {
            CapabilitiesFormat::Modern(m) => MachineKeyCapabilities {
                capabilities: m.capabilities,
                expires_at: m.expires_at,
            },
            CapabilitiesFormat::Direct(caps) => MachineKeyCapabilities {
                capabilities: caps,
                expires_at: None,
            },
            CapabilitiesFormat::Legacy(legacy) => {
                let mut caps = Vec::new();
                if legacy.can_authenticate {
                    caps.push(capability::AUTHENTICATE.into());
                }
                if legacy.can_encrypt {
                    caps.push(capability::ENCRYPT.into());
                }
                if legacy.can_sign_messages {
                    caps.push(capability::SIGN.into());
                }
                if legacy.can_authorize_machines {
                    caps.push(capability::AUTHORIZE_MACHINES.into());
                }
                if legacy.can_revoke_machines {
                    caps.push(capability::REVOKE_MACHINES.into());
                }
                MachineKeyCapabilities {
                    capabilities: caps,
                    expires_at: legacy.expires_at,
                }
            }
        }
    }
}

impl Default for MachineKeyCapabilities {
    fn default() -> Self {
        Self {
            capabilities: vec![capability::AUTHENTICATE.into(), capability::ENCRYPT.into()],
            expires_at: None,
        }
    }
}

impl MachineKeyCapabilities {
    /// Create capabilities with all permissions.
    pub fn full() -> Self {
        Self {
            capabilities: vec![
                capability::AUTHENTICATE.into(),
                capability::SIGN.into(),
                capability::ENCRYPT.into(),
                capability::AUTHORIZE_MACHINES.into(),
                capability::REVOKE_MACHINES.into(),
            ],
            expires_at: None,
        }
    }

    /// Create capabilities from a list of capability strings.
    pub fn from_strings(caps: Vec<String>) -> Self {
        Self {
            capabilities: caps,
            expires_at: None,
        }
    }

    /// Check if a capability is present.
    pub fn has(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }

    /// Check if the capabilities are expired.
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at.is_some_and(|exp| now >= exp)
    }

    // Legacy accessors for backward compatibility

    /// Can sign authentication challenges
    pub fn can_authenticate(&self) -> bool {
        self.has(capability::AUTHENTICATE)
    }

    /// Can encrypt/decrypt data
    pub fn can_encrypt(&self) -> bool {
        self.has(capability::ENCRYPT)
    }

    /// Can sign messages on behalf of user
    pub fn can_sign_messages(&self) -> bool {
        self.has(capability::SIGN)
    }

    /// Can authorize new machines
    pub fn can_authorize_machines(&self) -> bool {
        self.has(capability::AUTHORIZE_MACHINES)
    }

    /// Can revoke other machines
    pub fn can_revoke_machines(&self) -> bool {
        self.has(capability::REVOKE_MACHINES)
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
        alloc::format!("/home/{}/.zos/identity/private_keys.enc", user_id)
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
    /// Machine ID (hex string for JavaScript interop)
    #[serde(with = "u128_hex_string")]
    pub machine_id: u128,

    /// Machine-specific signing public key (Ed25519, 32 bytes)
    pub signing_public_key: [u8; 32],

    /// Machine-specific encryption public key (X25519, 32 bytes)
    pub encryption_public_key: [u8; 32],

    /// Signing secret key (32 bytes) - stored securely, used to reconstruct keypair for signing
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_sk: Option<[u8; 32]>,

    /// Encryption secret key (32 bytes) - stored securely, used to reconstruct keypair for encryption
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encryption_sk: Option<[u8; 32]>,

    /// When this machine was authorized
    pub authorized_at: u64,

    /// Who authorized this machine (user_id or machine_id, hex string for JavaScript interop)
    #[serde(with = "u128_hex_string")]
    pub authorized_by: u128,

    /// Machine capabilities
    pub capabilities: MachineKeyCapabilities,

    /// Human-readable machine name
    pub machine_name: Option<String>,

    /// Last seen timestamp
    pub last_seen_at: u64,

    /// Key epoch (increments on rotation)
    #[serde(default = "default_epoch")]
    pub epoch: u64,

    /// Key scheme used (Classical or PqHybrid)
    #[serde(default)]
    pub key_scheme: KeyScheme,

    /// ML-DSA-65 PQ signing public key (1952 bytes, only for PqHybrid)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_bytes_hex"
    )]
    pub pq_signing_public_key: Option<Vec<u8>>,

    /// ML-KEM-768 PQ encryption public key (1184 bytes, only for PqHybrid)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "option_bytes_hex"
    )]
    pub pq_encryption_public_key: Option<Vec<u8>>,
}

/// Default epoch value for backward compatibility with existing records
fn default_epoch() -> u64 {
    1
}

impl MachineKeyRecord {
    /// Path where machine key is stored.
    pub fn storage_path(user_id: UserId, machine_id: u128) -> String {
        alloc::format!(
            "/home/{}/.zos/identity/machine/{:032x}.json",
            user_id,
            machine_id
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
        alloc::format!("/home/{}/.zos/credentials/credentials.json", user_id)
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
        assert!(caps.can_authenticate());
        assert!(caps.can_encrypt());
        assert!(!caps.can_sign_messages());
        assert!(!caps.is_expired(1000));

        let full = MachineKeyCapabilities::full();
        assert!(full.can_authorize_machines());
        assert!(full.can_revoke_machines());
    }

    #[test]
    fn test_machine_capabilities_from_strings() {
        let caps = MachineKeyCapabilities::from_strings(vec!["AUTHENTICATE".into(), "SIGN".into()]);
        assert!(caps.can_authenticate());
        assert!(caps.can_sign_messages());
        assert!(!caps.can_encrypt());
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
