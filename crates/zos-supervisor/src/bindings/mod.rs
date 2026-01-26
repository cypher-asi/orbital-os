//! JavaScript Bindings for Browser APIs
//!
//! This module contains wasm-bindgen extern bindings to JavaScript objects
//! that provide browser functionality:
//!
//! - `axiom_storage` - IndexedDB persistence for Axiom CommitLog
//! - `keystore` - IndexedDB persistence for cryptographic keys
//! - `vfs_storage` - IndexedDB persistence for VFS (bootstrap only)
//!
//! These bindings are used during supervisor bootstrap and for Axiom syncing.
//! After bootstrap, processes access storage through syscalls routed via HAL.

pub(crate) mod axiom_storage;
pub(crate) mod keystore;
pub(crate) mod vfs_storage;
