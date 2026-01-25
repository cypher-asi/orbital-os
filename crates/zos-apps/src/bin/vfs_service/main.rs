//! VFS Service (PID 4)
//!
//! The VFS Service manages filesystem operations for Zero OS. It:
//! - Handles MSG_VFS_* IPC messages from processes
//! - Performs storage operations via async syscalls (routed through supervisor to IndexedDB)
//! - Responds with MSG_VFS_*_RESPONSE messages
//!
//! # Architecture
//!
//! VFS operations are event-driven using push-based async storage:
//!
//! ```text
//! Client Process (e.g. App)
//!        │
//!        │ IPC (MSG_VFS_READ)
//!        ▼
//! ┌─────────────────┐
//! │   VFS Service   │  ◄── This service
//! │   (Process)     │
//! └────────┬────────┘
//!          │
//!          │ SYS_STORAGE_READ syscall (returns request_id immediately)
//!          ▼
//! ┌─────────────────┐
//! │  Kernel/Axiom   │
//! └────────┬────────┘
//!          │
//!          │ HAL async storage
//!          ▼
//! ┌─────────────────┐
//! │   Supervisor    │  ◄── Main thread
//! └────────┬────────┘
//!          │
//!          │ ZosStorage.startRead()
//!          ▼
//! ┌─────────────────┐
//! │   IndexedDB     │  ◄── Browser storage
//! └────────┬────────┘
//!          │
//!          │ Promise resolves
//!          ▼
//! ┌─────────────────┐
//! │   Supervisor    │  ◄── notify_storage_read_complete()
//! └────────┬────────┘
//!          │
//!          │ IPC (MSG_STORAGE_RESULT)
//!          ▼
//! ┌─────────────────┐
//! │   VFS Service   │  ◄── Matches request_id, sends response to client
//! └─────────────────┘
//! ```
//!
//! # Protocol
//!
//! Processes communicate with VfsService via IPC:
//!
//! - `MSG_VFS_MKDIR (0x8000)`: Create directory
//! - `MSG_VFS_RMDIR (0x8002)`: Remove directory
//! - `MSG_VFS_READDIR (0x8004)`: List directory contents
//! - `MSG_VFS_WRITE (0x8010)`: Write file
//! - `MSG_VFS_READ (0x8012)`: Read file
//! - `MSG_VFS_UNLINK (0x8014)`: Delete file
//! - `MSG_VFS_STAT (0x8020)`: Get file/directory info
//! - `MSG_VFS_EXISTS (0x8022)`: Check if path exists

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_apps::manifest::VFS_SERVICE_MANIFEST;
use zos_apps::syscall;
use zos_apps::{app_main, AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};
use zos_process::MSG_STORAGE_RESULT;
use zos_vfs::ipc::vfs_msg;

mod handlers;

// =============================================================================
// Pending Storage Operations
// =============================================================================

/// Tracks pending storage operations awaiting results
#[derive(Clone)]
pub enum PendingOp {
    /// Get inode for stat/exists/directory operations
    GetInode {
        client_pid: u32,
        path: String,
        op_type: InodeOpType,
    },
    /// Get file content for read
    GetContent { client_pid: u32, path: String },
    /// Put inode (after put, send response)
    PutInode {
        client_pid: u32,
        response_tag: u32,
    },
    /// Put content (after put, send response)
    PutContent { client_pid: u32, path: String },
    /// Delete inode
    DeleteInode {
        client_pid: u32,
        response_tag: u32,
    },
    /// Delete content
    DeleteContent { client_pid: u32, path: String },
    /// List children for readdir
    ListChildren {
        client_pid: u32,
        #[allow(dead_code)]
        path: String,
    },
    /// Check exists
    ExistsCheck {
        client_pid: u32,
        #[allow(dead_code)]
        path: String,
    },
}

/// Type of inode operation
#[derive(Clone)]
#[allow(dead_code)]
pub enum InodeOpType {
    /// Stat request
    Stat,
    /// Exists check (just check if found)
    Exists,
    /// Read file (need to get content next)
    ReadFile,
    /// Mkdir check parent exists
    MkdirCheckParent { create_parents: bool },
    /// Write file check parent exists
    WriteFileCheckParent { content: Vec<u8> },
    /// Rmdir check inode is directory
    Rmdir { recursive: bool },
    /// Unlink check inode is file
    Unlink,
    /// Readdir get children
    Readdir,
}

// =============================================================================
// VfsService Application
// =============================================================================

/// VFS Service - manages filesystem operations
#[derive(Default)]
pub struct VfsService {
    /// Whether we have registered with init
    registered: bool,
    /// Pending storage operations: request_id -> operation context
    pending_ops: BTreeMap<u32, PendingOp>,
}

impl VfsService {
    // =========================================================================
    // Storage syscall helpers
    // =========================================================================

    /// Start async storage read and track the pending operation
    pub fn start_storage_read(
        &mut self,
        key: &str,
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        match syscall::storage_read_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "VfsService: storage_read_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("VfsService: storage_read_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage read failed: {}", e)))
            }
        }
    }

    /// Start async storage write and track the pending operation
    pub fn start_storage_write(
        &mut self,
        key: &str,
        value: &[u8],
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        match syscall::storage_write_async(key, value) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "VfsService: storage_write_async({}, {} bytes) -> request_id={}",
                    key,
                    value.len(),
                    request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("VfsService: storage_write_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage write failed: {}", e)))
            }
        }
    }

    /// Start async storage delete and track the pending operation
    pub fn start_storage_delete(
        &mut self,
        key: &str,
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        match syscall::storage_delete_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "VfsService: storage_delete_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("VfsService: storage_delete_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage delete failed: {}", e)))
            }
        }
    }

    /// Start async storage list and track the pending operation
    pub fn start_storage_list(
        &mut self,
        prefix: &str,
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        match syscall::storage_list_async(prefix) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "VfsService: storage_list_async({}) -> request_id={}",
                    prefix, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("VfsService: storage_list_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage list failed: {}", e)))
            }
        }
    }

    /// Start async storage exists and track the pending operation
    pub fn start_storage_exists(
        &mut self,
        key: &str,
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        match syscall::storage_exists_async(key) {
            Ok(request_id) => {
                syscall::debug(&format!(
                    "VfsService: storage_exists_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("VfsService: storage_exists_async failed: {}", e));
                Err(AppError::IpcError(format!("Storage exists failed: {}", e)))
            }
        }
    }

    // =========================================================================
    // Storage result handler (main dispatcher)
    // =========================================================================

    /// Handle MSG_STORAGE_RESULT - async storage operation completed
    fn handle_storage_result(&mut self, ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse storage result
        // Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
        if msg.data.len() < 9 {
            syscall::debug("VfsService: storage result too short");
            return Ok(());
        }

        let request_id = u32::from_le_bytes([msg.data[0], msg.data[1], msg.data[2], msg.data[3]]);
        let result_type = msg.data[4];
        let data_len =
            u32::from_le_bytes([msg.data[5], msg.data[6], msg.data[7], msg.data[8]]) as usize;
        let data = if data_len > 0 && msg.data.len() >= 9 + data_len {
            &msg.data[9..9 + data_len]
        } else {
            &[]
        };

        syscall::debug(&format!(
            "VfsService: storage result request_id={}, type={}, data_len={}",
            request_id, result_type, data_len
        ));

        // Look up pending operation
        let pending_op = match self.pending_ops.remove(&request_id) {
            Some(op) => op,
            None => {
                syscall::debug(&format!(
                    "VfsService: unknown request_id {}",
                    request_id
                ));
                return Ok(());
            }
        };

        // Dispatch based on operation type and result
        match pending_op {
            PendingOp::GetInode {
                client_pid,
                path,
                op_type,
            } => self.handle_inode_result(ctx, client_pid, &path, op_type, result_type, data),
            PendingOp::GetContent { client_pid, path } => {
                self.handle_content_result(client_pid, &path, result_type, data)
            }
            PendingOp::PutInode {
                client_pid,
                response_tag,
            } => self.handle_put_inode_result(client_pid, response_tag, result_type),
            PendingOp::PutContent { client_pid, path } => {
                self.handle_put_content_result(client_pid, &path, result_type)
            }
            PendingOp::DeleteInode {
                client_pid,
                response_tag,
            } => self.handle_delete_inode_result(client_pid, response_tag, result_type),
            PendingOp::DeleteContent { client_pid, path } => {
                self.handle_delete_content_result(client_pid, &path, result_type)
            }
            PendingOp::ListChildren { client_pid, path: _ } => {
                self.handle_list_children_result(client_pid, result_type, data)
            }
            PendingOp::ExistsCheck { client_pid, path: _ } => {
                self.handle_exists_result(client_pid, result_type, data)
            }
        }
    }

    /// Handle inode read result - dispatches to specific handlers
    fn handle_inode_result(
        &mut self,
        _ctx: &AppContext,
        client_pid: u32,
        path: &str,
        op_type: InodeOpType,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        match op_type {
            InodeOpType::Stat => self.handle_stat_inode_result(client_pid, result_type, data),
            InodeOpType::Exists => self.handle_exists_inode_result(client_pid, result_type),
            InodeOpType::ReadFile => {
                self.handle_read_file_inode_result(client_pid, path, result_type, data)
            }
            InodeOpType::MkdirCheckParent { create_parents: _ } => {
                self.handle_mkdir_inode_result(client_pid, path, result_type, data)
            }
            InodeOpType::WriteFileCheckParent { content } => {
                self.handle_write_file_inode_result(client_pid, path, result_type, content)
            }
            InodeOpType::Rmdir { recursive: _ } => {
                self.handle_rmdir_inode_result(client_pid, path, result_type, data)
            }
            InodeOpType::Unlink => {
                self.handle_unlink_inode_result(client_pid, path, result_type, data)
            }
            InodeOpType::Readdir => Ok(()), // readdir uses ListChildren
        }
    }

    // =========================================================================
    // Response helpers
    // =========================================================================

    /// Send response via debug message (for supervisor to route via IPC)
    pub fn send_response_via_debug<T: serde::Serialize>(
        &self,
        to_pid: u32,
        tag: u32,
        response: &T,
    ) -> Result<(), AppError> {
        match serde_json::to_vec(response) {
            Ok(data) => {
                let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
                syscall::debug(&format!("VFS:RESPONSE:{}:{:08x}:{}", to_pid, tag, hex));
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("VfsService: Failed to serialize response: {}", e));
                Err(AppError::IpcError(format!("Serialization failed: {}", e)))
            }
        }
    }
}

impl ZeroApp for VfsService {
    fn manifest() -> &'static AppManifest {
        &VFS_SERVICE_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug(&format!("VfsService starting (PID {})", ctx.pid));

        // Register with init as "vfs" service
        let service_name = "vfs";
        let name_bytes = service_name.as_bytes();
        let mut data = Vec::with_capacity(1 + name_bytes.len() + 8);
        data.push(name_bytes.len() as u8);
        data.extend_from_slice(name_bytes);
        // Endpoint ID (placeholder)
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        let _ = syscall::send(
            syscall::INIT_ENDPOINT_SLOT,
            syscall::MSG_REGISTER_SERVICE,
            &data,
        );
        self.registered = true;

        syscall::debug("VfsService: Registered with init");

        Ok(())
    }

    fn update(&mut self, _ctx: &AppContext) -> ControlFlow {
        ControlFlow::Yield
    }

    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        syscall::debug(&format!(
            "VfsService: Received message tag 0x{:x} from PID {}",
            msg.tag, msg.from_pid
        ));

        match msg.tag {
            MSG_STORAGE_RESULT => self.handle_storage_result(ctx, &msg),
            vfs_msg::MSG_VFS_MKDIR => self.handle_mkdir(ctx, &msg),
            vfs_msg::MSG_VFS_RMDIR => self.handle_rmdir(ctx, &msg),
            vfs_msg::MSG_VFS_READDIR => self.handle_readdir(ctx, &msg),
            vfs_msg::MSG_VFS_WRITE => self.handle_write(ctx, &msg),
            vfs_msg::MSG_VFS_READ => self.handle_read(ctx, &msg),
            vfs_msg::MSG_VFS_UNLINK => self.handle_unlink(ctx, &msg),
            vfs_msg::MSG_VFS_STAT => self.handle_stat(ctx, &msg),
            vfs_msg::MSG_VFS_EXISTS => self.handle_exists(ctx, &msg),
            _ => {
                syscall::debug(&format!(
                    "VfsService: Unknown message tag 0x{:x}",
                    msg.tag
                ));
                Ok(())
            }
        }
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("VfsService: shutting down");
    }
}

// Entry point
app_main!(VfsService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("VfsService is meant to run as WASM in Zero OS");
}
