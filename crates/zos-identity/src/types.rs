//! User types for the Identity layer.
//!
//! Defines the core user primitive and related types.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// A unique user identifier (UUID as 128-bit value).
pub type UserId = u128;

/// A ZOS user backed by a Zero-ID Identity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    /// Local user ID (matches zero-id identity_id)
    pub id: UserId,

    /// Display name for UI
    pub display_name: String,

    /// User status in the system
    pub status: UserStatus,

    /// Default namespace for this user's resources
    pub default_namespace_id: u128,

    /// When the user was created locally (nanos since epoch)
    pub created_at: u64,

    /// Last activity timestamp (nanos since epoch)
    pub last_active_at: u64,
}

impl User {
    /// Returns the user's home directory path.
    pub fn home_dir(&self) -> String {
        alloc::format!("/home/{}", self.id)
    }

    /// Returns the user's hidden ZOS directory path.
    pub fn zos_dir(&self) -> String {
        alloc::format!("/home/{}/.zos", self.id)
    }

    /// Returns the user's identity directory path.
    pub fn identity_dir(&self) -> String {
        alloc::format!("/home/{}/.zos/identity", self.id)
    }

    /// Returns the user's sessions directory path.
    pub fn sessions_dir(&self) -> String {
        alloc::format!("/home/{}/.zos/sessions", self.id)
    }

    /// Returns the user's credentials directory path.
    pub fn credentials_dir(&self) -> String {
        alloc::format!("/home/{}/.zos/credentials", self.id)
    }

    /// Returns the user's tokens directory path.
    pub fn tokens_dir(&self) -> String {
        alloc::format!("/home/{}/.zos/tokens", self.id)
    }

    /// Returns the user's config directory path.
    pub fn config_dir(&self) -> String {
        alloc::format!("/home/{}/.zos/config", self.id)
    }

    /// Returns the user's app data directory for a specific app.
    pub fn app_data_dir(&self, app_id: &str) -> String {
        alloc::format!("/home/{}/Apps/{}", self.id, app_id)
    }
}

/// Status of a user account.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserStatus {
    /// User has at least one active local session
    Active,

    /// User exists but has no active sessions
    #[default]
    Offline,

    /// Account is suspended (cannot login)
    Suspended,
}

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
    /// Path where preferences are stored for a user.
    pub fn storage_path(user_id: UserId) -> String {
        alloc::format!("/home/{}/.zos/config/preferences.json", user_id)
    }
}

/// Registry of all users on this machine.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserRegistry {
    /// List of user entries
    pub users: Vec<UserRegistryEntry>,
}

impl UserRegistry {
    /// Path to the registry file.
    pub const PATH: &'static str = "/users/registry.json";

    /// Create a new empty registry.
    pub fn new() -> Self {
        Self { users: Vec::new() }
    }

    /// Add a user to the registry.
    pub fn add(&mut self, id: UserId, display_name: &str, created_at: u64) {
        self.users.push(UserRegistryEntry {
            id,
            display_name: String::from(display_name),
            created_at,
        });
    }

    /// Remove a user from the registry.
    pub fn remove(&mut self, id: UserId) {
        self.users.retain(|u| u.id != id);
    }

    /// Find a user by ID.
    pub fn find(&self, id: UserId) -> Option<&UserRegistryEntry> {
        self.users.iter().find(|u| u.id == id)
    }

    /// Find users by display name.
    pub fn find_by_name(&self, name: &str) -> Vec<&UserRegistryEntry> {
        self.users
            .iter()
            .filter(|u| u.display_name == name)
            .collect()
    }
}

/// Entry in the user registry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserRegistryEntry {
    /// User ID
    pub id: UserId,

    /// Display name (for quick lookup)
    pub display_name: String,

    /// When the user was created
    pub created_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_paths() {
        let user = User {
            id: 0x12345678_9abcdef0_12345678_9abcdef0,
            display_name: String::from("Test User"),
            status: UserStatus::Active,
            default_namespace_id: 0,
            created_at: 0,
            last_active_at: 0,
        };

        assert!(user.home_dir().starts_with("/home/"));
        assert!(user.zos_dir().ends_with("/.zos"));
        assert!(user.identity_dir().ends_with("/.zos/identity"));
        assert!(user.sessions_dir().ends_with("/.zos/sessions"));
    }

    #[test]
    fn test_user_registry() {
        let mut registry = UserRegistry::new();

        registry.add(1, "Alice", 1000);
        registry.add(2, "Bob", 2000);

        assert_eq!(registry.users.len(), 2);
        assert!(registry.find(1).is_some());
        assert!(registry.find(3).is_none());

        registry.remove(1);
        assert_eq!(registry.users.len(), 1);
        assert!(registry.find(1).is_none());
    }
}
