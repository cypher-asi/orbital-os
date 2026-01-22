//! Service traits for the Identity layer.
//!
//! Defines the interfaces for user and session management.

use alloc::vec::Vec;

use crate::error::{SessionError, UserError};
use crate::session::{LocalSession, RemoteAuthState, SessionId};
use crate::types::{User, UserId, UserPreferences, UserStatus};

/// Service for managing users.
pub trait UserService {
    /// Create a new user with the given display name.
    fn create_user(&self, display_name: &str) -> Result<User, UserError>;

    /// Get a user by ID.
    fn get_user(&self, user_id: UserId) -> Result<Option<User>, UserError>;

    /// Get a user by display name (may return multiple if not unique).
    fn find_users_by_name(&self, display_name: &str) -> Result<Vec<User>, UserError>;

    /// List all users on this machine.
    fn list_users(&self) -> Result<Vec<User>, UserError>;

    /// Update a user's display name.
    fn update_display_name(&self, user_id: UserId, new_name: &str) -> Result<(), UserError>;

    /// Update a user's status.
    fn update_status(&self, user_id: UserId, status: UserStatus) -> Result<(), UserError>;

    /// Delete a user and optionally their home directory.
    fn delete_user(&self, user_id: UserId, delete_home: bool) -> Result<(), UserError>;

    /// Get user preferences.
    fn get_preferences(&self, user_id: UserId) -> Result<UserPreferences, UserError>;

    /// Update user preferences.
    fn set_preferences(&self, user_id: UserId, prefs: &UserPreferences) -> Result<(), UserError>;
}

/// Service for managing user sessions.
pub trait SessionService {
    /// Create a new local session for a user.
    fn create_session(&self, user_id: UserId) -> Result<LocalSession, SessionError>;

    /// Get a session by ID.
    fn get_session(&self, session_id: SessionId) -> Result<Option<LocalSession>, SessionError>;

    /// List all active sessions for a user.
    fn list_user_sessions(&self, user_id: UserId) -> Result<Vec<LocalSession>, SessionError>;

    /// Validate a session (check expiry, update activity).
    fn validate_session(&self, session_id: SessionId) -> Result<bool, SessionError>;

    /// Refresh a session's expiry time.
    fn refresh_session(&self, session_id: SessionId) -> Result<(), SessionError>;

    /// End a session (logout).
    fn end_session(&self, session_id: SessionId) -> Result<(), SessionError>;

    /// End all sessions for a user.
    fn end_all_sessions(&self, user_id: UserId) -> Result<u32, SessionError>;

    /// Link a session to remote authentication.
    fn link_remote_auth(
        &self,
        session_id: SessionId,
        auth: RemoteAuthState,
    ) -> Result<(), SessionError>;

    /// Refresh remote authentication tokens.
    fn refresh_remote_auth(&self, session_id: SessionId) -> Result<(), SessionError>;

    /// Clean up expired sessions for all users.
    fn cleanup_expired_sessions(&self) -> Result<u32, SessionError>;

    /// Get the session for a process.
    fn get_process_session(&self, pid: u32) -> Result<Option<LocalSession>, SessionError>;
}

/// Service for cryptographic key operations.
pub trait KeyService {
    /// Generate a new identity key pair.
    fn generate_identity_keys(
        &self,
        user_id: UserId,
        passphrase: &str,
    ) -> Result<crate::keystore::LocalKeyStore, crate::error::KeyError>;

    /// Generate machine-specific keys.
    fn generate_machine_keys(
        &self,
        user_id: UserId,
        passphrase: &str,
    ) -> Result<crate::keystore::MachineKeyRecord, crate::error::KeyError>;

    /// Load public keys for a user.
    fn load_public_keys(
        &self,
        user_id: UserId,
    ) -> Result<crate::keystore::LocalKeyStore, crate::error::KeyError>;

    /// Verify a signature against identity public key.
    fn verify_identity_signature(
        &self,
        user_id: UserId,
        data: &[u8],
        signature: &[u8],
    ) -> Result<bool, crate::error::KeyError>;

    /// Rotate keys (create new epoch).
    fn rotate_keys(
        &self,
        user_id: UserId,
        old_passphrase: &str,
        new_passphrase: &str,
    ) -> Result<u64, crate::error::KeyError>;
}
