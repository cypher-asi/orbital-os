//! Write operation handlers for VFS Service
//!
//! Handles: write, mkdir operations

use alloc::format;
use alloc::vec::Vec;
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, Message};
use zos_process::storage_result;
use zos_vfs::ipc::{
    vfs_msg, MkdirRequest, MkdirResponse, WriteFileRequest, WriteFileResponse,
};
use zos_vfs::types::Inode;
use zos_vfs::{parent_path, VfsError};

use super::super::{InodeOpType, PendingOp, VfsService};

impl VfsService {
    // =========================================================================
    // Request handlers (start async operations)
    // =========================================================================

    /// Handle MSG_VFS_WRITE - write file content
    pub fn handle_write(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: WriteFileRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = WriteFileResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    vfs_msg::MSG_VFS_WRITE_RESPONSE,
                    &response,
                );
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
    pub fn handle_mkdir(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: MkdirRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = MkdirResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    vfs_msg::MSG_VFS_MKDIR_RESPONSE,
                    &response,
                );
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

    // =========================================================================
    // Result handlers
    // =========================================================================

    /// Handle mkdir inode result (checking if path already exists)
    pub fn handle_mkdir_inode_result(
        &mut self,
        client_pid: u32,
        path: &str,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        if result_type == storage_result::EXISTS_OK {
            let exists = !data.is_empty() && data[0] == 1;
            if exists {
                let response = MkdirResponse {
                    result: Err(VfsError::AlreadyExists),
                };
                return self.send_response_via_debug(
                    client_pid,
                    vfs_msg::MSG_VFS_MKDIR_RESPONSE,
                    &response,
                );
            }
        }

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
                return self.send_response_via_debug(
                    client_pid,
                    vfs_msg::MSG_VFS_MKDIR_RESPONSE,
                    &response,
                );
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

    /// Handle write file inode result (checking parent exists)
    pub fn handle_write_file_inode_result(
        &mut self,
        client_pid: u32,
        path: &str,
        result_type: u8,
        content: Vec<u8>,
    ) -> Result<(), AppError> {
        if result_type == storage_result::NOT_FOUND {
            let response = WriteFileResponse {
                result: Err(VfsError::NotFound),
            };
            return self.send_response_via_debug(
                client_pid,
                vfs_msg::MSG_VFS_WRITE_RESPONSE,
                &response,
            );
        }

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
                return self.send_response_via_debug(
                    client_pid,
                    vfs_msg::MSG_VFS_WRITE_RESPONSE,
                    &response,
                );
            }
        };

        // Store inode first, then content
        let _ = self.start_storage_write(
            &format!("inode:{}", path),
            &inode_json,
            PendingOp::PutInode {
                client_pid: 0,
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

    /// Handle put inode result
    pub fn handle_put_inode_result(
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
                result: if success {
                    Ok(())
                } else {
                    Err(VfsError::StorageError("Write failed".into()))
                },
            };
            self.send_response_via_debug(client_pid, response_tag, &response)
        } else {
            // Generic success for other operations
            Ok(())
        }
    }

    /// Handle put content result
    pub fn handle_put_content_result(
        &self,
        client_pid: u32,
        _path: &str,
        result_type: u8,
    ) -> Result<(), AppError> {
        let success = result_type == storage_result::WRITE_OK;
        let response = WriteFileResponse {
            result: if success {
                Ok(())
            } else {
                Err(VfsError::StorageError("Write failed".into()))
            },
        };
        self.send_response_via_debug(client_pid, vfs_msg::MSG_VFS_WRITE_RESPONSE, &response)
    }
}
