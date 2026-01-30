//! IPC protocol definitions for the Identity layer.
//!
//! Defines request/response types for inter-process communication.
//! Message constants are defined in `zos-ipc` (the single source of truth).

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::error::{CredentialError, KeyError, SessionError, UserError, ZidError};
use crate::keystore::{KeyScheme, LocalKeyStore, MachineKeyCapabilities, MachineKeyRecord};
use crate::serde_helpers::u128_hex_string;
use crate::session::SessionId;
use crate::types::{User, UserId, UserStatus};

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
    #[serde(with = "u128_hex_string")]
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
    #[serde(with = "u128_hex_string")]
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
    #[serde(with = "u128_hex_string")]
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
    #[serde(with = "u128_hex_string")]
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
///
/// Calls ZID API to attach email credential. Requires active ZID session.
/// Password is hashed server-side with Argon2id.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachEmailRequest {
    /// User ID
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// Email address to attach
    pub email: String,
    /// Password for ZID account (hashed server-side with Argon2id)
    pub password: String,
    /// JWT access token from ZID login
    pub access_token: String,
    /// ZID API endpoint (e.g., "https://api.zero-id.io")
    pub zid_endpoint: String,
}

/// Attach email response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttachEmailResponse {
    /// Result of the operation (simplified - no verification needed with ZID)
    pub result: Result<(), CredentialError>,
}

/// Get credentials request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetCredentialsRequest {
    /// User ID
    #[serde(with = "u128_hex_string")]
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

/// Unlink credential request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnlinkCredentialRequest {
    /// User ID
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// Type of credential to unlink
    pub credential_type: crate::keystore::CredentialType,
}

/// Unlink credential response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UnlinkCredentialResponse {
    /// Result of the operation
    pub result: Result<(), CredentialError>,
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
/// 4. Encrypt 2 shards with password, store to keystore
/// 5. Return 3 external shards + public identifiers
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GenerateNeuralKeyRequest {
    /// User ID to generate keys for
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// Password for encrypting 2 shards (minimum 12 characters)
    pub password: String,
}

/// Result of successful Neural Key generation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NeuralKeyGenerated {
    /// The derived user ID (first 128 bits of SHA-256 of identity signing public key)
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// Public identifiers (stored server-side)
    pub public_identifiers: PublicIdentifiers,
    /// External Shamir shards (3 of 5) - returned to UI for backup, NOT stored
    /// The other 2 shards are encrypted with the password and stored in keystore.
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
    #[serde(with = "u128_hex_string")]
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
    #[serde(with = "u128_hex_string")]
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
    #[serde(with = "u128_hex_string")]
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
/// Machine keys are derived from the user's Neural Key using:
/// - 1 external shard (from paper backup)
/// - Password (to decrypt 2 stored shards from keystore)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateMachineKeyRequest {
    /// User ID
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// Optional human-readable machine name
    pub machine_name: Option<String>,
    /// Machine key capabilities
    pub capabilities: MachineKeyCapabilities,
    /// Key scheme to use (defaults to Classical)
    #[serde(default)]
    pub key_scheme: KeyScheme,
    /// Single external Neural shard (from paper backup)
    pub external_shard: NeuralShard,
    /// Password to decrypt stored shards
    pub password: String,
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
    #[serde(with = "u128_hex_string")]
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
    #[serde(with = "u128_hex_string")]
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
    #[serde(with = "u128_hex_string")]
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
    #[serde(with = "u128_hex_string")]
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

/// Create machine key AND enroll with ZID in one atomic operation.
///
/// This combined endpoint solves the signature mismatch problem where
/// separate createMachineKey + enrollMachine calls would generate different keypairs.
/// With this combined flow:
/// 1. Reconstructs Neural Key from shards + password
/// 2. Derives machine keypair canonically
/// 3. Stores machine key record with SK seeds
/// 4. Enrolls with ZID using the same derived keypair
/// 5. Returns both MachineKeyRecord and ZidTokens
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateMachineKeyAndEnrollRequest {
    /// User ID
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// Optional human-readable machine name
    pub machine_name: Option<String>,
    /// Machine key capabilities
    pub capabilities: MachineKeyCapabilities,
    /// Key scheme to use (defaults to Classical)
    #[serde(default)]
    pub key_scheme: KeyScheme,
    /// Single external Neural shard (from paper backup)
    pub external_shard: NeuralShard,
    /// Password to decrypt stored shards
    pub password: String,
    /// ZID API endpoint (e.g., "https://api.zero-id.io")
    pub zid_endpoint: String,
}

/// Combined result of machine key creation and ZID enrollment.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachineKeyAndTokens {
    /// The created machine key record
    pub machine_key: MachineKeyRecord,
    /// ZID tokens from successful enrollment
    pub tokens: ZidTokens,
}

/// Create machine key and enroll response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateMachineKeyAndEnrollResponse {
    /// Result containing the machine key and tokens, or an error
    pub result: Result<MachineKeyAndTokens, ZidError>,
}

// ============================================================================
// ZID Auth Request/Response Types (0x7080-0x708F)
// ============================================================================

/// ZID login request (machine key challenge-response).
///
/// Initiates authentication with ZERO-ID server using local machine key:
/// 1. Service requests challenge from ZID server
/// 2. Signs challenge with machine key (Ed25519)
/// 3. Submits signed challenge for verification
/// 4. Receives tokens on success
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidLoginRequest {
    /// User ID whose machine key should be used
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// ZID API endpoint (e.g., "https://api.zero-id.io")
    pub zid_endpoint: String,
}

/// ZID login response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidLoginResponse {
    /// Result containing tokens or error
    pub result: Result<ZidTokens, ZidError>,
}

/// Login type indicating how the session was authenticated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginType {
    /// Authenticated via machine key challenge-response
    #[default]
    MachineKey,
    /// Authenticated via neural key
    NeuralKey,
    /// Authenticated via email/password
    Email,
    /// Authenticated via OAuth provider
    OAuth,
    /// Authenticated via WebAuthn/passkey
    WebAuthn,
    /// Authenticated via recovery flow
    Recovery,
}

/// Tokens returned from successful ZID authentication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidTokens {
    /// JWT access token for API calls
    pub access_token: String,
    /// Refresh token for obtaining new access tokens
    pub refresh_token: String,
    /// Unique session identifier
    pub session_id: String,
    /// Machine ID (UUID string)
    pub machine_id: String,
    /// When the access token expires (RFC3339 timestamp)
    pub expires_at: String,
    /// How this session was authenticated
    #[serde(default)]
    pub login_type: LoginType,
    /// Optional warning message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Persisted ZID session (stored in VFS).
///
/// Path: `/home/{user_id}/.zos/identity/zid_session.json`
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidSession {
    /// ZID API endpoint used for this session
    pub zid_endpoint: String,
    /// JWT access token
    pub access_token: String,
    /// Refresh token
    pub refresh_token: String,
    /// Session ID from ZID server
    pub session_id: String,
    /// Machine ID used for authentication (UUID string)
    #[serde(default)]
    pub machine_id: String,
    /// How this session was authenticated
    #[serde(default)]
    pub login_type: LoginType,
    /// When the access token expires (Unix timestamp ms)
    pub expires_at: u64,
    /// When this session was created (Unix timestamp ms)
    pub created_at: u64,
}

impl ZidSession {
    /// Path where ZID session is stored.
    pub fn storage_path(user_id: UserId) -> String {
        alloc::format!("/home/{}/.zos/identity/zid_session.json", user_id)
    }
}

// ============================================================================
// Identity Preferences (0x7090-0x7099)
// ============================================================================

/// Identity preferences stored in VFS.
///
/// Path: `/home/{user_id}/.zos/identity/preferences.json`
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityPreferences {
    /// Default key scheme for new machine keys
    #[serde(default)]
    pub default_key_scheme: KeyScheme,
    /// Default machine key ID for authentication.
    /// Auto-set when the first machine key is created.
    /// Used by ZID login to determine which machine key to authenticate with.
    #[serde(default)]
    pub default_machine_id: Option<u128>,
}

impl Default for IdentityPreferences {
    fn default() -> Self {
        Self {
            default_key_scheme: KeyScheme::Classical,
            default_machine_id: None,
        }
    }
}

impl IdentityPreferences {
    /// VFS path where preferences are stored
    pub fn storage_path(user_id: UserId) -> String {
        alloc::format!("/home/{}/.zos/identity/preferences.json", user_id)
    }
}

/// Get identity preferences request
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetIdentityPreferencesRequest {
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
}

/// Get identity preferences response
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetIdentityPreferencesResponse {
    pub preferences: IdentityPreferences,
}

/// Set default key scheme request
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetDefaultKeySchemeRequest {
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    pub key_scheme: KeyScheme,
}

/// Set default key scheme response
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetDefaultKeySchemeResponse {
    pub result: Result<(), KeyError>,
}

/// Set default machine key request.
///
/// Sets the default machine key ID for authentication.
/// This key will be used by ZID login.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetDefaultMachineKeyRequest {
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// Machine ID to set as default (hex string for JavaScript interop)
    #[serde(with = "u128_hex_string")]
    pub machine_id: u128,
}

/// Set default machine key response
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SetDefaultMachineKeyResponse {
    pub result: Result<(), KeyError>,
}

/// ZID token refresh request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidRefreshRequest {
    /// User ID
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// ZID API endpoint
    pub zid_endpoint: String,
}

/// ZID token refresh response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidRefreshResponse {
    /// Result containing new tokens or error
    pub result: Result<ZidTokens, ZidError>,
}

/// ZID enroll machine request (register with ZID server).
///
/// Registers a new identity + machine with the ZID server:
/// 1. Service reads local machine key from VFS
/// 2. Posts to /v1/identity with machine's public key
/// 3. Creates identity + first machine on ZID server
/// 4. Returns tokens on success (auto-login after enrollment)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidEnrollMachineRequest {
    /// User ID whose machine key should be enrolled
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// ZID API endpoint (e.g., "https://api.zero-id.io")
    pub zid_endpoint: String,
}

/// ZID enroll machine response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidEnrollMachineResponse {
    /// Result containing tokens or error
    pub result: Result<ZidTokens, ZidError>,
}

/// ZID logout request (delete session from VFS).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidLogoutRequest {
    /// User ID whose session should be cleared
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
}

/// ZID logout response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidLogoutResponse {
    /// Result of the operation
    pub result: Result<(), ZidError>,
}

/// ZID login with email/password request.
///
/// Authenticates with ZERO-ID server using email and password credentials.
/// This is an alternative to machine key challenge-response authentication.
///
/// # Safety Invariants (per zos-service.md Rule 0)
///
/// ## Success Conditions
/// - ZID returns valid tokens
/// - Session stored in VFS
/// - Tokens returned to caller
///
/// ## Acceptable Partial Failure
/// - Session write fails after ZID success (tokens still returned)
///
/// ## Forbidden States
/// - Returning success without ZID verification
/// - Processing without authorization check
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidEmailLoginRequest {
    /// User ID whose session should be created
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// Email address for authentication
    pub email: String,
    /// Password for authentication
    pub password: String,
    /// ZID API endpoint (e.g., "https://api.zero-id.io")
    pub zid_endpoint: String,
    /// Optional machine ID to associate with this session
    pub machine_id: Option<String>,
    /// Optional MFA code if MFA is enabled
    pub mfa_code: Option<String>,
}

/// ZID login with email/password response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidEmailLoginResponse {
    /// Result containing tokens or error
    pub result: Result<ZidTokens, ZidError>,
}

// ============================================================================
// ZID Server Enrollment Types (sent to ZID server)
// ============================================================================

/// Machine key structure for ZID server enrollment (simplified).
///
/// This is the format the ZID server expects when creating a new identity.
/// Only includes essential fields required for first-time registration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidMachineKey {
    /// Unique machine identifier (UUID format)
    pub machine_id: String,
    /// Ed25519 signing public key (hex encoded)
    pub signing_public_key: String,
    /// X25519 encryption public key (hex encoded)
    pub encryption_public_key: String,
    /// Capabilities of this machine key (e.g., ["SIGN", "ENCRYPT", "VAULT_OPERATIONS"])
    pub capabilities: Vec<String>,
    /// Human-readable device name (e.g., "Browser")
    pub device_name: String,
    /// Device platform (e.g., "web", "wasm32", "linux")
    pub device_platform: String,
}

/// Create identity request for ZID server enrollment.
///
/// This is the complete payload structure expected by the ZID server
/// when creating a new identity with its first machine.
///
/// The authorization_signature should be a signature over:
/// "create" + identity_id.bytes + machine_key.signing_public_key.bytes + created_at.bytes
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateIdentityRequest {
    /// New identity ID (UUID format, hyphenated)
    pub identity_id: String,
    /// Identity-level Ed25519 signing public key (hex encoded)
    pub identity_signing_public_key: String,
    /// Authorization signature proving ownership (hex encoded, signs "create" message)
    pub authorization_signature: String,
    /// First machine key for this identity
    pub machine_key: ZidMachineKey,
    /// Namespace name (e.g., "personal")
    pub namespace_name: String,
    /// Timestamp when identity was created (Unix seconds, not milliseconds)
    pub created_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn test_request_serialization() {
        let req = CreateUserRequest {
            display_name: String::from("Test User"),
        };

        // This would need serde_json for full test, just check it compiles
        let _ = req.display_name.len();
    }

    #[test]
    fn test_create_identity_serialization() {
        let request = CreateIdentityRequest {
            identity_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            identity_signing_public_key: "a1b2c3d4".to_string(),
            authorization_signature: "d3a4b5c6".to_string(),
            machine_key: ZidMachineKey {
                machine_id: "660e8400-e29b-41d4-a716-446655440001".to_string(),
                signing_public_key: "f0e1d2c3".to_string(),
                encryption_public_key: "01234567".to_string(),
                capabilities: alloc::vec!["SIGN".to_string()],
                device_name: "Browser".to_string(),
                device_platform: "web".to_string(),
            },
            namespace_name: "personal".to_string(),
            created_at: 1737504000,
        };

        // Verify all fields are present
        assert_eq!(request.identity_id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(request.namespace_name, "personal");
        assert_eq!(request.created_at, 1737504000);
        
        // Test JSON serialization
        let json = serde_json::to_string(&request).unwrap();
        
        // Verify identity_id is present in the JSON
        assert!(json.contains("\"identity_id\""));
        assert!(json.contains("550e8400-e29b-41d4-a716-446655440000"));
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
            key_scheme: crate::keystore::KeyScheme::default(),
            external_shard: NeuralShard { index: 1, hex: "abc123".to_string() },
            password: "secure-password-123".to_string(),
        };
        assert_eq!(create_req.user_id, 3);
        assert!(create_req.machine_name.is_some());
        assert_eq!(create_req.external_shard.index, 1);

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
