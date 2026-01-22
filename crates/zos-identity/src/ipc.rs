//! IPC protocol definitions for the Identity layer.
//!
//! Defines message types for inter-process communication.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::error::{CredentialError, SessionError, UserError};
use crate::session::SessionId;
use crate::types::{User, UserId, UserStatus};

/// User service IPC message types.
pub mod user_msg {
    // User Management
    /// Create user request
    pub const MSG_CREATE_USER: u32 = 0x7000;
    /// Create user response
    pub const MSG_CREATE_USER_RESPONSE: u32 = 0x7001;
    /// Get user request
    pub const MSG_GET_USER: u32 = 0x7002;
    /// Get user response
    pub const MSG_GET_USER_RESPONSE: u32 = 0x7003;
    /// List users request
    pub const MSG_LIST_USERS: u32 = 0x7004;
    /// List users response
    pub const MSG_LIST_USERS_RESPONSE: u32 = 0x7005;
    /// Delete user request
    pub const MSG_DELETE_USER: u32 = 0x7006;
    /// Delete user response
    pub const MSG_DELETE_USER_RESPONSE: u32 = 0x7007;

    // Local Login (Offline)
    /// Login challenge request
    pub const MSG_LOGIN_CHALLENGE: u32 = 0x7010;
    /// Login challenge response
    pub const MSG_LOGIN_CHALLENGE_RESPONSE: u32 = 0x7011;
    /// Login verify request
    pub const MSG_LOGIN_VERIFY: u32 = 0x7012;
    /// Login verify response
    pub const MSG_LOGIN_VERIFY_RESPONSE: u32 = 0x7013;
    /// Logout request
    pub const MSG_LOGOUT: u32 = 0x7014;
    /// Logout response
    pub const MSG_LOGOUT_RESPONSE: u32 = 0x7015;

    // Remote Authentication
    /// Remote auth request
    pub const MSG_REMOTE_AUTH: u32 = 0x7020;
    /// Remote auth response
    pub const MSG_REMOTE_AUTH_RESPONSE: u32 = 0x7021;

    // Process Queries
    /// Whoami request
    pub const MSG_WHOAMI: u32 = 0x7030;
    /// Whoami response
    pub const MSG_WHOAMI_RESPONSE: u32 = 0x7031;

    // Credential Management
    /// Attach email request
    pub const MSG_ATTACH_EMAIL: u32 = 0x7040;
    /// Attach email response
    pub const MSG_ATTACH_EMAIL_RESPONSE: u32 = 0x7041;
    /// Get credentials request
    pub const MSG_GET_CREDENTIALS: u32 = 0x7042;
    /// Get credentials response
    pub const MSG_GET_CREDENTIALS_RESPONSE: u32 = 0x7043;
}

/// Permission service IPC message types.
pub mod perm_msg {
    /// Check permission request
    pub const MSG_CHECK_PERM: u32 = 0x5000;
    /// Check permission response
    pub const MSG_CHECK_PERM_RESPONSE: u32 = 0x5001;

    /// Query capabilities request
    pub const MSG_QUERY_CAPS: u32 = 0x5002;
    /// Query capabilities response
    pub const MSG_QUERY_CAPS_RESPONSE: u32 = 0x5003;

    /// Query history request
    pub const MSG_QUERY_HISTORY: u32 = 0x5004;
    /// Query history response
    pub const MSG_QUERY_HISTORY_RESPONSE: u32 = 0x5005;

    /// Get provenance request
    pub const MSG_GET_PROVENANCE: u32 = 0x5006;
    /// Get provenance response
    pub const MSG_GET_PROVENANCE_RESPONSE: u32 = 0x5007;

    /// Update policy request (admin only)
    pub const MSG_UPDATE_POLICY: u32 = 0x5008;
    /// Update policy response
    pub const MSG_UPDATE_POLICY_RESPONSE: u32 = 0x5009;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_constants() {
        // Ensure no overlapping message IDs between modules
        assert!(user_msg::MSG_CREATE_USER != perm_msg::MSG_CHECK_PERM);
        assert!(user_msg::MSG_WHOAMI > user_msg::MSG_LOGOUT);
    }

    #[test]
    fn test_request_serialization() {
        let req = CreateUserRequest {
            display_name: String::from("Test User"),
        };

        // This would need serde_json for full test, just check it compiles
        let _ = req.display_name.len();
    }
}
