//! Session management types for the Identity layer.
//!
//! Sessions represent authenticated user contexts in ZOS.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::types::UserId;

/// A unique session identifier (UUID as 128-bit value).
pub type SessionId = u128;

/// A unique machine identifier (UUID as 128-bit value).
pub type MachineId = u128;

/// A local ZOS session - works fully offline.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalSession {
    /// Unique session identifier
    pub id: SessionId,

    /// User this session belongs to
    pub user_id: UserId,

    /// Machine where session was created
    pub machine_id: MachineId,

    /// When the session was created (nanos since epoch)
    pub created_at: u64,

    /// When the session expires (nanos since epoch)
    pub expires_at: u64,

    /// Processes running in this session
    pub process_ids: Vec<u32>,

    /// Optional remote authentication state
    pub remote_auth: Option<RemoteAuthState>,

    /// Whether MFA has been verified this session
    pub mfa_verified: bool,

    /// Capabilities granted to this session
    pub capabilities: Vec<String>,

    /// Additional session metadata
    pub metadata: SessionMetadata,
}

/// Default session duration (24 hours in nanoseconds).
pub const SESSION_DURATION_NANOS: u64 = 24 * 60 * 60 * 1_000_000_000;

impl LocalSession {
    /// Create a new session with default expiration.
    pub fn new(user_id: UserId, machine_id: MachineId, now: u64) -> Self {
        Self {
            id: 0, // Should be set by caller with UUID generation
            user_id,
            machine_id,
            created_at: now,
            expires_at: now + SESSION_DURATION_NANOS,
            process_ids: Vec::new(),
            remote_auth: None,
            mfa_verified: false,
            capabilities: Vec::new(),
            metadata: SessionMetadata::new(now),
        }
    }

    /// Check if the session is expired.
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }

    /// Check if the session is active (not expired).
    pub fn is_active(&self, now: u64) -> bool {
        !self.is_expired(now)
    }

    /// Path where this session is stored.
    pub fn storage_path(&self) -> String {
        alloc::format!(
            "/home/{:032x}/.zos/sessions/{:032x}.json",
            self.user_id, self.id
        )
    }

    /// Add a process to this session.
    pub fn add_process(&mut self, pid: u32) {
        if !self.process_ids.contains(&pid) {
            self.process_ids.push(pid);
        }
    }

    /// Remove a process from this session.
    pub fn remove_process(&mut self, pid: u32) {
        self.process_ids.retain(|&p| p != pid);
    }

    /// Extend the session expiration.
    pub fn extend(&mut self, now: u64) {
        self.expires_at = now + SESSION_DURATION_NANOS;
        self.metadata.last_activity_at = now;
    }

    /// Update last activity timestamp.
    pub fn touch(&mut self, now: u64) {
        self.metadata.last_activity_at = now;
    }
}

/// Metadata about a session.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// IP address of the client (if known)
    pub ip_address: Option<String>,

    /// User agent string (if from browser)
    pub user_agent: Option<String>,

    /// Location hint (city, country)
    pub location_hint: Option<String>,

    /// Last activity timestamp
    pub last_activity_at: u64,

    /// Number of authentication attempts
    pub auth_attempts: u32,

    /// Custom metadata key-value pairs
    pub custom: BTreeMap<String, String>,
}

impl SessionMetadata {
    /// Create new metadata with the given timestamp.
    pub fn new(now: u64) -> Self {
        Self {
            ip_address: None,
            user_agent: None,
            location_hint: None,
            last_activity_at: now,
            auth_attempts: 0,
            custom: BTreeMap::new(),
        }
    }
}

/// State for sessions linked to remote authentication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteAuthState {
    /// Remote authentication server endpoint
    pub server_endpoint: String,

    /// OAuth2/OIDC access token
    pub access_token: String,

    /// When the access token expires (nanos since epoch)
    pub token_expires_at: u64,

    /// Refresh token (if available)
    pub refresh_token: Option<String>,

    /// Granted OAuth scopes
    pub scopes: Vec<String>,

    /// Token family ID for rotation tracking
    pub token_family_id: u128,
}

impl RemoteAuthState {
    /// Check if the access token is expired.
    pub fn is_token_expired(&self, now: u64) -> bool {
        now >= self.token_expires_at
    }

    /// Check if the token can be refreshed.
    pub fn can_refresh(&self) -> bool {
        self.refresh_token.is_some()
    }
}

/// Token family for refresh token rotation tracking.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TokenFamily {
    /// Family ID
    pub id: u128,

    /// User this family belongs to
    pub user_id: UserId,

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
    pub fn storage_path(user_id: UserId, family_id: u128) -> String {
        alloc::format!(
            "/home/{:032x}/.zos/tokens/{:032x}.json",
            user_id, family_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_expiration() {
        let now = 1_000_000_000u64;
        let session = LocalSession::new(1, 1, now);

        assert!(!session.is_expired(now));
        assert!(session.is_active(now));

        // After 25 hours, should be expired
        let future = now + 25 * 60 * 60 * 1_000_000_000;
        assert!(session.is_expired(future));
        assert!(!session.is_active(future));
    }

    #[test]
    fn test_session_processes() {
        let mut session = LocalSession::new(1, 1, 0);

        session.add_process(100);
        session.add_process(101);
        session.add_process(100); // Duplicate, should not add

        assert_eq!(session.process_ids.len(), 2);

        session.remove_process(100);
        assert_eq!(session.process_ids.len(), 1);
        assert!(!session.process_ids.contains(&100));
    }

    #[test]
    fn test_remote_auth_expiration() {
        let auth = RemoteAuthState {
            server_endpoint: String::from("https://auth.example.com"),
            access_token: String::from("token"),
            token_expires_at: 1_000_000_000,
            refresh_token: Some(String::from("refresh")),
            scopes: Vec::new(),
            token_family_id: 1,
        };

        assert!(!auth.is_token_expired(500_000_000));
        assert!(auth.is_token_expired(1_500_000_000));
        assert!(auth.can_refresh());
    }
}
