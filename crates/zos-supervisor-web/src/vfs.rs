//! VFS Storage - IndexedDB persistence for Virtual Filesystem

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// VfsStorage JavaScript object for IndexedDB persistence
    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn init() -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn clear() -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn getInodeCount() -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn putInode(path: &str, inode: JsValue) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn getInode(path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
    pub async fn putContent(path: &str, data: &[u8]) -> JsValue;

    #[wasm_bindgen(js_namespace = VfsStorage)]
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
