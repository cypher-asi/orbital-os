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
use zos_process::{storage_result, MSG_STORAGE_RESULT};
use zos_vfs::ipc::{
    vfs_msg, ExistsRequest, ExistsResponse, MkdirRequest, MkdirResponse, ReadFileRequest,
    ReadFileResponse, ReaddirRequest, ReaddirResponse, RmdirRequest, RmdirResponse, StatRequest,
    StatResponse, UnlinkRequest, UnlinkResponse, WriteFileRequest, WriteFileResponse,
};
use zos_vfs::types::{DirEntry, Inode};
use zos_vfs::{parent_path, VfsError};

// =============================================================================
// Pending Storage Operations
// =============================================================================

/// Tracks pending storage operations awaiting results
#[derive(Clone)]
enum PendingOp {
    /// Get inode for stat/exists/directory operations
    GetInode {
        client_pid: u32,
        path: String,
        op_type: InodeOpType,
    },
    /// Get file content for read
    GetContent {
        client_pid: u32,
        path: String,
    },
    /// Put inode (after put, send response)
    PutInode {
        client_pid: u32,
        response_tag: u32,
    },
    /// Put content (after put, send response)
    PutContent {
        client_pid: u32,
        path: String,
    },
    /// Delete inode
    DeleteInode {
        client_pid: u32,
        response_tag: u32,
    },
    /// Delete content
    DeleteContent {
        client_pid: u32,
        path: String,
    },
    /// List children for readdir
    ListChildren {
        client_pid: u32,
        path: String,
    },
    /// Check exists
    ExistsCheck {
        client_pid: u32,
        path: String,
    },
}

/// Type of inode operation
#[derive(Clone)]
enum InodeOpType {
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
pub struct VfsService {
    /// Whether we have registered with init
    registered: bool,
    /// Pending storage operations: request_id -> operation context
    pending_ops: BTreeMap<u32, PendingOp>,
}

impl Default for VfsService {
    fn default() -> Self {
        Self {
            registered: false,
            pending_ops: BTreeMap::new(),
        }
    }
}

impl VfsService {
    // =========================================================================
    // Storage syscall helpers
    // =========================================================================

    /// Start async storage read and track the pending operation
    fn start_storage_read(&mut self, key: &str, pending_op: PendingOp) -> Result<(), AppError> {
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
    fn start_storage_write(
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
    fn start_storage_delete(&mut self, key: &str, pending_op: PendingOp) -> Result<(), AppError> {
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
    fn start_storage_list(&mut self, prefix: &str, pending_op: PendingOp) -> Result<(), AppError> {
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
    fn start_storage_exists(&mut self, key: &str, pending_op: PendingOp) -> Result<(), AppError> {
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
    // Request handlers (start async operations)
    // =========================================================================

    /// Handle MSG_VFS_STAT - get inode info
    fn handle_stat(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: StatRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = StatResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(msg.from_pid, vfs_msg::MSG_VFS_STAT_RESPONSE, &response);
            }
        };

        syscall::debug(&format!("VfsService: stat {}", request.path));

        // Start async inode read
        self.start_storage_read(
            &format!("inode:{}", request.path),
            PendingOp::GetInode {
                client_pid: msg.from_pid,
                path: request.path,
                op_type: InodeOpType::Stat,
            },
        )
    }

    /// Handle MSG_VFS_EXISTS - check if path exists
    fn handle_exists(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: ExistsRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(_) => {
                let response = ExistsResponse { exists: false };
                return self.send_response_via_debug(msg.from_pid, vfs_msg::MSG_VFS_EXISTS_RESPONSE, &response);
            }
        };

        syscall::debug(&format!("VfsService: exists {}", request.path));

        // Start async exists check
        self.start_storage_exists(
            &format!("inode:{}", request.path),
            PendingOp::ExistsCheck {
                client_pid: msg.from_pid,
                path: request.path,
            },
        )
    }

    /// Handle MSG_VFS_READ - read file content
    fn handle_read(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: ReadFileRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = ReadFileResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(msg.from_pid, vfs_msg::MSG_VFS_READ_RESPONSE, &response);
            }
        };

        syscall::debug(&format!("VfsService: read {}", request.path));

        // First check inode exists and is a file
        self.start_storage_read(
            &format!("inode:{}", request.path),
            PendingOp::GetInode {
                client_pid: msg.from_pid,
                path: request.path,
                op_type: InodeOpType::ReadFile,
            },
        )
    }

    /// Handle MSG_VFS_WRITE - write file content
    fn handle_write(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: WriteFileRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = WriteFileResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(msg.from_pid, vfs_msg::MSG_VFS_WRITE_RESPONSE, &response);
            }
        };

        syscall::debug(&format!(
            "VfsService: write {} ({} bytes)",
            request.path,
            request.content.len()
        ));

        // Check parent exists
        let parent = parent_path(&request.path);
        self.start_storage_read(
            &format!("inode:{}", parent),
            PendingOp::GetInode {
                client_pid: msg.from_pid,
                path: request.path,
                op_type: InodeOpType::WriteFileCheckParent {
                    content: request.content,
                },
            },
        )
    }

    /// Handle MSG_VFS_MKDIR - create directory
    fn handle_mkdir(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: MkdirRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = MkdirResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(msg.from_pid, vfs_msg::MSG_VFS_MKDIR_RESPONSE, &response);
            }
        };

        syscall::debug(&format!("VfsService: mkdir {}", request.path));

        // First check if already exists
        self.start_storage_exists(
            &format!("inode:{}", request.path),
            PendingOp::GetInode {
                client_pid: msg.from_pid,
                path: request.path,
                op_type: InodeOpType::MkdirCheckParent {
                    create_parents: request.create_parents,
                },
            },
        )
    }

    /// Handle MSG_VFS_RMDIR - remove directory
    fn handle_rmdir(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: RmdirRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = RmdirResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(msg.from_pid, vfs_msg::MSG_VFS_RMDIR_RESPONSE, &response);
            }
        };

        syscall::debug(&format!("VfsService: rmdir {}", request.path));

        // Check inode exists and is directory
        self.start_storage_read(
            &format!("inode:{}", request.path),
            PendingOp::GetInode {
                client_pid: msg.from_pid,
                path: request.path,
                op_type: InodeOpType::Rmdir {
                    recursive: request.recursive,
                },
            },
        )
    }

    /// Handle MSG_VFS_UNLINK - delete file
    fn handle_unlink(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: UnlinkRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = UnlinkResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(msg.from_pid, vfs_msg::MSG_VFS_UNLINK_RESPONSE, &response);
            }
        };

        syscall::debug(&format!("VfsService: unlink {}", request.path));

        // Check inode exists and is file
        self.start_storage_read(
            &format!("inode:{}", request.path),
            PendingOp::GetInode {
                client_pid: msg.from_pid,
                path: request.path,
                op_type: InodeOpType::Unlink,
            },
        )
    }

    /// Handle MSG_VFS_READDIR - list directory
    fn handle_readdir(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: ReaddirRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = ReaddirResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(msg.from_pid, vfs_msg::MSG_VFS_READDIR_RESPONSE, &response);
            }
        };

        syscall::debug(&format!("VfsService: readdir {}", request.path));

        // List children
        self.start_storage_list(
            &format!("inode:{}", request.path),
            PendingOp::ListChildren {
                client_pid: msg.from_pid,
                path: request.path,
            },
        )
    }

    // =========================================================================
    // Storage result handler
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
            } => {
                self.handle_inode_result(ctx, client_pid, &path, op_type, result_type, data)
            }
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

    /// Handle inode read result
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
            InodeOpType::Stat => {
                let response = if result_type == storage_result::READ_OK {
                    match serde_json::from_slice::<Inode>(data) {
                        Ok(inode) => StatResponse { result: Ok(inode) },
                        Err(e) => StatResponse {
                            result: Err(VfsError::StorageError(e.to_string())),
                        },
                    }
                } else if result_type == storage_result::NOT_FOUND {
                    StatResponse {
                        result: Err(VfsError::NotFound),
                    }
                } else {
                    StatResponse {
                        result: Err(VfsError::StorageError(String::from_utf8_lossy(data).to_string())),
                    }
                };
                self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_STAT_RESPONSE, &response)
            }

            InodeOpType::Exists => {
                let exists = result_type == storage_result::READ_OK;
                let response = ExistsResponse { exists };
                self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_EXISTS_RESPONSE, &response)
            }

            InodeOpType::ReadFile => {
                if result_type == storage_result::READ_OK {
                    // Parse inode and verify it's a file
                    match serde_json::from_slice::<Inode>(data) {
                        Ok(inode) if inode.is_file() => {
                            // Now get the content
                            self.start_storage_read(
                                &format!("content:{}", path),
                                PendingOp::GetContent {
                                    client_pid,
                                    path: path.to_string(),
                                },
                            )
                        }
                        Ok(_) => {
                            let response = ReadFileResponse {
                                result: Err(VfsError::NotAFile),
                            };
                            self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_READ_RESPONSE, &response)
                        }
                        Err(e) => {
                            let response = ReadFileResponse {
                                result: Err(VfsError::StorageError(e.to_string())),
                            };
                            self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_READ_RESPONSE, &response)
                        }
                    }
                } else if result_type == storage_result::NOT_FOUND {
                    let response = ReadFileResponse {
                        result: Err(VfsError::NotFound),
                    };
                    self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_READ_RESPONSE, &response)
                } else {
                    let response = ReadFileResponse {
                        result: Err(VfsError::StorageError(String::from_utf8_lossy(data).to_string())),
                    };
                    self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_READ_RESPONSE, &response)
                }
            }

            InodeOpType::MkdirCheckParent { create_parents: _ } => {
                // This was checking if the path already exists (via exists check)
                if result_type == storage_result::EXISTS_OK {
                    // data[0] tells us if it exists
                    let exists = !data.is_empty() && data[0] == 1;
                    if exists {
                        let response = MkdirResponse {
                            result: Err(VfsError::AlreadyExists),
                        };
                        return self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_MKDIR_RESPONSE, &response);
                    }
                }
                
                // Create the directory inode
                let name = path.rsplit('/').next().unwrap_or(path).to_string();
                let parent = parent_path(path);
                let now = syscall::get_wallclock();
                let inode = Inode::new_directory(path.to_string(), parent, name, None, now);
                
                let inode_json = match serde_json::to_vec(&inode) {
                    Ok(j) => j,
                    Err(e) => {
                        let response = MkdirResponse {
                            result: Err(VfsError::StorageError(e.to_string())),
                        };
                        return self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_MKDIR_RESPONSE, &response);
                    }
                };
                
                self.start_storage_write(
                    &format!("inode:{}", path),
                    &inode_json,
                    PendingOp::PutInode {
                        client_pid,
                        response_tag: vfs_msg::MSG_VFS_MKDIR_RESPONSE,
                    },
                )
            }

            InodeOpType::WriteFileCheckParent { content } => {
                if result_type == storage_result::NOT_FOUND {
                    let response = WriteFileResponse {
                        result: Err(VfsError::NotFound),
                    };
                    return self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_WRITE_RESPONSE, &response);
                }

                // Create/update file inode
                let name = path.rsplit('/').next().unwrap_or(path).to_string();
                let parent = parent_path(path);
                let now = syscall::get_wallclock();
                let inode = Inode::new_file(
                    path.to_string(),
                    parent,
                    name,
                    None,
                    content.len() as u64,
                    None,
                    now,
                );

                let inode_json = match serde_json::to_vec(&inode) {
                    Ok(j) => j,
                    Err(e) => {
                        let response = WriteFileResponse {
                            result: Err(VfsError::StorageError(e.to_string())),
                        };
                        return self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_WRITE_RESPONSE, &response);
                    }
                };

                // Store inode first, then content
                // For simplicity, we'll write both and respond after content write
                let _ = self.start_storage_write(
                    &format!("inode:{}", path),
                    &inode_json,
                    PendingOp::PutInode {
                        client_pid: 0, // Don't respond yet
                        response_tag: 0,
                    },
                );

                self.start_storage_write(
                    &format!("content:{}", path),
                    &content,
                    PendingOp::PutContent {
                        client_pid,
                        path: path.to_string(),
                    },
                )
            }

            InodeOpType::Rmdir { recursive: _ } => {
                if result_type == storage_result::NOT_FOUND {
                    let response = RmdirResponse {
                        result: Err(VfsError::NotFound),
                    };
                    return self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_RMDIR_RESPONSE, &response);
                }

                // Parse inode and check it's a directory
                match serde_json::from_slice::<Inode>(data) {
                    Ok(inode) if inode.is_directory() => {
                        // Delete the inode (simplified - not handling recursive or empty check)
                        self.start_storage_delete(
                            &format!("inode:{}", path),
                            PendingOp::DeleteInode {
                                client_pid,
                                response_tag: vfs_msg::MSG_VFS_RMDIR_RESPONSE,
                            },
                        )
                    }
                    Ok(_) => {
                        let response = RmdirResponse {
                            result: Err(VfsError::NotADirectory),
                        };
                        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_RMDIR_RESPONSE, &response)
                    }
                    Err(e) => {
                        let response = RmdirResponse {
                            result: Err(VfsError::StorageError(e.to_string())),
                        };
                        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_RMDIR_RESPONSE, &response)
                    }
                }
            }

            InodeOpType::Unlink => {
                if result_type == storage_result::NOT_FOUND {
                    let response = UnlinkResponse {
                        result: Err(VfsError::NotFound),
                    };
                    return self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_UNLINK_RESPONSE, &response);
                }

                match serde_json::from_slice::<Inode>(data) {
                    Ok(inode) if inode.is_file() => {
                        // Delete content first, then inode
                        let _ = self.start_storage_delete(
                            &format!("content:{}", path),
                            PendingOp::DeleteContent {
                                client_pid: 0, // Don't respond yet
                                path: path.to_string(),
                            },
                        );
                        self.start_storage_delete(
                            &format!("inode:{}", path),
                            PendingOp::DeleteInode {
                                client_pid,
                                response_tag: vfs_msg::MSG_VFS_UNLINK_RESPONSE,
                            },
                        )
                    }
                    Ok(_) => {
                        let response = UnlinkResponse {
                            result: Err(VfsError::NotAFile),
                        };
                        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_UNLINK_RESPONSE, &response)
                    }
                    Err(e) => {
                        let response = UnlinkResponse {
                            result: Err(VfsError::StorageError(e.to_string())),
                        };
                        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_UNLINK_RESPONSE, &response)
                    }
                }
            }

            InodeOpType::Readdir => {
                // This shouldn't happen - readdir uses ListChildren
                Ok(())
            }
        }
    }

    /// Handle content read result
    fn handle_content_result(
        &self,
        client_pid: u32,
        _path: &str,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        let response = if result_type == storage_result::READ_OK {
            ReadFileResponse {
                result: Ok(data.to_vec()),
            }
        } else if result_type == storage_result::NOT_FOUND {
            // File exists but content is empty
            ReadFileResponse {
                result: Ok(Vec::new()),
            }
        } else {
            ReadFileResponse {
                result: Err(VfsError::StorageError(String::from_utf8_lossy(data).to_string())),
            }
        };
        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_READ_RESPONSE, &response)
    }

    /// Handle put inode result
    fn handle_put_inode_result(
        &self,
        client_pid: u32,
        response_tag: u32,
        result_type: u8,
    ) -> Result<(), AppError> {
        if client_pid == 0 {
            // Don't send response (part of multi-step operation)
            return Ok(());
        }

        let success = result_type == storage_result::WRITE_OK;
        
        if response_tag == vfs_msg::MSG_VFS_MKDIR_RESPONSE {
            let response = MkdirResponse {
                result: if success { Ok(()) } else { Err(VfsError::StorageError("Write failed".into())) },
            };
            self.send_response_via_debug(client_pid, response_tag, &response)
        } else {
            // Generic success for other operations
            Ok(())
        }
    }

    /// Handle put content result
    fn handle_put_content_result(
        &self,
        client_pid: u32,
        _path: &str,
        result_type: u8,
    ) -> Result<(), AppError> {
        let success = result_type == storage_result::WRITE_OK;
        let response = WriteFileResponse {
            result: if success { Ok(()) } else { Err(VfsError::StorageError("Write failed".into())) },
        };
        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_WRITE_RESPONSE, &response)
    }

    /// Handle delete inode result
    fn handle_delete_inode_result(
        &self,
        client_pid: u32,
        response_tag: u32,
        result_type: u8,
    ) -> Result<(), AppError> {
        if client_pid == 0 {
            return Ok(());
        }

        let success = result_type == storage_result::WRITE_OK;

        if response_tag == vfs_msg::MSG_VFS_RMDIR_RESPONSE {
            let response = RmdirResponse {
                result: if success { Ok(()) } else { Err(VfsError::StorageError("Delete failed".into())) },
            };
            self.send_response_via_debug(client_pid, response_tag, &response)
        } else if response_tag == vfs_msg::MSG_VFS_UNLINK_RESPONSE {
            let response = UnlinkResponse {
                result: if success { Ok(()) } else { Err(VfsError::StorageError("Delete failed".into())) },
            };
            self.send_response_via_debug(client_pid, response_tag, &response)
        } else {
            Ok(())
        }
    }

    /// Handle delete content result
    fn handle_delete_content_result(
        &self,
        _client_pid: u32,
        _path: &str,
        _result_type: u8,
    ) -> Result<(), AppError> {
        // Content delete is part of unlink - response sent after inode delete
        Ok(())
    }

    /// Handle list children result
    fn handle_list_children_result(
        &self,
        client_pid: u32,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        let response = if result_type == storage_result::LIST_OK {
            // data is JSON array of keys
            match serde_json::from_slice::<Vec<String>>(data) {
                Ok(keys) => {
                    // Convert keys to DirEntry (simplified - would need to fetch each inode)
                    let entries: Vec<DirEntry> = keys
                        .iter()
                        .map(|path| {
                            let name = path.rsplit('/').next().unwrap_or(path).to_string();
                            DirEntry {
                                name,
                                path: path.clone(),
                                is_directory: false, // Would need inode to know
                                is_symlink: false,
                                size: 0,
                                modified_at: 0,
                            }
                        })
                        .collect();
                    ReaddirResponse { result: Ok(entries) }
                }
                Err(e) => ReaddirResponse {
                    result: Err(VfsError::StorageError(e.to_string())),
                },
            }
        } else {
            ReaddirResponse {
                result: Ok(Vec::new()), // Empty for errors/not found
            }
        };
        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_READDIR_RESPONSE, &response)
    }

    /// Handle exists check result
    fn handle_exists_result(
        &self,
        client_pid: u32,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        let exists = if result_type == storage_result::EXISTS_OK {
            !data.is_empty() && data[0] == 1
        } else {
            false
        };
        let response = ExistsResponse { exists };
        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_EXISTS_RESPONSE, &response)
    }

    // =========================================================================
    // Response helpers
    // =========================================================================

    /// Send response via debug message (for supervisor to route via IPC)
    fn send_response_via_debug<T: serde::Serialize>(
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

        let _ = syscall::send(syscall::INIT_ENDPOINT_SLOT, syscall::MSG_REGISTER_SERVICE, &data);
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
