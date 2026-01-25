//! Storage result handlers for identity service.
//!
//! This module contains handlers for async storage operation results.
//! Each handler is a focused function that processes a specific pending operation type.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::AppError;
use crate::identity::pending::PendingStorageOp;

// Re-export all handler functions from storage module
pub use super::storage::*;

/// Result of handling a storage operation.
pub enum StorageHandlerResult {
    /// Operation complete, no further action needed
    Done(Result<(), AppError>),
    /// Need to start another storage write operation
    ContinueWrite {
        key: String,
        value: Vec<u8>,
        next_op: PendingStorageOp,
    },
    /// Need to start another storage read operation
    ContinueRead {
        key: String,
        next_op: PendingStorageOp,
    },
    /// Need to start another storage delete operation
    ContinueDelete {
        key: String,
        next_op: PendingStorageOp,
    },
}
