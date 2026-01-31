//! VFS and Network Helper Methods
//!
//! Async operation starters for VFS IPC and network requests.
//! These methods initiate async operations and track them in pending maps.
//!
//! # Rule 11 Compliance: Resource & DoS Protection
//!
//! All VFS and network operations are bounded by MAX_PENDING_*_OPS limits.
//! Exceeding these limits results in immediate rejection with an error.

use alloc::format;

use super::pending::{PendingNetworkOp, PendingStorageOp};
use super::{IdentityService, MAX_PENDING_VFS_OPS, MAX_PENDING_NET_OPS};
use zos_apps::syscall;
use zos_apps::AppError;
use zos_network::HttpRequest;
use zos_vfs::async_client;

impl IdentityService {
    // =========================================================================
    // VFS IPC helpers (async, non-blocking) - Invariant 31 compliant
    // =========================================================================
    //
    // All storage operations route through VFS Service (PID 4) via IPC.

    /// Start async VFS read and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_VFS_OPS limit to prevent unbounded resource growth.
    pub fn start_vfs_read(
        &mut self,
        path: &str,
        pending_op: PendingStorageOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_vfs_ops.len() >= MAX_PENDING_VFS_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending VFS operations ({}), rejecting read for {}",
                self.pending_vfs_ops.len(), path
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        let op_id = self.next_vfs_op_id;
        self.next_vfs_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: vfs_read({}) -> op_id={}",
            path, op_id
        ));

        async_client::send_read_request(path)?;
        self.pending_vfs_ops.insert(op_id, pending_op);
        Ok(())
    }

    /// Start async VFS write and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_VFS_OPS limit to prevent unbounded resource growth.
    pub fn start_vfs_write(
        &mut self,
        path: &str,
        value: &[u8],
        pending_op: PendingStorageOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_vfs_ops.len() >= MAX_PENDING_VFS_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending VFS operations ({}), rejecting write for {}",
                self.pending_vfs_ops.len(), path
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        let op_id = self.next_vfs_op_id;
        self.next_vfs_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: vfs_write({}, {} bytes) -> op_id={}",
            path,
            value.len(),
            op_id
        ));

        async_client::send_write_request(path, value)?;
        self.pending_vfs_ops.insert(op_id, pending_op);
        Ok(())
    }

    /// Start async VFS delete (unlink) and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_VFS_OPS limit to prevent unbounded resource growth.
    pub fn start_vfs_delete(
        &mut self,
        path: &str,
        pending_op: PendingStorageOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_vfs_ops.len() >= MAX_PENDING_VFS_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending VFS operations ({}), rejecting delete for {}",
                self.pending_vfs_ops.len(), path
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        let op_id = self.next_vfs_op_id;
        self.next_vfs_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: vfs_unlink({}) -> op_id={}",
            path, op_id
        ));

        async_client::send_unlink_request(path)?;
        self.pending_vfs_ops.insert(op_id, pending_op);
        Ok(())
    }

    /// Start async VFS exists check and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_VFS_OPS limit to prevent unbounded resource growth.
    pub fn start_vfs_exists(
        &mut self,
        path: &str,
        pending_op: PendingStorageOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_vfs_ops.len() >= MAX_PENDING_VFS_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending VFS operations ({}), rejecting exists for {}",
                self.pending_vfs_ops.len(), path
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        let op_id = self.next_vfs_op_id;
        self.next_vfs_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: vfs_exists({}) -> op_id={}",
            path, op_id
        ));

        async_client::send_exists_request(path)?;
        self.pending_vfs_ops.insert(op_id, pending_op);
        Ok(())
    }

    /// Start async VFS readdir (list directory) and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_VFS_OPS limit to prevent unbounded resource growth.
    pub fn start_vfs_readdir(
        &mut self,
        path: &str,
        pending_op: PendingStorageOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_vfs_ops.len() >= MAX_PENDING_VFS_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending VFS operations ({}), rejecting readdir for {}",
                self.pending_vfs_ops.len(), path
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        let op_id = self.next_vfs_op_id;
        self.next_vfs_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: vfs_readdir({}) -> op_id={}",
            path, op_id
        ));

        async_client::send_readdir_request(path)?;
        self.pending_vfs_ops.insert(op_id, pending_op);
        Ok(())
    }

    /// Start async VFS mkdir and track the pending operation.
    /// Uses VFS IPC instead of direct storage syscalls per Invariant 31.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_VFS_OPS limit to prevent unbounded resource growth.
    pub fn start_vfs_mkdir(
        &mut self,
        path: &str,
        create_parents: bool,
        pending_op: PendingStorageOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_vfs_ops.len() >= MAX_PENDING_VFS_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending VFS operations ({}), rejecting mkdir for {}",
                self.pending_vfs_ops.len(), path
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        let op_id = self.next_vfs_op_id;
        self.next_vfs_op_id += 1;

        syscall::debug(&format!(
            "IdentityService: vfs_mkdir({}, create_parents={}) -> op_id={}",
            path, create_parents, op_id
        ));

        async_client::send_mkdir_request(path, create_parents)?;
        self.pending_vfs_ops.insert(op_id, pending_op);
        Ok(())
    }

    // =========================================================================
    // Network syscall helpers (async, non-blocking)
    // =========================================================================

    /// Start async network fetch and track the pending operation.
    ///
    /// # Rule 11 Compliance
    /// Enforces MAX_PENDING_NET_OPS limit to prevent unbounded resource growth.
    pub fn start_network_fetch(
        &mut self,
        request: &HttpRequest,
        pending_op: PendingNetworkOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_net_ops.len() >= MAX_PENDING_NET_OPS {
            syscall::debug(&format!(
                "IdentityService: Too many pending network operations ({}), rejecting fetch for {}",
                self.pending_net_ops.len(), request.url
            ));
            return Err(AppError::IpcError("Too many pending network operations".into()));
        }

        let request_json = match serde_json::to_vec(request) {
            Ok(json) => json,
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: Failed to serialize HTTP request: {}",
                    e
                ));
                return Err(AppError::IpcError(format!(
                    "Request serialization failed: {}",
                    e
                )));
            }
        };

        match syscall::network_fetch_async(&request_json) {
            Ok(request_id) => {
                let request_id = request_id as u32;
                syscall::debug(&format!(
                    "IdentityService: network_fetch_async({} {}) -> request_id={}",
                    request.method.as_str(),
                    request.url,
                    request_id
                ));
                self.pending_net_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!(
                    "IdentityService: network_fetch_async failed: {}",
                    e
                ));
                Err(AppError::IpcError(format!("Network fetch failed: {}", e)))
            }
        }
    }
}
