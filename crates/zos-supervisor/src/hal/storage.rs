//! Storage operations for WASM HAL
//!
//! This module handles async storage operations via IndexedDB and the JavaScript ZosStorage API.
//!
//! # Safety Invariants
//!
//! ## Success Criteria
//! - Async storage operations return a unique request_id for correlation
//! - Request ID is recorded with requesting PID before JavaScript call
//! - JavaScript ZosStorage API is invoked with correct parameters
//! - Bootstrap storage operations return correct data from ZosStorage cache
//!
//! ## Acceptable Partial Failures
//! - ZosStorage unavailable: Logged, operation returns without starting
//! - Lock contention on pending_storage_requests: Operation may not be recorded
//! - Bootstrap storage not initialized: Returns NotSupported error
//!
//! ## Forbidden States
//! - Request ID reuse before completion (monotonically increasing counter)
//! - PID not recorded before async operation starts (must record first)
//! - Data truncation without explicit length limits (use bounded buffers)

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use zos_hal::{HalError, StorageRequestId};

use super::WasmHal;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

/// Helper functions to call ZosStorage methods
pub(crate) fn start_storage_read(request_id: u32, key: &str) {
    if let Some(window) = web_sys::window() {
        // Call window.ZosStorage.startRead(request_id, key)
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startRead".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &key.into()),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosStorage.startRead not available for request_id={}",
        request_id
    ));
}

pub(crate) fn start_storage_write(request_id: u32, key: &str, value: &[u8]) {
    // Log storage write details to help diagnose truncation issues
    log(&format!(
        "[wasm-hal] start_storage_write: request_id={}, key={}, value_len={}",
        request_id,
        key,
        value.len()
    ));

    if let Some(window) = web_sys::window() {
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let value_array = js_sys::Uint8Array::from(value);
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startWrite".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of3(&request_id.into(), &key.into(), &value_array),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosStorage.startWrite not available for request_id={}",
        request_id
    ));
}

pub(crate) fn start_storage_delete(request_id: u32, key: &str) {
    if let Some(window) = web_sys::window() {
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startDelete".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &key.into()),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosStorage.startDelete not available for request_id={}",
        request_id
    ));
}

pub(crate) fn start_storage_list(request_id: u32, prefix: &str) {
    if let Some(window) = web_sys::window() {
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startList".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &prefix.into()),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosStorage.startList not available for request_id={}",
        request_id
    ));
}

pub(crate) fn start_storage_exists(request_id: u32, key: &str) {
    if let Some(window) = web_sys::window() {
        let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
        if let Some(storage) = zos_storage {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startExists".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &key.into()),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosStorage.startExists not available for request_id={}",
        request_id
    ));
}

// =============================================================================
// Keystore Helper Functions (ZosKeystore)
// =============================================================================

/// Helper to call ZosKeystore.startRead
pub(crate) fn start_keystore_read(request_id: u32, key: &str) {
    if let Some(window) = web_sys::window() {
        let zos_keystore = js_sys::Reflect::get(&window, &"ZosKeystore".into()).ok();
        if let Some(storage) = zos_keystore {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startRead".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &key.into()),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosKeystore.startRead not available for request_id={}",
        request_id
    ));
}

/// Helper to call ZosKeystore.startWrite
pub(crate) fn start_keystore_write(request_id: u32, key: &str, value: &[u8]) {
    log(&format!(
        "[wasm-hal] start_keystore_write: request_id={}, key={}, value_len={}",
        request_id,
        key,
        value.len()
    ));

    if let Some(window) = web_sys::window() {
        let zos_keystore = js_sys::Reflect::get(&window, &"ZosKeystore".into()).ok();
        if let Some(storage) = zos_keystore {
            if !storage.is_undefined() {
                let value_array = js_sys::Uint8Array::from(value);
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startWrite".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of3(&request_id.into(), &key.into(), &value_array),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosKeystore.startWrite not available for request_id={}",
        request_id
    ));
}

/// Helper to call ZosKeystore.startDelete
pub(crate) fn start_keystore_delete(request_id: u32, key: &str) {
    if let Some(window) = web_sys::window() {
        let zos_keystore = js_sys::Reflect::get(&window, &"ZosKeystore".into()).ok();
        if let Some(storage) = zos_keystore {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startDelete".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &key.into()),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosKeystore.startDelete not available for request_id={}",
        request_id
    ));
}

/// Helper to call ZosKeystore.startList
pub(crate) fn start_keystore_list(request_id: u32, prefix: &str) {
    if let Some(window) = web_sys::window() {
        let zos_keystore = js_sys::Reflect::get(&window, &"ZosKeystore".into()).ok();
        if let Some(storage) = zos_keystore {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startList".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &prefix.into()),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosKeystore.startList not available for request_id={}",
        request_id
    ));
}

/// Helper to call ZosKeystore.startExists
pub(crate) fn start_keystore_exists(request_id: u32, key: &str) {
    if let Some(window) = web_sys::window() {
        let zos_keystore = js_sys::Reflect::get(&window, &"ZosKeystore".into()).ok();
        if let Some(storage) = zos_keystore {
            if !storage.is_undefined() {
                let _ = js_sys::Reflect::apply(
                    &js_sys::Reflect::get(&storage, &"startExists".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
                        .unwrap_or_else(|| js_sys::Function::new_no_args("")),
                    &storage,
                    &js_sys::Array::of2(&request_id.into(), &key.into()),
                );
                return;
            }
        }
    }
    log(&format!(
        "[wasm-hal] ZosKeystore.startExists not available for request_id={}",
        request_id
    ));
}

impl WasmHal {
    // === Async Platform Storage ===

    /// Start an async storage read operation
    pub fn do_storage_read_async(&self, pid: u64, key: &str) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_request_id();
        if !self.record_pending_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] storage_read_async: request_id={}, pid={}, key={}",
            request_id, pid, key
        ));

        // Call JavaScript to start IndexedDB operation
        start_storage_read(request_id, key);

        Ok(request_id)
    }

    /// Start an async storage write operation
    pub fn do_storage_write_async(
        &self,
        pid: u64,
        key: &str,
        value: &[u8],
    ) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_request_id();
        if !self.record_pending_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] storage_write_async: request_id={}, pid={}, key={}, len={}",
            request_id,
            pid,
            key,
            value.len()
        ));

        // Call JavaScript to start IndexedDB operation
        start_storage_write(request_id, key, value);

        Ok(request_id)
    }

    /// Start an async storage delete operation
    pub fn do_storage_delete_async(
        &self,
        pid: u64,
        key: &str,
    ) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_request_id();
        if !self.record_pending_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] storage_delete_async: request_id={}, pid={}, key={}",
            request_id, pid, key
        ));

        // Call JavaScript to start IndexedDB operation
        start_storage_delete(request_id, key);

        Ok(request_id)
    }

    /// Start an async storage list operation
    pub fn do_storage_list_async(
        &self,
        pid: u64,
        prefix: &str,
    ) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_request_id();
        if !self.record_pending_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] storage_list_async: request_id={}, pid={}, prefix={}",
            request_id, pid, prefix
        ));

        // Call JavaScript to start IndexedDB operation
        start_storage_list(request_id, prefix);

        Ok(request_id)
    }

    /// Start an async storage exists check
    pub fn do_storage_exists_async(
        &self,
        pid: u64,
        key: &str,
    ) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_request_id();
        if !self.record_pending_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] storage_exists_async: request_id={}, pid={}, key={}",
            request_id, pid, key
        ));

        // Call JavaScript to start IndexedDB operation
        start_storage_exists(request_id, key);

        Ok(request_id)
    }

    /// Get the PID associated with a storage request
    pub fn do_get_storage_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.pending_storage_requests
            .lock()
            .ok()
            .and_then(|pending| pending.get(&request_id).copied())
    }

    /// Take (remove) the PID associated with a storage request
    pub fn do_take_storage_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.pending_storage_requests
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&request_id))
    }

    // === Bootstrap Storage (Supervisor Only) ===
    // These methods use ZosStorage's synchronous cache for reads.
    // For async operations (init, writes), use vfs module's async functions directly.

    /// Check if bootstrap storage is initialized
    pub fn do_bootstrap_storage_init(&self) -> Result<bool, HalError> {
        // Check if ZosStorage is available (init happens asynchronously via vfs::init)
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    // Check if DB is initialized by looking for the db property
                    let db = js_sys::Reflect::get(&storage, &"db".into()).ok();
                    if let Some(db_val) = db {
                        return Ok(!db_val.is_null() && !db_val.is_undefined());
                    }
                }
            }
        }
        Ok(false)
    }

    /// Get an inode from bootstrap storage
    pub fn do_bootstrap_storage_get_inode(&self, path: &str) -> Result<Option<Vec<u8>>, HalError> {
        // Use ZosStorage.getInodeSync from the in-memory cache
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    // Call getInodeSync
                    let get_fn = js_sys::Reflect::get(&storage, &"getInodeSync".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok());

                    if let Some(func) = get_fn {
                        let result = func.call1(&storage, &path.into()).ok();
                        if let Some(inode) = result {
                            if !inode.is_null() && !inode.is_undefined() {
                                // Serialize to JSON
                                let json = js_sys::JSON::stringify(&inode).ok();
                                if let Some(json_str) = json {
                                    let json_string: String = json_str.into();
                                    return Ok(Some(json_string.into_bytes()));
                                }
                            }
                        }
                        return Ok(None);
                    }
                }
            }
        }
        Err(HalError::NotSupported)
    }

    /// Put an inode into bootstrap storage
    pub fn do_bootstrap_storage_put_inode(
        &self,
        path: &str,
        inode_json: &[u8],
    ) -> Result<(), HalError> {
        // Parse JSON and call ZosStorage.putInode asynchronously via spawn_local
        // Note: This returns immediately; the actual write completes asynchronously.
        // For bootstrap where waiting is required, use vfs::putInode().await directly.
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    // Parse the JSON
                    let json_str = String::from_utf8_lossy(inode_json);
                    let inode = js_sys::JSON::parse(&json_str).ok();

                    if let Some(inode_obj) = inode {
                        // Call putInode (this is async but we fire-and-forget)
                        let put_fn = js_sys::Reflect::get(&storage, &"putInode".into())
                            .ok()
                            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());

                        if let Some(func) = put_fn {
                            let _ = func.call2(&storage, &path.into(), &inode_obj);
                            return Ok(());
                        }
                    }
                }
            }
        }
        Err(HalError::NotSupported)
    }

    /// Get the count of inodes in bootstrap storage
    pub fn do_bootstrap_storage_inode_count(&self) -> Result<u64, HalError> {
        // Use ZosStorage.inodeCache.size from the in-memory cache
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    // Get inodeCache.size
                    let cache = js_sys::Reflect::get(&storage, &"inodeCache".into()).ok();
                    if let Some(cache_obj) = cache {
                        let size = js_sys::Reflect::get(&cache_obj, &"size".into()).ok();
                        if let Some(size_val) = size {
                            if let Some(n) = size_val.as_f64() {
                                return Ok(n as u64);
                            }
                        }
                    }
                }
            }
        }
        Err(HalError::NotSupported)
    }

    /// Clear bootstrap storage
    pub fn do_bootstrap_storage_clear(&self) -> Result<(), HalError> {
        // Call ZosStorage.clear() asynchronously
        // Note: This returns immediately; the actual clear completes asynchronously.
        // For bootstrap where waiting is required, use vfs::clear().await directly.
        if let Some(window) = web_sys::window() {
            let zos_storage = js_sys::Reflect::get(&window, &"ZosStorage".into()).ok();
            if let Some(storage) = zos_storage {
                if !storage.is_undefined() {
                    let clear_fn = js_sys::Reflect::get(&storage, &"clear".into())
                        .ok()
                        .and_then(|f| f.dyn_into::<js_sys::Function>().ok());

                    if let Some(func) = clear_fn {
                        let _ = func.call0(&storage);
                        return Ok(());
                    }
                }
            }
        }
        Err(HalError::NotSupported)
    }

    // === Async Keystore (KeyService Only) ===

    /// Start an async keystore read operation
    pub fn do_keystore_read_async(
        &self,
        pid: u64,
        key: &str,
    ) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_keystore_request_id();
        if !self.record_pending_keystore_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] keystore_read_async: request_id={}, pid={}, key={}",
            request_id, pid, key
        ));

        // Call JavaScript to start IndexedDB operation
        start_keystore_read(request_id, key);

        Ok(request_id)
    }

    /// Start an async keystore write operation
    pub fn do_keystore_write_async(
        &self,
        pid: u64,
        key: &str,
        value: &[u8],
    ) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_keystore_request_id();
        if !self.record_pending_keystore_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] keystore_write_async: request_id={}, pid={}, key={}, len={}",
            request_id,
            pid,
            key,
            value.len()
        ));

        // Call JavaScript to start IndexedDB operation
        start_keystore_write(request_id, key, value);

        Ok(request_id)
    }

    /// Start an async keystore delete operation
    pub fn do_keystore_delete_async(
        &self,
        pid: u64,
        key: &str,
    ) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_keystore_request_id();
        if !self.record_pending_keystore_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] keystore_delete_async: request_id={}, pid={}, key={}",
            request_id, pid, key
        ));

        // Call JavaScript to start IndexedDB operation
        start_keystore_delete(request_id, key);

        Ok(request_id)
    }

    /// Start an async keystore list operation
    pub fn do_keystore_list_async(
        &self,
        pid: u64,
        prefix: &str,
    ) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_keystore_request_id();
        if !self.record_pending_keystore_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] keystore_list_async: request_id={}, pid={}, prefix={}",
            request_id, pid, prefix
        ));

        // Call JavaScript to start IndexedDB operation
        start_keystore_list(request_id, prefix);

        Ok(request_id)
    }

    /// Start an async keystore exists check
    pub fn do_keystore_exists_async(
        &self,
        pid: u64,
        key: &str,
    ) -> Result<StorageRequestId, HalError> {
        let request_id = self.next_keystore_request_id();
        if !self.record_pending_keystore_request(request_id, pid) {
            return Err(HalError::ResourceExhausted);
        }

        log(&format!(
            "[wasm-hal] keystore_exists_async: request_id={}, pid={}, key={}",
            request_id, pid, key
        ));

        // Call JavaScript to start IndexedDB operation
        start_keystore_exists(request_id, key);

        Ok(request_id)
    }

    /// Get the PID associated with a keystore request
    pub fn do_get_keystore_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.pending_keystore_requests
            .lock()
            .ok()
            .and_then(|pending| pending.get(&request_id).copied())
    }

    /// Take (remove) the PID associated with a keystore request
    pub fn do_take_keystore_request_pid(&self, request_id: StorageRequestId) -> Option<u64> {
        self.pending_keystore_requests
            .lock()
            .ok()
            .and_then(|mut pending| pending.remove(&request_id))
    }
}
