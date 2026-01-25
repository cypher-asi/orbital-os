//! Read operation handlers for VFS Service
//!
//! Handles: stat, exists, read, readdir operations

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, Message};
use zos_process::storage_result;
use zos_vfs::ipc::{
    vfs_msg, ExistsRequest, ExistsResponse, ReadFileRequest, ReadFileResponse, ReaddirRequest,
    ReaddirResponse, StatRequest, StatResponse,
};
use zos_vfs::types::{DirEntry, Inode};
use zos_vfs::VfsError;

use super::super::{InodeOpType, PendingOp, VfsService};

impl VfsService {
    // =========================================================================
    // Request handlers (start async operations)
    // =========================================================================

    /// Handle MSG_VFS_STAT - get inode info
    pub fn handle_stat(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: StatRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = StatResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    vfs_msg::MSG_VFS_STAT_RESPONSE,
                    &response,
                );
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
    pub fn handle_exists(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: ExistsRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(_) => {
                let response = ExistsResponse { exists: false };
                return self.send_response_via_debug(
                    msg.from_pid,
                    vfs_msg::MSG_VFS_EXISTS_RESPONSE,
                    &response,
                );
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
    pub fn handle_read(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: ReadFileRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = ReadFileResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    vfs_msg::MSG_VFS_READ_RESPONSE,
                    &response,
                );
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

    /// Handle MSG_VFS_READDIR - list directory
    pub fn handle_readdir(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: ReaddirRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = ReaddirResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    vfs_msg::MSG_VFS_READDIR_RESPONSE,
                    &response,
                );
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
    // Result handlers
    // =========================================================================

    /// Handle stat operation inode result
    pub fn handle_stat_inode_result(
        &self,
        client_pid: u32,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
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
                result: Err(VfsError::StorageError(
                    String::from_utf8_lossy(data).to_string(),
                )),
            }
        };
        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_STAT_RESPONSE, &response)
    }

    /// Handle exists check inode result
    pub fn handle_exists_inode_result(
        &self,
        client_pid: u32,
        result_type: u8,
    ) -> Result<(), AppError> {
        let exists = result_type == storage_result::READ_OK;
        let response = ExistsResponse { exists };
        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_EXISTS_RESPONSE, &response)
    }

    /// Handle read file inode result
    pub fn handle_read_file_inode_result(
        &mut self,
        client_pid: u32,
        path: &str,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        if result_type == storage_result::READ_OK {
            match serde_json::from_slice::<Inode>(data) {
                Ok(inode) if inode.is_file() => self.start_storage_read(
                    &format!("content:{}", path),
                    PendingOp::GetContent {
                        client_pid,
                        path: path.to_string(),
                    },
                ),
                Ok(_) => {
                    let response = ReadFileResponse {
                        result: Err(VfsError::NotAFile),
                    };
                    self.send_response_via_debug(
                        client_pid,
                        vfs_msg::MSG_VFS_READ_RESPONSE,
                        &response,
                    )
                }
                Err(e) => {
                    let response = ReadFileResponse {
                        result: Err(VfsError::StorageError(e.to_string())),
                    };
                    self.send_response_via_debug(
                        client_pid,
                        vfs_msg::MSG_VFS_READ_RESPONSE,
                        &response,
                    )
                }
            }
        } else if result_type == storage_result::NOT_FOUND {
            let response = ReadFileResponse {
                result: Err(VfsError::NotFound),
            };
            self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_READ_RESPONSE, &response)
        } else {
            let response = ReadFileResponse {
                result: Err(VfsError::StorageError(
                    String::from_utf8_lossy(data).to_string(),
                )),
            };
            self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_READ_RESPONSE, &response)
        }
    }

    /// Handle content read result
    pub fn handle_content_result(
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
                result: Err(VfsError::StorageError(
                    String::from_utf8_lossy(data).to_string(),
                )),
            }
        };
        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_READ_RESPONSE, &response)
    }

    /// Handle list children result
    pub fn handle_list_children_result(
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
    pub fn handle_exists_result(
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
}
