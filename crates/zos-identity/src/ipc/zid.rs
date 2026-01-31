//! ZID Auth Request/Response Types
//!
//! Handles authentication with ZERO-ID server including:
//! - Machine key login/enrollment
//! - Email/password login
//! - Token refresh
//! - Combined machine key + enrollment flow

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::error::ZidError;
use crate::keystore::{KeyScheme, MachineKeyCapabilities, MachineKeyRecord};
use crate::serde_helpers::u128_hex_string;
use crate::types::UserId;

use super::keys::NeuralShard;

extern crate alloc;

// ============================================================================
// Combined Machine Key + ZID Enrollment
// ============================================================================

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
// ZID Login/Enrollment
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
// Identity Preferences
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
    pub result: Result<(), crate::KeyError>,
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
    pub result: Result<(), crate::KeyError>,
}

// ============================================================================
// ZID Token Refresh
// ============================================================================

/// ZID token refresh request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidRefreshRequest {
    /// User ID
    #[serde(with = "u128_hex_string")]
    pub user_id: UserId,
    /// ZID API endpoint
    pub zid_endpoint: String,
    /// Optional refresh token from frontend (if provided, use instead of reading from VFS).
    /// This prevents race conditions when multiple refresh attempts occur before VFS is updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

/// ZID token refresh response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidRefreshResponse {
    /// Result containing new tokens or error
    pub result: Result<ZidTokens, ZidError>,
}

// ============================================================================
// ZID Enrollment
// ============================================================================

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

// ============================================================================
// ZID Email Login
// ============================================================================

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
        let req = super::super::user::CreateUserRequest {
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
}
