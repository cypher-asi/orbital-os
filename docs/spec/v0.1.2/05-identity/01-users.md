# Users

> The ZOS user primitive backed by Zero-ID identities.

## Overview

A ZOS user represents an identity that can:

1. Own files and directories
2. Have active sessions
3. Receive capability grants
4. Run applications

Users are backed by Zero-ID identities, ensuring cryptographic authenticity.

## Data Structures

### User

```rust
use uuid::Uuid;
use serde::{Serialize, Deserialize};

/// A ZOS user backed by a Zero-ID Identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    /// Local user ID (matches zero-id identity_id)
    pub id: Uuid,
    
    /// Display name for UI
    pub display_name: String,
    
    /// User status in the system
    pub status: UserStatus,
    
    /// Default namespace for this user's resources
    pub default_namespace_id: Uuid,
    
    /// When the user was created locally
    pub created_at: u64,
    
    /// Last activity timestamp
    pub last_active_at: u64,
}

impl User {
    /// Returns the user's home directory path.
    pub fn home_dir(&self) -> String {
        format!("/home/{}", self.id)
    }
    
    /// Returns the user's hidden ZOS directory path.
    pub fn zos_dir(&self) -> String {
        format!("/home/{}/.zos", self.id)
    }
    
    /// Returns the user's identity directory path.
    pub fn identity_dir(&self) -> String {
        format!("/home/{}/.zos/identity", self.id)
    }
    
    /// Returns the user's sessions directory path.
    pub fn sessions_dir(&self) -> String {
        format!("/home/{}/.zos/sessions", self.id)
    }
    
    /// Returns the user's app data directory.
    pub fn app_data_dir(&self, app_id: &str) -> String {
        format!("/home/{}/Apps/{}", self.id, app_id)
    }
}
```

### UserStatus

```rust
/// Status of a user account.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserStatus {
    /// User has at least one active local session
    Active,
    
    /// User exists but has no active sessions
    Offline,
    
    /// Account is suspended (cannot login)
    Suspended,
}
```

### UserPreferences

```rust
use alloc::collections::BTreeMap;

/// User preferences stored in the config directory.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserPreferences {
    /// UI theme name
    pub theme: Option<String>,
    
    /// Locale/language code (e.g., "en-US")
    pub locale: Option<String>,
    
    /// Wallpaper path (relative to home)
    pub wallpaper: Option<String>,
    
    /// Custom key-value preferences
    pub custom: BTreeMap<String, String>,
}

impl UserPreferences {
    /// Path where preferences are stored.
    pub fn storage_path(user_id: Uuid) -> String {
        format!("/home/{}/.zos/config/preferences.json", user_id)
    }
}
```

## Home Directory Structure

When a user is created, the system bootstraps their home directory:

```
/home/{user_id}/
├── .zos/                       # Hidden ZOS system data
│   ├── identity/               # Identity & cryptographic material
│   │   ├── user.json          # User record (this file)
│   │   ├── public_keys.json   # Public key material
│   │   ├── private_keys.enc   # Encrypted private keys
│   │   └── machine/           # Machine-specific keys
│   │       └── {machine_id}.json
│   ├── sessions/               # Active sessions
│   │   └── {session_id}.json
│   ├── credentials/            # Linked credentials
│   │   └── credentials.json
│   ├── tokens/                 # Token families
│   │   └── {family_id}.json
│   └── config/                 # User ZOS settings
│       └── preferences.json
├── Documents/                  # User documents
├── Downloads/                  # Downloaded files
├── Desktop/                    # Desktop items
├── Pictures/                   # Images
├── Music/                      # Audio files
└── Apps/                       # Per-app data directories
    └── {app_id}/
        ├── config/             # App configuration
        ├── data/               # App data
        └── cache/              # App cache (clearable)
```

## User Service

### Trait Definition

```rust
/// Service for managing users.
pub trait UserService {
    /// Create a new user with the given display name.
    fn create_user(&self, display_name: &str) -> Result<User, UserError>;
    
    /// Get a user by ID.
    fn get_user(&self, user_id: Uuid) -> Result<Option<User>, UserError>;
    
    /// Get a user by display name (may return multiple if not unique).
    fn find_users_by_name(&self, display_name: &str) -> Result<Vec<User>, UserError>;
    
    /// List all users on this machine.
    fn list_users(&self) -> Result<Vec<User>, UserError>;
    
    /// Update a user's display name.
    fn update_display_name(&self, user_id: Uuid, new_name: &str) -> Result<(), UserError>;
    
    /// Update a user's status.
    fn update_status(&self, user_id: Uuid, status: UserStatus) -> Result<(), UserError>;
    
    /// Delete a user and optionally their home directory.
    fn delete_user(&self, user_id: Uuid, delete_home: bool) -> Result<(), UserError>;
    
    /// Get user preferences.
    fn get_preferences(&self, user_id: Uuid) -> Result<UserPreferences, UserError>;
    
    /// Update user preferences.
    fn set_preferences(&self, user_id: Uuid, prefs: &UserPreferences) -> Result<(), UserError>;
}

/// Errors from user operations.
#[derive(Clone, Debug)]
pub enum UserError {
    /// User not found
    NotFound,
    /// User already exists
    AlreadyExists,
    /// Permission denied
    PermissionDenied,
    /// Storage error
    StorageError(String),
    /// Invalid display name
    InvalidDisplayName,
}
```

### User Creation Flow

```rust
impl UserService for UserServiceImpl {
    fn create_user(&self, display_name: &str) -> Result<User, UserError> {
        // 1. Validate display name
        if display_name.is_empty() || display_name.len() > 64 {
            return Err(UserError::InvalidDisplayName);
        }
        
        // 2. Generate new user ID
        let user_id = Uuid::new_v4();
        let home = format!("/home/{}", user_id);
        
        // 3. Create home directory structure
        self.vfs.mkdir_p(&home)?;
        self.vfs.chown(&home, Some(user_id))?;
        
        // 4. Create hidden ZOS directory
        self.vfs.mkdir(&format!("{}/.zos", home))?;
        self.vfs.mkdir(&format!("{}/.zos/identity", home))?;
        self.vfs.mkdir(&format!("{}/.zos/sessions", home))?;
        self.vfs.mkdir(&format!("{}/.zos/credentials", home))?;
        self.vfs.mkdir(&format!("{}/.zos/tokens", home))?;
        self.vfs.mkdir(&format!("{}/.zos/config", home))?;
        
        // 5. Create standard user directories
        self.vfs.mkdir(&format!("{}/Documents", home))?;
        self.vfs.mkdir(&format!("{}/Downloads", home))?;
        self.vfs.mkdir(&format!("{}/Desktop", home))?;
        self.vfs.mkdir(&format!("{}/Pictures", home))?;
        self.vfs.mkdir(&format!("{}/Music", home))?;
        self.vfs.mkdir(&format!("{}/Apps", home))?;
        
        // 6. Create user record
        let now = current_timestamp();
        let user = User {
            id: user_id,
            display_name: display_name.to_string(),
            status: UserStatus::Offline,
            default_namespace_id: Uuid::new_v4(),
            created_at: now,
            last_active_at: now,
        };
        
        // 7. Store user record
        let user_json = serde_json::to_vec(&user)?;
        self.vfs.write_file(&format!("{}/.zos/identity/user.json", home), &user_json)?;
        
        // 8. Add to user registry
        self.add_to_registry(user_id, &display_name)?;
        
        Ok(user)
    }
}
```

## User Registry

The system maintains a global user registry:

```rust
/// Registry of all users on this machine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRegistry {
    /// List of user entries
    pub users: Vec<UserRegistryEntry>,
}

/// Entry in the user registry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRegistryEntry {
    /// User ID
    pub id: Uuid,
    /// Display name (for quick lookup)
    pub display_name: String,
    /// When the user was created
    pub created_at: u64,
}

impl UserRegistry {
    /// Path to the registry file.
    pub const PATH: &'static str = "/users/registry.json";
    
    /// Load the registry from disk.
    pub fn load(vfs: &impl VfsService) -> Result<Self, VfsError> {
        match vfs.read_file(Self::PATH) {
            Ok(data) => Ok(serde_json::from_slice(&data)?),
            Err(VfsError::NotFound) => Ok(Self { users: vec![] }),
            Err(e) => Err(e),
        }
    }
    
    /// Save the registry to disk.
    pub fn save(&self, vfs: &impl VfsService) -> Result<(), VfsError> {
        let data = serde_json::to_vec(self)?;
        vfs.write_file(Self::PATH, &data)
    }
    
    /// Add a user to the registry.
    pub fn add(&mut self, id: Uuid, display_name: &str, created_at: u64) {
        self.users.push(UserRegistryEntry {
            id,
            display_name: display_name.to_string(),
            created_at,
        });
    }
    
    /// Remove a user from the registry.
    pub fn remove(&mut self, id: Uuid) {
        self.users.retain(|u| u.id != id);
    }
}
```

## IPC Protocol

### Create User

```rust
/// Create user request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateUserRequest {
    /// Display name for the new user
    pub display_name: String,
}

/// Create user response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateUserResponse {
    pub result: Result<User, UserError>,
}
```

### Get User

```rust
/// Get user request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetUserRequest {
    /// User ID to retrieve
    pub user_id: Uuid,
}

/// Get user response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GetUserResponse {
    pub result: Result<Option<User>, UserError>,
}
```

### List Users

```rust
/// List users request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListUsersRequest {
    /// Optional status filter
    pub status_filter: Option<UserStatus>,
}

/// List users response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListUsersResponse {
    pub users: Vec<User>,
}
```

### Delete User

```rust
/// Delete user request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeleteUserRequest {
    /// User ID to delete
    pub user_id: Uuid,
    /// Whether to delete the home directory
    pub delete_home: bool,
}

/// Delete user response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeleteUserResponse {
    pub result: Result<(), UserError>,
}
```

## Invariants

1. **Unique IDs**: User IDs are globally unique UUIDs
2. **Home directory existence**: Every user has a home directory at `/home/{id}`
3. **Registry consistency**: User registry contains all valid user IDs
4. **Status consistency**: Status reflects actual session state
5. **File ownership**: Files in user home are owned by that user

## Security Considerations

1. **User isolation**: Users cannot access each other's home directories by default
2. **System user**: A special system user owns `/system` and `/users`
3. **Guest users**: Optional guest users with limited, temporary storage
4. **User deletion**: Secure deletion of home directory clears all user data

## WASM Notes

- User IDs are generated using `crypto.randomUUID()` in browsers
- Timestamps use `performance.now()` for monotonic time
- User registry is stored in the `zos-userspace` IndexedDB database

## Related Specifications

- [02-sessions.md](02-sessions.md) - Session management for users
- [03-zero-id.md](03-zero-id.md) - Cryptographic identity backing
- [../06-filesystem/02-vfs.md](../06-filesystem/02-vfs.md) - VFS for home directories
