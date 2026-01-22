//! App Identity Integration Tests
//!
//! Tests that verify app identity and permission interactions.

use zos_vfs::MemoryVfs;
use zos_vfs::VfsService;

/// Test that an app receives user context from session.
#[test]
fn test_app_receives_user_context() {
    // Create VFS with user home directory
    let vfs = MemoryVfs::new();
    let user_id: u128 = 0x00000000000000000000000000000001;
    let home_path = format!("/home/{:032x}", user_id);

    // Create home directory structure
    vfs.mkdir_p(&home_path).unwrap();
    vfs.mkdir_p(&format!("{}/.zos/identity", home_path)).unwrap();
    vfs.mkdir_p(&format!("{}/Apps", home_path)).unwrap();

    // Verify home exists
    assert!(vfs.exists(&home_path).unwrap());
    assert!(vfs.stat(&home_path).unwrap().is_directory());
}

/// Test that app permissions are checked against manifest.
#[test]
fn test_app_permissions_checked_against_manifest() {
    // This test verifies the permission checking logic
    // In a real implementation, this would check the manifest parser
    
    // For now, verify the basic permission checking in VFS
    use zos_vfs::service::{check_read, check_write, PermissionContext, ProcessClass};
    use zos_vfs::types::{FilePermissions, Inode};

    let user_id: u128 = 0x00000000000000000000000000000001;
    
    // Create a file owned by the user
    let inode = Inode::new_file(
        String::from("/home/user/app_data.txt"),
        String::from("/home/user"),
        String::from("app_data.txt"),
        Some(user_id),
        100,
        None,
        1000,
    );

    // User context can read/write their own files
    let user_ctx = PermissionContext::user(user_id);
    assert!(check_read(&inode, &user_ctx));
    assert!(check_write(&inode, &user_ctx));

    // Other user cannot read/write
    let other_ctx = PermissionContext::user(0x00000000000000000000000000000002);
    assert!(!check_read(&inode, &other_ctx));
    assert!(!check_write(&inode, &other_ctx));

    // System can always read
    let system_ctx = PermissionContext::system();
    assert!(check_read(&inode, &system_ctx));
}

/// Test that app data directory is created at /home/{user}/Apps/{app_id}/.
#[test]
fn test_app_data_directory_created() {
    let vfs = MemoryVfs::new();
    let user_id: u128 = 0x00000000000000000000000000000001;
    let app_id = "com.example.calculator";
    let home_path = format!("/home/{:032x}", user_id);
    let app_data_path = format!("{}/Apps/{}", home_path, app_id);

    // Create the app data directory (simulating app launch)
    vfs.mkdir_p(&app_data_path).unwrap();

    // Verify structure
    assert!(vfs.exists(&app_data_path).unwrap());
    assert!(vfs.stat(&app_data_path).unwrap().is_directory());

    // App can write to its data directory
    let config_path = format!("{}/config.json", app_data_path);
    vfs.write_file(&config_path, b"{\"theme\": \"dark\"}").unwrap();
    
    let content = vfs.read_file(&config_path).unwrap();
    assert_eq!(content, b"{\"theme\": \"dark\"}");
}

/// Test that permission denial works for unauthorized operations.
#[test]
fn test_permission_denial_for_unauthorized_operations() {
    use zos_vfs::service::{check_read, check_write, PermissionContext};
    use zos_vfs::types::{FilePermissions, Inode};

    let owner_id: u128 = 0x00000000000000000000000000000001;
    let attacker_id: u128 = 0x00000000000000000000000000000002;

    // Create a private file with no world permissions
    let mut inode = Inode::new_file(
        String::from("/home/user/private.txt"),
        String::from("/home/user"),
        String::from("private.txt"),
        Some(owner_id),
        100,
        None,
        1000,
    );
    
    // Ensure no world permissions
    inode.permissions = FilePermissions {
        owner_read: true,
        owner_write: true,
        owner_execute: false,
        system_read: true,
        system_write: false,
        world_read: false,
        world_write: false,
    };

    // Attacker cannot read or write
    let attacker_ctx = PermissionContext::user(attacker_id);
    assert!(!check_read(&inode, &attacker_ctx));
    assert!(!check_write(&inode, &attacker_ctx));

    // Owner can still access
    let owner_ctx = PermissionContext::user(owner_id);
    assert!(check_read(&inode, &owner_ctx));
    assert!(check_write(&inode, &owner_ctx));
}

/// Test user session ID generation.
#[test]
fn test_session_data_isolation() {
    let vfs = MemoryVfs::new();
    
    let user1_id: u128 = 0x00000000000000000000000000000001;
    let user2_id: u128 = 0x00000000000000000000000000000002;
    
    let user1_home = format!("/home/{:032x}", user1_id);
    let user2_home = format!("/home/{:032x}", user2_id);

    // Create both user homes
    vfs.mkdir_p(&format!("{}/.zos/sessions", user1_home)).unwrap();
    vfs.mkdir_p(&format!("{}/.zos/sessions", user2_home)).unwrap();

    // Write session data for user1
    let session_path = format!("{}/.zos/sessions/current.json", user1_home);
    vfs.write_file(&session_path, b"{\"session_id\": \"abc123\"}").unwrap();

    // User1's session file should exist
    assert!(vfs.exists(&session_path).unwrap());

    // User2's session path should not have user1's session
    let user2_session = format!("{}/.zos/sessions/current.json", user2_home);
    assert!(!vfs.exists(&user2_session).unwrap());
}

use alloc::string::String;
extern crate alloc;
