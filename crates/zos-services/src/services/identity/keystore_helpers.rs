//! Keystore Service Client Helpers
//!
//! Async operation starters for Keystore IPC requests.
//! These methods initiate async operations and track them in pending maps.
//!
//! The identity service uses the KeystoreService for all `/keys/` paths,
//! keeping cryptographic key material separate from the general filesystem.
//!
//! # Rule 11 Compliance: Resource & DoS Protection
//!
//! All keystore operations are bounded by MAX_PENDING_KEYSTORE_OPS limits.

use alloc::format;

use super::pending::PendingKeystoreOp;
use super::{IdentityService, MAX_PENDING_KEYSTORE_OPS};
use zos_apps::syscall;
use zos_apps::AppError;
use zos_vfs::client::keystore_async;

impl IdentityService {
    // =========================================================================
    // Keystore IPC helpers (async, non-blocking)
    // =========================================================================
    //
    // All key storage operations route through Keystore Service (PID 7) via IPC.

    /// Start async keystore read and track the pending operation.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_KEYSTORE_OPS limit to prevent unbounded resource growth.
    pub fn start_keystore_read(
        &mut self,
        key: &str,
        pending_op: PendingKeystoreOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_keystore_ops.len() >= MAX_PENDING_KEYSTORE_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending keystore operations ({}), rejecting read for {}",
                self.pending_keystore_ops.len(), key
            ));
            return Err(AppError::IpcError("Too many pending keystore operations".into()));
        }

        let op_id = self.next_keystore_op_id;
        self.next_keystore_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: keystore_read({}) -> op_id={}",
            key, op_id
        ));

        keystore_async::send_read_request(key)?;
        self.pending_keystore_ops.insert(op_id, pending_op);
        Ok(())
    }

    /// Start async keystore write and track the pending operation.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_KEYSTORE_OPS limit to prevent unbounded resource growth.
    pub fn start_keystore_write(
        &mut self,
        key: &str,
        value: &[u8],
        pending_op: PendingKeystoreOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_keystore_ops.len() >= MAX_PENDING_KEYSTORE_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending keystore operations ({}), rejecting write for {}",
                self.pending_keystore_ops.len(), key
            ));
            return Err(AppError::IpcError("Too many pending keystore operations".into()));
        }

        let op_id = self.next_keystore_op_id;
        self.next_keystore_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: keystore_write({}, {} bytes) -> op_id={}",
            key,
            value.len(),
            op_id
        ));

        keystore_async::send_write_request(key, value)?;
        self.pending_keystore_ops.insert(op_id, pending_op);
        Ok(())
    }

    /// Start async keystore delete and track the pending operation.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_KEYSTORE_OPS limit to prevent unbounded resource growth.
    pub fn start_keystore_delete(
        &mut self,
        key: &str,
        pending_op: PendingKeystoreOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_keystore_ops.len() >= MAX_PENDING_KEYSTORE_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending keystore operations ({}), rejecting delete for {}",
                self.pending_keystore_ops.len(), key
            ));
            return Err(AppError::IpcError("Too many pending keystore operations".into()));
        }

        let op_id = self.next_keystore_op_id;
        self.next_keystore_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: keystore_delete({}) -> op_id={}",
            key, op_id
        ));

        keystore_async::send_delete_request(key)?;
        self.pending_keystore_ops.insert(op_id, pending_op);
        Ok(())
    }

    /// Start async keystore exists check and track the pending operation.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_KEYSTORE_OPS limit to prevent unbounded resource growth.
    pub fn start_keystore_exists(
        &mut self,
        key: &str,
        pending_op: PendingKeystoreOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_keystore_ops.len() >= MAX_PENDING_KEYSTORE_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending keystore operations ({}), rejecting exists for {}",
                self.pending_keystore_ops.len(), key
            ));
            return Err(AppError::IpcError("Too many pending keystore operations".into()));
        }

        let op_id = self.next_keystore_op_id;
        self.next_keystore_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: keystore_exists({}) -> op_id={}",
            key, op_id
        ));

        keystore_async::send_exists_request(key)?;
        self.pending_keystore_ops.insert(op_id, pending_op);
        Ok(())
    }

    /// Start async keystore list and track the pending operation.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_KEYSTORE_OPS limit to prevent unbounded resource growth.
    pub fn start_keystore_list(
        &mut self,
        prefix: &str,
        pending_op: PendingKeystoreOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_keystore_ops.len() >= MAX_PENDING_KEYSTORE_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending keystore operations ({}), rejecting list for {}",
                self.pending_keystore_ops.len(), prefix
            ));
            return Err(AppError::IpcError("Too many pending keystore operations".into()));
        }

        let op_id = self.next_keystore_op_id;
        self.next_keystore_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: keystore_list({}) -> op_id={}",
            prefix, op_id
        ));

        keystore_async::send_list_request(prefix)?;
        self.pending_keystore_ops.insert(op_id, pending_op);
        Ok(())
    }
}
