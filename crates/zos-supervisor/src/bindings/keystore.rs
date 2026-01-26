//! Keystore - IndexedDB persistence for cryptographic keys
//!
//! This module provides async access to ZosKeystore for bootstrap operations.
//! It initializes the zos-keystore IndexedDB database during supervisor boot.
//!
//! ## Why This Module Exists
//!
//! The zos-keystore database stores cryptographic key material separately from
//! the filesystem for security isolation. This module ensures the database
//! is created during boot, even if no key operations occur initially.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// ZosKeystore JavaScript object for IndexedDB persistence
    /// Note: Using unique Rust name to avoid wasm-bindgen conflict with vfs_storage::init
    #[wasm_bindgen(js_namespace = ZosKeystore, js_name = init)]
    pub async fn keystore_init() -> JsValue;
}
