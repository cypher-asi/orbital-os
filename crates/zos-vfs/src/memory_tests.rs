use super::*;
use crate::storage::DEFAULT_QUOTA_BYTES;

#[test]
fn test_mkdir() {
    let vfs = MemoryVfs::new();

    vfs.mkdir("/home").unwrap();
    assert!(vfs.exists("/home").unwrap());

    // Should fail - already exists
    assert!(vfs.mkdir("/home").is_err());
}

#[test]
fn test_mkdir_p() {
    let vfs = MemoryVfs::new();

    vfs.mkdir_p("/home/user/Documents").unwrap();
    assert!(vfs.exists("/home").unwrap());
    assert!(vfs.exists("/home/user").unwrap());
    assert!(vfs.exists("/home/user/Documents").unwrap());
}

#[test]
fn test_write_read_file() {
    let vfs = MemoryVfs::new();

    vfs.mkdir("/home").unwrap();
    vfs.write_file("/home/test.txt", b"Hello, World!").unwrap();

    let content = vfs.read_file("/home/test.txt").unwrap();
    assert_eq!(content, b"Hello, World!");
}

#[test]
fn test_readdir() {
    let vfs = MemoryVfs::new();

    vfs.mkdir_p("/home/user").unwrap();
    vfs.write_file("/home/user/file1.txt", b"1").unwrap();
    vfs.write_file("/home/user/file2.txt", b"2").unwrap();
    vfs.mkdir("/home/user/Documents").unwrap();

    let entries = vfs.readdir("/home/user").unwrap();
    assert_eq!(entries.len(), 3);

    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"file1.txt"));
    assert!(names.contains(&"file2.txt"));
    assert!(names.contains(&"Documents"));
}

#[test]
fn test_rmdir() {
    let vfs = MemoryVfs::new();

    vfs.mkdir_p("/home/user").unwrap();

    // Can't remove non-empty directory
    assert!(vfs.rmdir("/home").is_err());

    // Can remove empty directory
    vfs.rmdir("/home/user").unwrap();
    assert!(!vfs.exists("/home/user").unwrap());
}

#[test]
fn test_rmdir_recursive() {
    let vfs = MemoryVfs::new();

    vfs.mkdir_p("/home/user/Documents").unwrap();
    vfs.write_file("/home/user/Documents/file.txt", b"data")
        .unwrap();

    vfs.rmdir_recursive("/home/user").unwrap();

    assert!(!vfs.exists("/home/user").unwrap());
    assert!(vfs.exists("/home").unwrap());
}

#[test]
fn test_rename() {
    let vfs = MemoryVfs::new();

    vfs.mkdir("/home").unwrap();
    vfs.write_file("/home/old.txt", b"content").unwrap();

    vfs.rename("/home/old.txt", "/home/new.txt").unwrap();

    assert!(!vfs.exists("/home/old.txt").unwrap());
    assert!(vfs.exists("/home/new.txt").unwrap());
    assert_eq!(vfs.read_file("/home/new.txt").unwrap(), b"content");
}

#[test]
fn test_copy() {
    let vfs = MemoryVfs::new();

    vfs.mkdir("/home").unwrap();
    vfs.write_file("/home/original.txt", b"content").unwrap();

    vfs.copy("/home/original.txt", "/home/copy.txt").unwrap();

    assert!(vfs.exists("/home/original.txt").unwrap());
    assert!(vfs.exists("/home/copy.txt").unwrap());
    assert_eq!(vfs.read_file("/home/copy.txt").unwrap(), b"content");
}

#[test]
fn test_stat() {
    let vfs = MemoryVfs::new();

    vfs.mkdir("/home").unwrap();
    vfs.write_file("/home/test.txt", b"Hello").unwrap();

    let inode = vfs.stat("/home/test.txt").unwrap();
    assert!(inode.is_file());
    assert_eq!(inode.size, 5);
    assert_eq!(inode.name, "test.txt");

    let dir_inode = vfs.stat("/home").unwrap();
    assert!(dir_inode.is_directory());
}

#[test]
fn test_chmod_chown() {
    let vfs = MemoryVfs::new();

    vfs.mkdir("/home").unwrap();

    // Change permissions
    vfs.chmod("/home", FilePermissions::world_rw()).unwrap();
    let inode = vfs.stat("/home").unwrap();
    assert!(inode.permissions.world_write);

    // Change owner
    vfs.chown("/home", Some(12345)).unwrap();
    let inode = vfs.stat("/home").unwrap();
    assert_eq!(inode.owner_id, Some(12345));
}

#[test]
fn test_symlink() {
    let vfs = MemoryVfs::new();

    vfs.mkdir("/home").unwrap();
    vfs.write_file("/home/target.txt", b"content").unwrap();
    vfs.symlink("/home/target.txt", "/home/link.txt").unwrap();

    let target = vfs.readlink("/home/link.txt").unwrap();
    assert_eq!(target, "/home/target.txt");

    let inode = vfs.stat("/home/link.txt").unwrap();
    assert!(inode.is_symlink());
}

#[test]
fn test_get_usage() {
    let vfs = MemoryVfs::new();

    vfs.mkdir_p("/home/user").unwrap();
    vfs.write_file("/home/user/file1.txt", b"12345").unwrap();
    vfs.write_file("/home/user/file2.txt", b"67890").unwrap();

    let usage = vfs.get_usage("/home/user").unwrap();
    assert_eq!(usage.file_count, 2);
    assert_eq!(usage.used_bytes, 10);
    assert_eq!(usage.directory_count, 1); // /home/user itself
}

#[test]
fn test_quota() {
    let vfs = MemoryVfs::new();

    let quota = vfs.get_quota(123).unwrap();
    assert_eq!(quota.max_bytes, DEFAULT_QUOTA_BYTES);

    vfs.set_quota(123, 1000).unwrap();
    let quota = vfs.get_quota(123).unwrap();
    assert_eq!(quota.max_bytes, 1000);
}
