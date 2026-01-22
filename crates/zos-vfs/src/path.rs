//! Path utilities for the VFS layer.
//!
//! Provides path validation, normalization, and resolution.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::VfsError;

/// Validate that a path is well-formed.
pub fn validate_path(path: &str) -> Result<(), VfsError> {
    if path.is_empty() {
        return Err(VfsError::InvalidPath(String::from("Empty path")));
    }

    if !path.starts_with('/') {
        return Err(VfsError::InvalidPath(String::from(
            "Path must be absolute (start with /)",
        )));
    }

    // Check for invalid characters
    if path.contains('\0') {
        return Err(VfsError::InvalidPath(String::from(
            "Path contains null character",
        )));
    }

    // Check for double slashes (except at start which we handle in normalize)
    if path.contains("//") && path != "/" {
        // This is a soft validation - normalize will fix it
    }

    Ok(())
}

/// Normalize a path by resolving `.` and `..` components and removing redundant slashes.
pub fn normalize_path(path: &str) -> Result<String, VfsError> {
    validate_path(path)?;

    let mut components: Vec<&str> = Vec::new();

    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                if components.is_empty() {
                    return Err(VfsError::InvalidPath(String::from(
                        "Path escapes root directory",
                    )));
                }
                components.pop();
            }
            c => components.push(c),
        }
    }

    if components.is_empty() {
        Ok(String::from("/"))
    } else {
        let mut result = String::new();
        for component in components {
            result.push('/');
            result.push_str(component);
        }
        Ok(result)
    }
}

/// Get the parent path of a given path.
pub fn parent_path(path: &str) -> String {
    if path == "/" {
        return String::from("/");
    }

    match path.rfind('/') {
        Some(0) => String::from("/"),
        Some(pos) => String::from(&path[..pos]),
        None => String::from("/"),
    }
}

/// Get the filename (last component) of a path.
pub fn filename(path: &str) -> &str {
    if path == "/" {
        return "";
    }

    match path.rfind('/') {
        Some(pos) => &path[pos + 1..],
        None => path,
    }
}

/// Join two path components.
pub fn join_path(base: &str, name: &str) -> String {
    if base == "/" {
        alloc::format!("/{}", name)
    } else {
        alloc::format!("{}/{}", base, name)
    }
}

/// Check if a path is under a given base path.
pub fn is_under(path: &str, base: &str) -> bool {
    if base == "/" {
        return true;
    }

    path.starts_with(base) && (path.len() == base.len() || path.as_bytes()[base.len()] == b'/')
}

/// Extract the user ID from a home directory path.
/// Returns None if the path is not under /home/{user_id}/
pub fn extract_user_id(path: &str) -> Option<u128> {
    if !path.starts_with("/home/") {
        return None;
    }

    let rest = &path[6..]; // Skip "/home/"
    let user_id_str = rest.split('/').next()?;

    // Try to parse as hex UUID
    u128::from_str_radix(user_id_str, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_path() {
        assert!(validate_path("/").is_ok());
        assert!(validate_path("/home/user").is_ok());
        assert!(validate_path("").is_err());
        assert!(validate_path("relative/path").is_err());
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("/").unwrap(), "/");
        assert_eq!(normalize_path("/home/user").unwrap(), "/home/user");
        assert_eq!(normalize_path("/home/./user").unwrap(), "/home/user");
        assert_eq!(normalize_path("/home/user/../other").unwrap(), "/home/other");
        assert_eq!(normalize_path("/a/b/c/../../d").unwrap(), "/a/d");
        assert!(normalize_path("/..").is_err()); // Escapes root
    }

    #[test]
    fn test_parent_path() {
        assert_eq!(parent_path("/"), "/");
        assert_eq!(parent_path("/home"), "/");
        assert_eq!(parent_path("/home/user"), "/home");
        assert_eq!(parent_path("/home/user/docs"), "/home/user");
    }

    #[test]
    fn test_filename() {
        assert_eq!(filename("/"), "");
        assert_eq!(filename("/home"), "home");
        assert_eq!(filename("/home/user"), "user");
        assert_eq!(filename("/home/user/file.txt"), "file.txt");
    }

    #[test]
    fn test_join_path() {
        assert_eq!(join_path("/", "home"), "/home");
        assert_eq!(join_path("/home", "user"), "/home/user");
        assert_eq!(join_path("/home/user", "file.txt"), "/home/user/file.txt");
    }

    #[test]
    fn test_is_under() {
        assert!(is_under("/home/user", "/home"));
        assert!(is_under("/home/user/docs", "/home/user"));
        assert!(is_under("/anything", "/"));
        assert!(!is_under("/home", "/home/user"));
        assert!(!is_under("/homeuser", "/home")); // Not a proper prefix
    }

    #[test]
    fn test_extract_user_id() {
        assert_eq!(
            extract_user_id("/home/00000000000000000000000000000001/docs"),
            Some(1)
        );
        assert_eq!(extract_user_id("/system/config"), None);
        assert_eq!(extract_user_id("/tmp"), None);
    }
}
