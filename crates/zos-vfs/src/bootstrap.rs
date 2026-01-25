//! Filesystem bootstrap for the VFS layer.
//!
//! Handles initialization of the root filesystem structure on first boot.

use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::error::VfsError;
use crate::service::VfsService;
use crate::types::FilePermissions;

/// Machine configuration stored at /system/config/machine.json
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachineConfig {
    /// Unique machine identifier
    pub machine_id: u128,

    /// When this machine was first initialized
    pub created_at: u64,

    /// Number of times the system has booted
    pub boot_count: u64,

    /// Optional machine name
    pub name: Option<String>,
}

impl MachineConfig {
    /// Path to the machine config file.
    pub const PATH: &'static str = "/system/config/machine.json";

    /// Create a new machine config.
    pub fn new(machine_id: u128, now: u64) -> Self {
        Self {
            machine_id,
            created_at: now,
            boot_count: 1,
            name: None,
        }
    }

    /// Increment boot count.
    pub fn increment_boot(&mut self) {
        self.boot_count += 1;
    }
}

/// Bootstrap the root filesystem structure.
///
/// Creates the following directory hierarchy:
/// ```text
/// /
/// ├── system/
/// │   ├── config/
/// │   │   └── machine.json
/// │   └── services/
/// ├── users/
/// │   └── registry.json
/// ├── tmp/
/// └── home/
/// ```
///
/// This function is idempotent - if the filesystem is already initialized,
/// it only cleans /tmp and increments the boot counter.
pub fn bootstrap_filesystem<V: VfsService>(
    vfs: &V,
    machine_id: u128,
    now: u64,
) -> Result<(), VfsError> {
    // Check if already initialized
    if vfs.exists("/system")? {
        // Already initialized - just clean tmp and update boot count
        clean_tmp(vfs)?;
        increment_boot_count(vfs)?;
        return Ok(());
    }

    // Create root directories with appropriate permissions
    create_system_directories(vfs)?;

    // Initialize machine config
    let machine_config = MachineConfig::new(machine_id, now);
    let config_json = serialize_json(&machine_config)?;
    vfs.write_file(MachineConfig::PATH, &config_json)?;

    // Initialize empty user registry
    let registry = UserRegistryData { users: Vec::new() };
    let registry_json = serialize_json(&registry)?;
    vfs.write_file("/users/registry.json", &registry_json)?;

    Ok(())
}

/// Create the system directory structure.
fn create_system_directories<V: VfsService>(vfs: &V) -> Result<(), VfsError> {
    // /system - system configuration (system-only access)
    vfs.mkdir("/system")?;
    vfs.chmod("/system", FilePermissions::system_only())?;

    vfs.mkdir("/system/config")?;
    vfs.chmod("/system/config", FilePermissions::system_only())?;

    vfs.mkdir("/system/services")?;
    vfs.chmod("/system/services", FilePermissions::system_only())?;

    // /users - user registry (system-only write, world read)
    vfs.mkdir("/users")?;
    vfs.chmod("/users", FilePermissions::system_only())?;

    // /tmp - temporary files (world read/write)
    vfs.mkdir("/tmp")?;
    vfs.chmod("/tmp", FilePermissions::world_rw())?;

    // /home - user home directories (system-only at root)
    vfs.mkdir("/home")?;
    vfs.chmod("/home", FilePermissions::system_only())?;

    Ok(())
}

/// Clean the /tmp directory on boot.
pub fn clean_tmp<V: VfsService>(vfs: &V) -> Result<(), VfsError> {
    // Check if /tmp exists
    if !vfs.exists("/tmp")? {
        return Ok(());
    }

    let entries = vfs.readdir("/tmp")?;
    for entry in entries {
        let path = if entry.path.starts_with('/') {
            entry.path
        } else {
            alloc::format!("/tmp/{}", entry.name)
        };

        if entry.is_directory {
            vfs.rmdir_recursive(&path)?;
        } else {
            vfs.unlink(&path)?;
        }
    }

    Ok(())
}

/// Increment the boot counter in machine config.
fn increment_boot_count<V: VfsService>(vfs: &V) -> Result<(), VfsError> {
    let config_data = vfs.read_file(MachineConfig::PATH)?;
    let mut config: MachineConfig = deserialize_json(&config_data)?;
    config.increment_boot();
    let config_json = serialize_json(&config)?;
    vfs.write_file(MachineConfig::PATH, &config_json)?;
    Ok(())
}

/// Create a user's home directory structure.
///
/// Creates:
/// ```text
/// /home/{user_id}/
/// ├── .zos/
/// │   ├── identity/
/// │   ├── sessions/
/// │   ├── credentials/
/// │   ├── tokens/
/// │   └── config/
/// ├── Documents/
/// ├── Downloads/
/// ├── Desktop/
/// ├── Pictures/
/// ├── Music/
/// └── Apps/
/// ```
pub fn create_user_home<V: VfsService>(vfs: &V, user_id: u128, _now: u64) -> Result<(), VfsError> {
    let home = alloc::format!("/home/{}", user_id);

    // Create home directory
    vfs.mkdir(&home)?;
    vfs.chown(&home, Some(user_id))?;

    // Hidden ZOS system directories
    vfs.mkdir(&alloc::format!("{}/.zos", home))?;
    vfs.mkdir(&alloc::format!("{}/.zos/identity", home))?;
    vfs.mkdir(&alloc::format!("{}/.zos/sessions", home))?;
    vfs.mkdir(&alloc::format!("{}/.zos/credentials", home))?;
    vfs.mkdir(&alloc::format!("{}/.zos/tokens", home))?;
    vfs.mkdir(&alloc::format!("{}/.zos/config", home))?;

    // Standard user directories
    vfs.mkdir(&alloc::format!("{}/Documents", home))?;
    vfs.mkdir(&alloc::format!("{}/Downloads", home))?;
    vfs.mkdir(&alloc::format!("{}/Desktop", home))?;
    vfs.mkdir(&alloc::format!("{}/Pictures", home))?;
    vfs.mkdir(&alloc::format!("{}/Music", home))?;
    vfs.mkdir(&alloc::format!("{}/Apps", home))?;

    // Set ownership on all directories
    set_home_ownership(vfs, &home, user_id)?;

    Ok(())
}

/// Set ownership on all directories in a user's home.
fn set_home_ownership<V: VfsService>(vfs: &V, home: &str, user_id: u128) -> Result<(), VfsError> {
    // Recursively set ownership
    let entries = vfs.readdir(home)?;
    for entry in entries {
        let path = alloc::format!("{}/{}", home, entry.name);
        vfs.chown(&path, Some(user_id))?;
        if entry.is_directory {
            set_home_ownership(vfs, &path, user_id)?;
        }
    }
    Ok(())
}

/// Delete a user's home directory.
pub fn delete_user_home<V: VfsService>(vfs: &V, user_id: u128) -> Result<(), VfsError> {
    let home = alloc::format!("/home/{}", user_id);
    if vfs.exists(&home)? {
        vfs.rmdir_recursive(&home)?;
    }
    Ok(())
}

// ============================================================================
// Internal types for bootstrap
// ============================================================================

/// Minimal user registry data for bootstrap.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct UserRegistryData {
    users: Vec<UserRegistryEntryData>,
}

/// Minimal user registry entry for bootstrap.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct UserRegistryEntryData {
    id: u128,
    display_name: String,
    created_at: u64,
}

// ============================================================================
// JSON helpers (minimal, no_std compatible)
// ============================================================================

/// Serialize a value to JSON bytes.
fn serialize_json<T: Serialize>(value: &T) -> Result<Vec<u8>, VfsError> {
    // In a real implementation, this would use serde_json
    // For no_std, we need a minimal JSON serializer
    // For now, we'll use a simple format that serde can handle
    #[cfg(feature = "std")]
    {
        serde_json::to_vec(value).map_err(|e| VfsError::StorageError(alloc::format!("{}", e)))
    }
    #[cfg(not(feature = "std"))]
    {
        // Minimal no_std JSON serialization using serde
        // This requires serde_json with alloc feature
        serde_json::to_vec(value)
            .map_err(|_| VfsError::StorageError(String::from("JSON serialization failed")))
    }
}

/// Deserialize JSON bytes to a value.
fn deserialize_json<T: for<'de> Deserialize<'de>>(data: &[u8]) -> Result<T, VfsError> {
    #[cfg(feature = "std")]
    {
        serde_json::from_slice(data).map_err(|e| VfsError::StorageError(alloc::format!("{}", e)))
    }
    #[cfg(not(feature = "std"))]
    {
        serde_json::from_slice(data)
            .map_err(|_| VfsError::StorageError(String::from("JSON deserialization failed")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_machine_config() {
        let config = MachineConfig::new(12345, 1000);
        assert_eq!(config.machine_id, 12345);
        assert_eq!(config.boot_count, 1);

        let mut config2 = config.clone();
        config2.increment_boot();
        assert_eq!(config2.boot_count, 2);
    }

    #[test]
    fn test_user_home_path() {
        let user_id = 0x12345678_9abcdef0_12345678_9abcdef0u128;
        let home = alloc::format!("/home/{}", user_id);
        assert!(home.starts_with("/home/"));
        assert_eq!(home.len(), "/home/".len() + 32);
    }
}
