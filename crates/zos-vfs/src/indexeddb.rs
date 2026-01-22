//! IndexedDB VFS backend for WASM targets.
//!
//! Provides persistent VFS storage using browser IndexedDB via the
//! `VfsStorage` JavaScript object (defined in `web/public/vfs-storage.js`).
//!
//! This module follows the same pattern as `zos-supervisor-web/src/axiom.rs`.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

// =============================================================================
// JavaScript Bridge (wasm-bindgen extern functions)
// =============================================================================

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    /// VfsStorage JavaScript object for IndexedDB persistence
    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn init() -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn putInode(path: &str, inode: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn getInode(path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn deleteInode(path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn listChildren(parent_path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn getAllInodes() -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn putContent(path: &str, data: &[u8]) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn getContent(path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn deleteContent(path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn getInodeCount() -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn exists(path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn clear() -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn putInodes(inodes: JsValue) -> JsValue;
}

// =============================================================================
// Inode Serialization Helpers
// =============================================================================

#[cfg(target_arch = "wasm32")]
use crate::types::{FilePermissions, Inode, InodeType};

#[cfg(target_arch = "wasm32")]
use alloc::string::String;
#[cfg(target_arch = "wasm32")]
use alloc::vec::Vec;

/// Convert an Inode to a JavaScript object for IndexedDB storage.
#[cfg(target_arch = "wasm32")]
pub fn inode_to_js(inode: &Inode) -> JsValue {
    use js_sys::{Object, Reflect};

    let obj = Object::new();

    // String fields
    let _ = Reflect::set(&obj, &"path".into(), &JsValue::from_str(&inode.path));
    let _ = Reflect::set(
        &obj,
        &"parent_path".into(),
        &JsValue::from_str(&inode.parent_path),
    );
    let _ = Reflect::set(&obj, &"name".into(), &JsValue::from_str(&inode.name));

    // Inode type
    let inode_type = match &inode.inode_type {
        InodeType::File => "File".to_string(),
        InodeType::Directory => "Directory".to_string(),
        InodeType::SymLink { target } => format!("SymLink:{}", target),
    };
    let _ = Reflect::set(&obj, &"inode_type".into(), &JsValue::from_str(&inode_type));

    // Owner ID (as hex string or null)
    if let Some(owner_id) = inode.owner_id {
        let _ = Reflect::set(
            &obj,
            &"owner_id".into(),
            &JsValue::from_str(&format!("{:032x}", owner_id)),
        );
    } else {
        let _ = Reflect::set(&obj, &"owner_id".into(), &JsValue::null());
    }

    // Permissions
    let perms = permissions_to_js(&inode.permissions);
    let _ = Reflect::set(&obj, &"permissions".into(), &perms);

    // Timestamps (as f64 since JS numbers are doubles)
    let _ = Reflect::set(
        &obj,
        &"created_at".into(),
        &JsValue::from_f64(inode.created_at as f64),
    );
    let _ = Reflect::set(
        &obj,
        &"modified_at".into(),
        &JsValue::from_f64(inode.modified_at as f64),
    );
    let _ = Reflect::set(
        &obj,
        &"accessed_at".into(),
        &JsValue::from_f64(inode.accessed_at as f64),
    );

    // Size and encrypted flag
    let _ = Reflect::set(&obj, &"size".into(), &JsValue::from_f64(inode.size as f64));
    let _ = Reflect::set(&obj, &"encrypted".into(), &JsValue::from_bool(inode.encrypted));

    // Content hash (as hex string or null)
    if let Some(ref hash) = inode.content_hash {
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        let _ = Reflect::set(&obj, &"content_hash".into(), &JsValue::from_str(&hex));
    } else {
        let _ = Reflect::set(&obj, &"content_hash".into(), &JsValue::null());
    }

    obj.into()
}

/// Convert a JavaScript object to an Inode.
#[cfg(target_arch = "wasm32")]
pub fn js_to_inode(js: &JsValue) -> Option<Inode> {
    use js_sys::Reflect;

    let path = Reflect::get(js, &"path".into())
        .ok()?
        .as_string()?;
    let parent_path = Reflect::get(js, &"parent_path".into())
        .ok()?
        .as_string()?;
    let name = Reflect::get(js, &"name".into())
        .ok()?
        .as_string()?;

    // Parse inode type
    let inode_type_str = Reflect::get(js, &"inode_type".into())
        .ok()?
        .as_string()?;
    let inode_type = if inode_type_str == "File" {
        InodeType::File
    } else if inode_type_str == "Directory" {
        InodeType::Directory
    } else if let Some(target) = inode_type_str.strip_prefix("SymLink:") {
        InodeType::SymLink {
            target: target.to_string(),
        }
    } else {
        return None;
    };

    // Parse owner ID
    let owner_js = Reflect::get(js, &"owner_id".into()).ok()?;
    let owner_id = if owner_js.is_null() || owner_js.is_undefined() {
        None
    } else {
        let owner_hex = owner_js.as_string()?;
        u128::from_str_radix(&owner_hex, 16).ok()
    };

    // Parse permissions
    let perms_js = Reflect::get(js, &"permissions".into()).ok()?;
    let permissions = js_to_permissions(&perms_js)?;

    // Parse timestamps
    let created_at = Reflect::get(js, &"created_at".into())
        .ok()?
        .as_f64()? as u64;
    let modified_at = Reflect::get(js, &"modified_at".into())
        .ok()?
        .as_f64()? as u64;
    let accessed_at = Reflect::get(js, &"accessed_at".into())
        .ok()?
        .as_f64()? as u64;

    // Parse size and encrypted
    let size = Reflect::get(js, &"size".into()).ok()?.as_f64()? as u64;
    let encrypted = Reflect::get(js, &"encrypted".into())
        .ok()?
        .as_bool()
        .unwrap_or(false);

    // Parse content hash
    let hash_js = Reflect::get(js, &"content_hash".into()).ok()?;
    let content_hash = if hash_js.is_null() || hash_js.is_undefined() {
        None
    } else {
        let hash_hex = hash_js.as_string()?;
        let bytes: Vec<u8> = (0..hash_hex.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&hash_hex[i..i + 2], 16).ok())
            .collect();
        if bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            Some(arr)
        } else {
            None
        }
    };

    Some(Inode {
        path,
        parent_path,
        name,
        inode_type,
        owner_id,
        permissions,
        created_at,
        modified_at,
        accessed_at,
        size,
        encrypted,
        content_hash,
    })
}

/// Convert FilePermissions to a JavaScript object.
#[cfg(target_arch = "wasm32")]
fn permissions_to_js(perms: &FilePermissions) -> JsValue {
    use js_sys::{Object, Reflect};

    let obj = Object::new();
    let _ = Reflect::set(
        &obj,
        &"owner_read".into(),
        &JsValue::from_bool(perms.owner_read),
    );
    let _ = Reflect::set(
        &obj,
        &"owner_write".into(),
        &JsValue::from_bool(perms.owner_write),
    );
    let _ = Reflect::set(
        &obj,
        &"owner_execute".into(),
        &JsValue::from_bool(perms.owner_execute),
    );
    let _ = Reflect::set(
        &obj,
        &"system_read".into(),
        &JsValue::from_bool(perms.system_read),
    );
    let _ = Reflect::set(
        &obj,
        &"system_write".into(),
        &JsValue::from_bool(perms.system_write),
    );
    let _ = Reflect::set(
        &obj,
        &"world_read".into(),
        &JsValue::from_bool(perms.world_read),
    );
    let _ = Reflect::set(
        &obj,
        &"world_write".into(),
        &JsValue::from_bool(perms.world_write),
    );

    obj.into()
}

/// Convert a JavaScript object to FilePermissions.
#[cfg(target_arch = "wasm32")]
fn js_to_permissions(js: &JsValue) -> Option<FilePermissions> {
    use js_sys::Reflect;

    Some(FilePermissions {
        owner_read: Reflect::get(js, &"owner_read".into())
            .ok()?
            .as_bool()?,
        owner_write: Reflect::get(js, &"owner_write".into())
            .ok()?
            .as_bool()?,
        owner_execute: Reflect::get(js, &"owner_execute".into())
            .ok()?
            .as_bool()?,
        system_read: Reflect::get(js, &"system_read".into())
            .ok()?
            .as_bool()?,
        system_write: Reflect::get(js, &"system_write".into())
            .ok()?
            .as_bool()?,
        world_read: Reflect::get(js, &"world_read".into())
            .ok()?
            .as_bool()?,
        world_write: Reflect::get(js, &"world_write".into())
            .ok()?
            .as_bool()?,
    })
}

// =============================================================================
// Unit Tests (non-WASM)
// =============================================================================

#[cfg(test)]
mod tests {
    // Tests for serialization would go here, but require mocking JsValue
    // which is complex. The actual testing happens in integration tests
    // running in a browser environment.
}
