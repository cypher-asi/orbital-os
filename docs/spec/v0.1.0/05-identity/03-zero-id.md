# Zero-ID Integration

> Cryptographic identity material storage and key management.

## Overview

Zero-ID provides the cryptographic foundation for ZOS user identities. All identity material is stored as files within the user's home directory, enabling:

1. **Portability**: Identity can be backed up and restored
2. **Multi-device**: Identity can sync across machines
3. **Transparency**: Identity state is inspectable as files
4. **Offline operation**: All cryptographic operations work offline

## Storage Paths

| Data | Path | Encrypted | Description |
|------|------|-----------|-------------|
| User record | `/home/{id}/.zos/identity/user.json` | No | User metadata |
| Public keys | `/home/{id}/.zos/identity/public_keys.json` | No | Public key material |
| Private keys | `/home/{id}/.zos/identity/private_keys.enc` | Yes | Encrypted private keys |
| Machine keys | `/home/{id}/.zos/identity/machine/{machine_id}.json` | Partial | Per-machine key material |
| Sessions | `/home/{id}/.zos/sessions/{session_id}.json` | No | Active sessions |
| Credentials | `/home/{id}/.zos/credentials/credentials.json` | Partial | Linked external credentials |
| Tokens | `/home/{id}/.zos/tokens/{family_id}.json` | No | Token families |

## Data Structures

### LocalKeyStore

```rust
use uuid::Uuid;
use serde::{Serialize, Deserialize};

/// Local storage for user cryptographic material (public keys).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalKeyStore {
    /// User ID this key store belongs to
    pub user_id: Uuid,
    
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
    pub fn storage_path(user_id: Uuid) -> String {
        format!("/home/{}/.zos/identity/public_keys.json", user_id)
    }
}
```

### KeyScheme

```rust
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
```

### MachineKeyCapabilities

```rust
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
```

### EncryptedPrivateKeys

```rust
/// Encrypted private key storage.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedPrivateKeys {
    /// Encryption algorithm used
    pub algorithm: String,  // "AES-256-GCM"
    
    /// Key derivation function
    pub kdf: KeyDerivation,
    
    /// Encrypted key bundle
    pub ciphertext: Vec<u8>,
    
    /// Nonce/IV for decryption
    pub nonce: [u8; 12],
    
    /// Authentication tag
    pub tag: [u8; 16],
}

/// Key derivation parameters.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyDerivation {
    /// KDF algorithm
    pub algorithm: String,  // "Argon2id"
    
    /// Salt for KDF
    pub salt: [u8; 32],
    
    /// Time cost (iterations)
    pub time_cost: u32,
    
    /// Memory cost (KB)
    pub memory_cost: u32,
    
    /// Parallelism
    pub parallelism: u32,
}

impl EncryptedPrivateKeys {
    /// Path where encrypted keys are stored.
    pub fn storage_path(user_id: Uuid) -> String {
        format!("/home/{}/.zos/identity/private_keys.enc", user_id)
    }
}
```

### MachineKeyRecord

```rust
/// Per-machine key record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachineKeyRecord {
    /// Machine ID
    pub machine_id: Uuid,
    
    /// Machine-specific signing public key
    pub signing_public_key: [u8; 32],
    
    /// Machine-specific encryption public key
    pub encryption_public_key: [u8; 32],
    
    /// When this machine was authorized
    pub authorized_at: u64,
    
    /// Who authorized this machine (user_id or machine_id)
    pub authorized_by: Uuid,
    
    /// Machine capabilities
    pub capabilities: MachineKeyCapabilities,
    
    /// Human-readable machine name
    pub machine_name: Option<String>,
    
    /// Last seen timestamp
    pub last_seen_at: u64,
}

impl MachineKeyRecord {
    /// Path where machine key is stored.
    pub fn storage_path(user_id: Uuid, machine_id: Uuid) -> String {
        format!("/home/{}/.zos/identity/machine/{}.json", user_id, machine_id)
    }
}
```

### Credentials

```rust
/// Linked external credentials.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CredentialStore {
    /// User ID
    pub user_id: Uuid,
    
    /// Linked credentials
    pub credentials: Vec<LinkedCredential>,
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

impl CredentialStore {
    /// Path where credentials are stored.
    pub fn storage_path(user_id: Uuid) -> String {
        format!("/home/{}/.zos/credentials/credentials.json", user_id)
    }
}
```

### TokenFamily

```rust
/// Token family for refresh token rotation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenFamily {
    /// Family ID
    pub id: Uuid,
    
    /// User this family belongs to
    pub user_id: Uuid,
    
    /// Remote server this family is for
    pub server_endpoint: String,
    
    /// Current token generation
    pub generation: u64,
    
    /// When this family was created
    pub created_at: u64,
    
    /// Last token refresh time
    pub last_refresh_at: u64,
    
    /// Is this family revoked?
    pub revoked: bool,
}

impl TokenFamily {
    /// Path where token family is stored.
    pub fn storage_path(user_id: Uuid, family_id: Uuid) -> String {
        format!("/home/{}/.zos/tokens/{}.json", user_id, family_id)
    }
}
```

## Key Operations

### Key Service Trait

```rust
/// Service for cryptographic key operations.
pub trait KeyService {
    /// Generate a new identity key pair.
    fn generate_identity_keys(&self, user_id: Uuid, passphrase: &str) -> Result<LocalKeyStore, KeyError>;
    
    /// Generate machine-specific keys.
    fn generate_machine_keys(&self, user_id: Uuid, passphrase: &str) -> Result<MachineKeyRecord, KeyError>;
    
    /// Load public keys for a user.
    fn load_public_keys(&self, user_id: Uuid) -> Result<LocalKeyStore, KeyError>;
    
    /// Unlock private keys with passphrase.
    fn unlock_private_keys(&self, user_id: Uuid, passphrase: &str) -> Result<UnlockedKeys, KeyError>;
    
    /// Sign data with identity key.
    fn sign_with_identity(&self, keys: &UnlockedKeys, data: &[u8]) -> Result<Vec<u8>, KeyError>;
    
    /// Sign data with machine key.
    fn sign_with_machine(&self, keys: &UnlockedKeys, data: &[u8]) -> Result<Vec<u8>, KeyError>;
    
    /// Verify a signature against identity public key.
    fn verify_identity_signature(&self, user_id: Uuid, data: &[u8], signature: &[u8]) -> Result<bool, KeyError>;
    
    /// Encrypt data for this user (to self).
    fn encrypt_to_self(&self, keys: &UnlockedKeys, plaintext: &[u8]) -> Result<Vec<u8>, KeyError>;
    
    /// Decrypt data encrypted to this user.
    fn decrypt_from_self(&self, keys: &UnlockedKeys, ciphertext: &[u8]) -> Result<Vec<u8>, KeyError>;
    
    /// Rotate keys (create new epoch).
    fn rotate_keys(&self, user_id: Uuid, old_passphrase: &str, new_passphrase: &str) -> Result<u64, KeyError>;
}

/// Unlocked private keys (in memory only).
pub struct UnlockedKeys {
    pub user_id: Uuid,
    pub identity_signing_key: [u8; 32],
    pub machine_signing_key: [u8; 32],
    pub machine_encryption_key: [u8; 32],
    pub scheme: KeyScheme,
    // PQ keys if applicable
    pub pq_signing_key: Option<Vec<u8>>,
    pub pq_encryption_key: Option<Vec<u8>>,
}

/// Errors from key operations.
#[derive(Clone, Debug)]
pub enum KeyError {
    /// User not found
    UserNotFound,
    /// Keys not found
    KeysNotFound,
    /// Invalid passphrase
    InvalidPassphrase,
    /// Key derivation failed
    DerivationFailed,
    /// Encryption/decryption failed
    CryptoError(String),
    /// Storage error
    StorageError(String),
}
```

### Key Generation

```rust
impl KeyService for KeyServiceImpl {
    fn generate_identity_keys(&self, user_id: Uuid, passphrase: &str) -> Result<LocalKeyStore, KeyError> {
        // 1. Generate identity key pair
        let identity_signing_keypair = ed25519_generate_keypair();
        
        // 2. Generate machine key pairs
        let machine_signing_keypair = ed25519_generate_keypair();
        let machine_encryption_keypair = x25519_generate_keypair();
        
        // 3. Create public key store
        let key_store = LocalKeyStore {
            user_id,
            identity_signing_public_key: identity_signing_keypair.public,
            machine_signing_public_key: machine_signing_keypair.public,
            machine_encryption_public_key: machine_encryption_keypair.public,
            key_scheme: KeyScheme::Ed25519X25519,
            capabilities: MachineKeyCapabilities::default(),
            epoch: 1,
            pq_signing_public_key: None,
            pq_encryption_public_key: None,
        };
        
        // 4. Store public keys
        let public_json = serde_json::to_vec(&key_store)?;
        self.vfs.write_file(&LocalKeyStore::storage_path(user_id), &public_json)?;
        
        // 5. Encrypt and store private keys
        let private_bundle = PrivateKeyBundle {
            identity_signing_key: identity_signing_keypair.secret,
            machine_signing_key: machine_signing_keypair.secret,
            machine_encryption_key: machine_encryption_keypair.secret,
        };
        
        let encrypted = self.encrypt_private_keys(&private_bundle, passphrase)?;
        let encrypted_json = serde_json::to_vec(&encrypted)?;
        self.vfs.write_file(&EncryptedPrivateKeys::storage_path(user_id), &encrypted_json)?;
        
        Ok(key_store)
    }
    
    fn encrypt_private_keys(&self, keys: &PrivateKeyBundle, passphrase: &str) -> Result<EncryptedPrivateKeys, KeyError> {
        // 1. Generate salt
        let mut salt = [0u8; 32];
        random_bytes(&mut salt);
        
        // 2. Derive encryption key from passphrase
        let kdf = KeyDerivation {
            algorithm: "Argon2id".to_string(),
            salt,
            time_cost: 3,
            memory_cost: 65536,
            parallelism: 1,
        };
        
        let encryption_key = argon2id_derive(
            passphrase.as_bytes(),
            &kdf.salt,
            kdf.time_cost,
            kdf.memory_cost,
            kdf.parallelism,
        );
        
        // 3. Serialize private keys
        let plaintext = serde_json::to_vec(keys)?;
        
        // 4. Encrypt with AES-256-GCM
        let mut nonce = [0u8; 12];
        random_bytes(&mut nonce);
        
        let (ciphertext, tag) = aes_256_gcm_encrypt(&encryption_key, &nonce, &plaintext)?;
        
        Ok(EncryptedPrivateKeys {
            algorithm: "AES-256-GCM".to_string(),
            kdf,
            ciphertext,
            nonce,
            tag,
        })
    }
}
```

## IPC Protocol

### Get Credentials

```rust
/// Get credentials request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetCredentialsRequest {
    /// User ID
    pub user_id: Uuid,
    /// Optional filter by credential type
    pub credential_type: Option<CredentialType>,
}

/// Get credentials response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetCredentialsResponse {
    pub credentials: Vec<LinkedCredential>,
}
```

### Attach Email

```rust
/// Attach email credential request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachEmailRequest {
    /// User ID
    pub user_id: Uuid,
    /// Email address to attach
    pub email: String,
}

/// Attach email response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachEmailResponse {
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

/// Errors from credential operations.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CredentialError {
    /// Credential already linked
    AlreadyLinked,
    /// Invalid credential format
    InvalidFormat,
    /// Verification failed
    VerificationFailed,
    /// Storage error
    StorageError(String),
}
```

## Cryptographic Algorithms

| Purpose | Algorithm | Key Size |
|---------|-----------|----------|
| Identity signing | Ed25519 | 32 bytes |
| Machine signing | Ed25519 | 32 bytes |
| Encryption | X25519 + AES-256-GCM | 32 bytes + 12 nonce |
| Key derivation | Argon2id | 32 bytes output |
| PQ signing (optional) | Dilithium3 | ~2.5KB |
| PQ encryption (optional) | Kyber1024 | ~1.5KB |

## Invariants

1. **Key existence**: Every user has public keys stored
2. **Key consistency**: Private keys decrypt with correct passphrase
3. **Machine binding**: Machine keys are specific to one machine
4. **Epoch monotonic**: Key epochs only increase
5. **Credential uniqueness**: Each credential value is unique per user

## Security Considerations

1. **Passphrase security**: Private keys require passphrase to decrypt
2. **Key isolation**: Private keys never leave memory unencrypted
3. **Salt uniqueness**: Each encrypted key file has unique salt
4. **No key reuse**: Different key pairs for signing vs encryption
5. **Post-quantum ready**: Optional PQ keys for future-proofing

## WASM Notes

- Ed25519 operations use `@noble/ed25519` or SubtleCrypto
- X25519 operations use `@noble/curves`
- AES-256-GCM uses SubtleCrypto `encrypt`/`decrypt`
- Argon2id uses `argon2-browser` WebAssembly implementation
- Key material is stored in `zos-userspace` IndexedDB via VFS

## Related Specifications

- [01-users.md](01-users.md) - User management
- [02-sessions.md](02-sessions.md) - Session authentication
- [../06-filesystem/03-storage.md](../06-filesystem/03-storage.md) - Encrypted file storage
