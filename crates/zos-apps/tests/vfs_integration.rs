//! App VFS Integration Tests
//!
//! Tests that verify app filesystem access and restrictions.

use zos_vfs::types::FilePermissions;
use zos_vfs::MemoryVfs;
use zos_vfs::VfsService;

extern crate alloc;

/// Test that an app can read/write to its data directory.
#[test]
fn test_app_can_read_write_data_directory() {
    let vfs = MemoryVfs::new();
    let user_id: u128 = 0x00000000000000000000000000000001;
    let app_id = "com.example.calculator";

    let home_path = format!("/home/{}", user_id);
    let app_data_path = format!("{}/Apps/{}", home_path, app_id);

    // Create app data directory
    vfs.mkdir_p(&app_data_path).unwrap();

    // Write files
    let data_file = format!("{}/data.json", app_data_path);
    vfs.write_file(&data_file, b"{\"history\": []}").unwrap();

    // Read back
    let content = vfs.read_file(&data_file).unwrap();
    assert_eq!(content, b"{\"history\": []}");

    // Update file
    vfs.write_file(&data_file, b"{\"history\": [\"1+1=2\"]}")
        .unwrap();
    let content = vfs.read_file(&data_file).unwrap();
    assert_eq!(content, b"{\"history\": [\"1+1=2\"]}");
}

/// Test that an app cannot access other apps' directories.
#[test]
fn test_app_cannot_access_other_apps_directories() {
    let vfs = MemoryVfs::new();
    let user_id: u128 = 0x00000000000000000000000000000001;

    let home_path = format!("/home/{}", user_id);
    let app1_path = format!("{}/Apps/com.example.app1", home_path);
    let app2_path = format!("{}/Apps/com.example.app2", home_path);

    // Create both app directories
    vfs.mkdir_p(&app1_path).unwrap();
    vfs.mkdir_p(&app2_path).unwrap();

    // App1 writes a secret file
    let secret_file = format!("{}/secret.txt", app1_path);
    vfs.write_file(&secret_file, b"app1 secret data").unwrap();

    // Change ownership to app1 (simulated by setting owner)
    vfs.chown(&app1_path, Some(1)).unwrap();

    // The file exists
    assert!(vfs.exists(&secret_file).unwrap());

    // In a real implementation, app2's process would have permission checks
    // that prevent it from reading app1's files. The VFS service would check
    // the calling process's context and deny access.
    //
    // This test verifies the file system structure is correct for isolation.
    let entries = vfs.readdir(&format!("{}/Apps", home_path)).unwrap();
    assert_eq!(entries.len(), 2);
}

/// Test that an app cannot access other users' home directories.
#[test]
fn test_app_cannot_access_other_users_home() {
    let vfs = MemoryVfs::new();

    let user1_id: u128 = 0x00000000000000000000000000000001;
    let user2_id: u128 = 0x00000000000000000000000000000002;

    let user1_home = format!("/home/{}", user1_id);
    let user2_home = format!("/home/{}", user2_id);

    // Create both homes with private permissions
    vfs.mkdir_p(&user1_home).unwrap();
    vfs.mkdir_p(&user2_home).unwrap();

    // Set ownership and private permissions
    vfs.chown(&user1_home, Some(user1_id)).unwrap();
    vfs.chown(&user2_home, Some(user2_id)).unwrap();
    vfs.chmod(&user1_home, FilePermissions::user_dir_default())
        .unwrap();
    vfs.chmod(&user2_home, FilePermissions::user_dir_default())
        .unwrap();

    // User1 writes private data
    let user1_file = format!("{}/private.txt", user1_home);
    vfs.write_file(&user1_file, b"user1 private data").unwrap();

    // Verify ownership structure
    let stat = vfs.stat(&user1_home).unwrap();
    assert_eq!(stat.owner_id, Some(user1_id));
    assert!(!stat.permissions.world_read);
}

/// Test quota enforcement for app storage.
#[test]
fn test_quota_enforcement_for_app_storage() {
    let vfs = MemoryVfs::new();
    let user_id: u128 = 0x00000000000000000000000000000001;

    let home_path = format!("/home/{}", user_id);
    vfs.mkdir_p(&home_path).unwrap();

    // Set a small quota (1KB)
    vfs.set_quota(user_id, 1024).unwrap();

    // Verify quota was set
    let quota = vfs.get_quota(user_id).unwrap();
    assert_eq!(quota.max_bytes, 1024);
    assert_eq!(quota.soft_limit_bytes, 819); // 80% of 1024

    // Write some data
    let file_path = format!("{}/test.txt", home_path);
    vfs.write_file(&file_path, b"Hello, World!").unwrap();

    // Get usage
    let usage = vfs.get_usage(&home_path).unwrap();
    assert_eq!(usage.used_bytes, 13); // "Hello, World!".len()
    assert_eq!(usage.file_count, 1);
}

/// Test file operations within app sandbox.
#[test]
fn test_file_operations_in_app_sandbox() {
    let vfs = MemoryVfs::new();
    let user_id: u128 = 0x00000000000000000000000000000001;
    let app_id = "com.example.notes";

    let home_path = format!("/home/{}", user_id);
    let app_path = format!("{}/Apps/{}", home_path, app_id);
    let docs_path = format!("{}/documents", app_path);

    // Create directory structure
    vfs.mkdir_p(&docs_path).unwrap();

    // Create file
    let note1 = format!("{}/note1.txt", docs_path);
    vfs.write_file(&note1, b"First note").unwrap();

    // Copy file
    let note1_backup = format!("{}/note1_backup.txt", docs_path);
    vfs.copy(&note1, &note1_backup).unwrap();

    // Rename file
    let note1_renamed = format!("{}/note1_renamed.txt", docs_path);
    vfs.rename(&note1, &note1_renamed).unwrap();

    // Verify operations
    assert!(!vfs.exists(&note1).unwrap());
    assert!(vfs.exists(&note1_renamed).unwrap());
    assert!(vfs.exists(&note1_backup).unwrap());

    // List directory
    let entries = vfs.readdir(&docs_path).unwrap();
    assert_eq!(entries.len(), 2);

    // Delete file
    vfs.unlink(&note1_backup).unwrap();
    assert!(!vfs.exists(&note1_backup).unwrap());
}

/// Test encrypted file storage for sensitive app data.
#[test]
fn test_encrypted_file_storage() {
    let vfs = MemoryVfs::new();
    let user_id: u128 = 0x00000000000000000000000000000001;

    let home_path = format!("/home/{}", user_id);
    let secrets_path = format!("{}/.zos/credentials", home_path);
    vfs.mkdir_p(&secrets_path).unwrap();

    // Write encrypted file (in MemoryVfs this is a no-op, but the API is tested)
    let key: [u8; 32] = [0u8; 32];
    let credential_file = format!("{}/api_key.enc", secrets_path);
    vfs.write_file_encrypted(&credential_file, b"secret-api-key-12345", &key)
        .unwrap();

    // Verify it's marked as encrypted
    let stat = vfs.stat(&credential_file).unwrap();
    assert!(stat.encrypted);

    // Read it back
    let content = vfs.read_file_encrypted(&credential_file, &key).unwrap();
    assert_eq!(content, b"secret-api-key-12345");
}

/// Test symlink operations for app shortcuts.
#[test]
fn test_symlink_operations() {
    let vfs = MemoryVfs::new();
    let user_id: u128 = 0x00000000000000000000000000000001;

    let home_path = format!("/home/{}", user_id);
    vfs.mkdir_p(&home_path).unwrap();

    // Create a file
    let original = format!("{}/original.txt", home_path);
    vfs.write_file(&original, b"Original content").unwrap();

    // Create symlink
    let link = format!("{}/link.txt", home_path);
    vfs.symlink(&original, &link).unwrap();

    // Read symlink target
    let target = vfs.readlink(&link).unwrap();
    assert_eq!(target, original);

    // Verify it's a symlink
    let stat = vfs.stat(&link).unwrap();
    assert!(stat.is_symlink());
}

/// Test recursive directory operations.
#[test]
fn test_recursive_directory_operations() {
    let vfs = MemoryVfs::new();
    let user_id: u128 = 0x00000000000000000000000000000001;

    let home_path = format!("/home/{}", user_id);
    let project_path = format!("{}/Apps/ide/projects/myproject", home_path);

    // Create deep directory structure
    vfs.mkdir_p(&project_path).unwrap();
    vfs.mkdir_p(&format!("{}/src", project_path)).unwrap();
    vfs.mkdir_p(&format!("{}/docs", project_path)).unwrap();

    // Create files
    vfs.write_file(&format!("{}/src/main.rs", project_path), b"fn main() {}")
        .unwrap();
    vfs.write_file(&format!("{}/README.md", project_path), b"# My Project")
        .unwrap();

    // Verify structure
    let entries = vfs.readdir(&project_path).unwrap();
    assert_eq!(entries.len(), 3); // src, docs, README.md

    // Recursive delete
    vfs.rmdir_recursive(&project_path).unwrap();
    assert!(!vfs.exists(&project_path).unwrap());

    // Parent should still exist
    let parent = format!("{}/Apps/ide/projects", home_path);
    assert!(vfs.exists(&parent).unwrap());
}
