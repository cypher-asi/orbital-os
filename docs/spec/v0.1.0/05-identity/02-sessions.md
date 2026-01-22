# Sessions

> Local and remote session management for authenticated users.

## Overview

Sessions represent authenticated user contexts in ZOS. The session system is designed to be:

1. **Offline-first**: Local sessions work without network
2. **Multi-session**: Users can have multiple active sessions
3. **Optionally federated**: Sessions can link to remote authentication servers
4. **Process-aware**: Sessions track which processes belong to them

## Data Structures

### LocalSession

```rust
use uuid::Uuid;
use serde::{Serialize, Deserialize};

/// A local ZOS session - works fully offline.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalSession {
    /// Unique session identifier
    pub id: Uuid,
    
    /// User this session belongs to
    pub user_id: Uuid,
    
    /// Machine where session was created
    pub machine_id: Uuid,
    
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

impl LocalSession {
    /// Check if the session is expired.
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }
    
    /// Check if the session is active (not expired and has remote auth if required).
    pub fn is_active(&self, now: u64) -> bool {
        !self.is_expired(now)
    }
    
    /// Path where this session is stored.
    pub fn storage_path(&self) -> String {
        format!("/home/{}/.zos/sessions/{}.json", self.user_id, self.id)
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
}
```

### SessionMetadata

```rust
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
```

### RemoteAuthState

```rust
/// State for sessions linked to remote authentication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteAuthState {
    /// Remote authentication server endpoint
    pub server_endpoint: String,
    
    /// OAuth2/OIDC access token
    pub access_token: String,
    
    /// When the access token expires
    pub token_expires_at: u64,
    
    /// Refresh token (if available)
    pub refresh_token: Option<String>,
    
    /// Granted OAuth scopes
    pub scopes: Vec<String>,
    
    /// Token family ID for rotation tracking
    pub token_family_id: Uuid,
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
```

## Session Service

### Trait Definition

```rust
/// Service for managing user sessions.
pub trait SessionService {
    /// Create a new local session for a user.
    fn create_session(&self, user_id: Uuid) -> Result<LocalSession, SessionError>;
    
    /// Get a session by ID.
    fn get_session(&self, session_id: Uuid) -> Result<Option<LocalSession>, SessionError>;
    
    /// List all active sessions for a user.
    fn list_user_sessions(&self, user_id: Uuid) -> Result<Vec<LocalSession>, SessionError>;
    
    /// Validate a session (check expiry, update activity).
    fn validate_session(&self, session_id: Uuid) -> Result<bool, SessionError>;
    
    /// Refresh a session's expiry time.
    fn refresh_session(&self, session_id: Uuid) -> Result<(), SessionError>;
    
    /// End a session (logout).
    fn end_session(&self, session_id: Uuid) -> Result<(), SessionError>;
    
    /// End all sessions for a user.
    fn end_all_sessions(&self, user_id: Uuid) -> Result<u32, SessionError>;
    
    /// Link a session to remote authentication.
    fn link_remote_auth(&self, session_id: Uuid, auth: RemoteAuthState) -> Result<(), SessionError>;
    
    /// Refresh remote authentication tokens.
    fn refresh_remote_auth(&self, session_id: Uuid) -> Result<(), SessionError>;
}

/// Errors from session operations.
#[derive(Clone, Debug)]
pub enum SessionError {
    /// Session not found
    NotFound,
    /// Session has expired
    Expired,
    /// User not found
    UserNotFound,
    /// Remote authentication failed
    RemoteAuthFailed(String),
    /// Token refresh failed
    RefreshFailed(String),
    /// Storage error
    StorageError(String),
}
```

## Login Flow

### Local Login (Offline)

```
┌─────────────┐     ┌──────────────────┐     ┌─────────────────┐
│   Client    │     │ Identity Service │     │   VFS Service   │
└──────┬──────┘     └────────┬─────────┘     └────────┬────────┘
       │                     │                        │
       │ MSG_LOGIN_CHALLENGE │                        │
       │ (user_id)           │                        │
       │────────────────────▶│                        │
       │                     │                        │
       │                     │ Read user's public key │
       │                     │───────────────────────▶│
       │                     │◀───────────────────────│
       │                     │                        │
       │ Challenge           │                        │
       │ (random nonce)      │                        │
       │◀────────────────────│                        │
       │                     │                        │
       │ MSG_LOGIN_VERIFY    │                        │
       │ (signed challenge)  │                        │
       │────────────────────▶│                        │
       │                     │                        │
       │                     │ Verify signature       │
       │                     │───────────────────────▶│
       │                     │◀───────────────────────│
       │                     │                        │
       │                     │ Create session file    │
       │                     │───────────────────────▶│
       │                     │◀───────────────────────│
       │                     │                        │
       │ Session created     │                        │
       │ (session_id, token) │                        │
       │◀────────────────────│                        │
       │                     │                        │
```

### Implementation

```rust
impl SessionService for SessionServiceImpl {
    fn create_session(&self, user_id: Uuid) -> Result<LocalSession, SessionError> {
        // 1. Verify user exists
        let user = self.user_service.get_user(user_id)?
            .ok_or(SessionError::UserNotFound)?;
        
        // 2. Generate session
        let session_id = Uuid::new_v4();
        let now = current_timestamp();
        
        let session = LocalSession {
            id: session_id,
            user_id,
            machine_id: self.machine_id,
            created_at: now,
            expires_at: now + SESSION_DURATION_NANOS,
            process_ids: vec![],
            remote_auth: None,
            mfa_verified: false,
            capabilities: vec![],
            metadata: SessionMetadata {
                last_activity_at: now,
                ..Default::default()
            },
        };
        
        // 3. Store session file
        let session_json = serde_json::to_vec(&session)?;
        self.vfs.write_file(&session.storage_path(), &session_json)?;
        
        // 4. Update user status to Active
        self.user_service.update_status(user_id, UserStatus::Active)?;
        
        Ok(session)
    }
    
    fn end_session(&self, session_id: Uuid) -> Result<(), SessionError> {
        // 1. Get session
        let session = self.get_session(session_id)?
            .ok_or(SessionError::NotFound)?;
        
        // 2. Terminate all session processes
        for pid in &session.process_ids {
            let _ = self.process_manager.terminate(*pid);
        }
        
        // 3. Delete session file
        self.vfs.unlink(&session.storage_path())?;
        
        // 4. Check if user has other sessions
        let remaining = self.list_user_sessions(session.user_id)?;
        if remaining.is_empty() {
            self.user_service.update_status(session.user_id, UserStatus::Offline)?;
        }
        
        Ok(())
    }
}

/// Default session duration (24 hours in nanoseconds).
const SESSION_DURATION_NANOS: u64 = 24 * 60 * 60 * 1_000_000_000;
```

## Remote Authentication

### OAuth2/OIDC Flow

```rust
/// Request to link session to remote authentication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteAuthRequest {
    /// Session to link
    pub session_id: Uuid,
    /// Remote server endpoint
    pub server_endpoint: String,
    /// Authorization code from OAuth flow
    pub authorization_code: String,
    /// Code verifier for PKCE
    pub code_verifier: String,
    /// Redirect URI used in auth request
    pub redirect_uri: String,
}

impl SessionService for SessionServiceImpl {
    fn link_remote_auth(&self, session_id: Uuid, auth: RemoteAuthState) -> Result<(), SessionError> {
        // 1. Get and update session
        let mut session = self.get_session(session_id)?
            .ok_or(SessionError::NotFound)?;
        
        session.remote_auth = Some(auth);
        
        // 2. Store updated session
        let session_json = serde_json::to_vec(&session)?;
        self.vfs.write_file(&session.storage_path(), &session_json)?;
        
        Ok(())
    }
    
    fn refresh_remote_auth(&self, session_id: Uuid) -> Result<(), SessionError> {
        let mut session = self.get_session(session_id)?
            .ok_or(SessionError::NotFound)?;
        
        let auth = session.remote_auth.as_ref()
            .ok_or(SessionError::RemoteAuthFailed("No remote auth".into()))?;
        
        let refresh_token = auth.refresh_token.as_ref()
            .ok_or(SessionError::RefreshFailed("No refresh token".into()))?;
        
        // Call remote server to refresh token
        let new_auth = self.network.refresh_oauth_token(
            &auth.server_endpoint,
            refresh_token,
            &auth.token_family_id,
        )?;
        
        session.remote_auth = Some(new_auth);
        
        let session_json = serde_json::to_vec(&session)?;
        self.vfs.write_file(&session.storage_path(), &session_json)?;
        
        Ok(())
    }
}
```

## IPC Protocol

### Login Challenge

```rust
/// Login challenge request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginChallengeRequest {
    /// User ID attempting to login
    pub user_id: Uuid,
}

/// Login challenge response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginChallengeResponse {
    /// Challenge nonce to sign
    pub challenge: [u8; 32],
    /// Challenge expiry (nanos since epoch)
    pub expires_at: u64,
}
```

### Login Verify

```rust
/// Login verify request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginVerifyRequest {
    /// User ID
    pub user_id: Uuid,
    /// Signed challenge
    pub signature: Vec<u8>,
    /// Original challenge (for verification)
    pub challenge: [u8; 32],
}

/// Login verify response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginVerifyResponse {
    pub result: Result<LoginSuccess, SessionError>,
}

/// Successful login result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LoginSuccess {
    /// Created session ID
    pub session_id: Uuid,
    /// Session token for subsequent requests
    pub session_token: String,
    /// Session expiry time
    pub expires_at: u64,
}
```

### Logout

```rust
/// Logout request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogoutRequest {
    /// Session ID to end
    pub session_id: Uuid,
}

/// Logout response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogoutResponse {
    pub result: Result<(), SessionError>,
}
```

### Whoami

```rust
/// Whoami request (query current session info).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WhoamiRequest {
    // Empty - uses caller's process context
}

/// Whoami response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WhoamiResponse {
    /// User ID (if authenticated)
    pub user_id: Option<Uuid>,
    /// Session ID (if authenticated)
    pub session_id: Option<Uuid>,
    /// User display name
    pub display_name: Option<String>,
    /// Session capabilities
    pub capabilities: Vec<String>,
}
```

## Session Storage

Sessions are stored as files in the user's home directory:

```
/home/{user_id}/.zos/sessions/
├── {session_id_1}.json
├── {session_id_2}.json
└── {session_id_3}.json
```

Each session file contains the serialized `LocalSession` struct.

## Session Cleanup

### Expired Session Cleanup

```rust
impl SessionService for SessionServiceImpl {
    /// Clean up expired sessions for all users.
    fn cleanup_expired_sessions(&self) -> Result<u32, SessionError> {
        let now = current_timestamp();
        let mut cleaned = 0;
        
        // Iterate all users
        for user in self.user_service.list_users()? {
            let sessions_dir = format!("/home/{}/.zos/sessions", user.id);
            
            for entry in self.vfs.readdir(&sessions_dir)? {
                let path = format!("{}/{}", sessions_dir, entry.name);
                let data = self.vfs.read_file(&path)?;
                let session: LocalSession = serde_json::from_slice(&data)?;
                
                if session.is_expired(now) {
                    self.end_session(session.id)?;
                    cleaned += 1;
                }
            }
        }
        
        Ok(cleaned)
    }
}
```

## Invariants

1. **Session ownership**: Sessions are owned by exactly one user
2. **Machine binding**: Sessions are bound to the machine where created
3. **Expiry**: All sessions have a finite expiry time
4. **File storage**: Session files exist for active sessions only
5. **Process tracking**: Session process list reflects actual running processes

## Security Considerations

1. **Challenge-response**: Local login uses cryptographic challenge-response
2. **Token rotation**: Remote auth tokens are rotated on refresh
3. **Session isolation**: Sessions cannot access each other's capabilities
4. **Activity tracking**: Last activity time enables idle timeout
5. **Concurrent sessions**: Support for multiple sessions per user (with limits)

## WASM Notes

- Session tokens are generated using `crypto.getRandomValues()`
- Session files are stored in the `zos-userspace` IndexedDB database via VFS
- Challenge signatures use SubtleCrypto Ed25519 (or fallback library)

## Related Specifications

- [01-users.md](01-users.md) - User management
- [03-zero-id.md](03-zero-id.md) - Cryptographic keys for authentication
- [04-permissions.md](04-permissions.md) - Session capability grants
