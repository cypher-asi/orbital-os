//! Bootstrap VFS Storage - Internal module for supervisor bootstrap operations
//!
//! This module provides async access to ZosStorage for bootstrap operations.
//! It is used ONLY during supervisor initialization before processes exist.
//!
//! ## Why This Module Exists
//!
//! The HAL trait methods are synchronous, but IndexedDB operations are async.
//! This module bridges that gap by providing async wasm_bindgen extern functions
//! that can be awaited during bootstrap.
//!
//! ## Access Patterns
//!
//! After bootstrap completes:
//! - Process storage operations use syscalls which route through HAL
//! - HAL sync methods (bootstrap_storage_*) use ZosStorage's in-memory cache
//! - React UI reads from ZosStorage caches (read-only)
//!
//! This module is internal to zos-supervisor-web and should not be used
//! outside of boot.rs bootstrap sequence.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// ZosStorage JavaScript object for IndexedDB persistence (bootstrap only)
    #[wasm_bindgen(js_namespace = ZosStorage)]
    pub async fn init() -> JsValue;

    #[wasm_bindgen(js_namespace = ZosStorage)]
    pub async fn clear() -> JsValue;

    #[wasm_bindgen(js_namespace = ZosStorage)]
    pub async fn getInodeCount() -> JsValue;

    #[wasm_bindgen(js_namespace = ZosStorage)]
    pub async fn putInode(path: &str, inode: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = ZosStorage)]
    pub async fn getInode(path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ZosStorage)]
    pub async fn putContent(path: &str, data: &[u8]) -> JsValue;

    #[wasm_bindgen(js_namespace = ZosStorage)]
    pub async fn getContent(path: &str) -> JsValue;
}

/// Create a root directory inode as a JavaScript object
pub fn create_root_inode() -> JsValue {
    let obj = js_sys::Object::new();
    let now = js_sys::Date::now();

    let _ = js_sys::Reflect::set(&obj, &"path".into(), &JsValue::from_str("/"));
    let _ = js_sys::Reflect::set(&obj, &"parent_path".into(), &JsValue::from_str(""));
    let _ = js_sys::Reflect::set(&obj, &"name".into(), &JsValue::from_str(""));
    let _ = js_sys::Reflect::set(&obj, &"inode_type".into(), &JsValue::from_str("Directory"));
    let _ = js_sys::Reflect::set(&obj, &"owner_id".into(), &JsValue::null());
    let _ = js_sys::Reflect::set(&obj, &"created_at".into(), &JsValue::from_f64(now));
    let _ = js_sys::Reflect::set(&obj, &"modified_at".into(), &JsValue::from_f64(now));
    let _ = js_sys::Reflect::set(&obj, &"accessed_at".into(), &JsValue::from_f64(now));
    let _ = js_sys::Reflect::set(&obj, &"size".into(), &JsValue::from_f64(0.0));
    let _ = js_sys::Reflect::set(&obj, &"encrypted".into(), &JsValue::from_bool(false));
    let _ = js_sys::Reflect::set(&obj, &"content_hash".into(), &JsValue::null());

    // Permissions for root: system rw, world r
    let perms = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&perms, &"owner_read".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"owner_write".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"owner_execute".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"system_read".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"system_write".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"world_read".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"world_write".into(), &JsValue::from_bool(false));
    let _ = js_sys::Reflect::set(&obj, &"permissions".into(), &perms);

    obj.into()
}

/// Create a directory inode as a JavaScript object
pub fn create_dir_inode(path: &str, parent_path: &str, name: &str) -> JsValue {
    let obj = js_sys::Object::new();
    let now = js_sys::Date::now();

    let _ = js_sys::Reflect::set(&obj, &"path".into(), &JsValue::from_str(path));
    let _ = js_sys::Reflect::set(&obj, &"parent_path".into(), &JsValue::from_str(parent_path));
    let _ = js_sys::Reflect::set(&obj, &"name".into(), &JsValue::from_str(name));
    let _ = js_sys::Reflect::set(&obj, &"inode_type".into(), &JsValue::from_str("Directory"));
    let _ = js_sys::Reflect::set(&obj, &"owner_id".into(), &JsValue::null());
    let _ = js_sys::Reflect::set(&obj, &"created_at".into(), &JsValue::from_f64(now));
    let _ = js_sys::Reflect::set(&obj, &"modified_at".into(), &JsValue::from_f64(now));
    let _ = js_sys::Reflect::set(&obj, &"accessed_at".into(), &JsValue::from_f64(now));
    let _ = js_sys::Reflect::set(&obj, &"size".into(), &JsValue::from_f64(0.0));
    let _ = js_sys::Reflect::set(&obj, &"encrypted".into(), &JsValue::from_bool(false));
    let _ = js_sys::Reflect::set(&obj, &"content_hash".into(), &JsValue::null());

    // Default directory permissions
    let perms = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&perms, &"owner_read".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"owner_write".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"owner_execute".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"system_read".into(), &JsValue::from_bool(true));
    let _ = js_sys::Reflect::set(&perms, &"system_write".into(), &JsValue::from_bool(false));
    let _ = js_sys::Reflect::set(&perms, &"world_read".into(), &JsValue::from_bool(false));
    let _ = js_sys::Reflect::set(&perms, &"world_write".into(), &JsValue::from_bool(false));
    let _ = js_sys::Reflect::set(&obj, &"permissions".into(), &perms);

    obj.into()
}
