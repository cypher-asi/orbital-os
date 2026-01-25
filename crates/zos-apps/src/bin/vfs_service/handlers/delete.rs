//! Delete operation handlers for VFS Service
//!
//! Handles: rmdir, unlink operations

use alloc::format;
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, Message};
use zos_process::storage_result;
use zos_vfs::ipc::{vfs_msg, RmdirRequest, RmdirResponse, UnlinkRequest, UnlinkResponse};
use zos_vfs::types::Inode;
use zos_vfs::VfsError;

use super::super::{InodeOpType, PendingOp, VfsService};

impl VfsService {
    // =========================================================================
    // Request handlers (start async operations)
    // =========================================================================

    /// Handle MSG_VFS_RMDIR - remove directory
    pub fn handle_rmdir(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: RmdirRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = RmdirResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    vfs_msg::MSG_VFS_RMDIR_RESPONSE,
                    &response,
                );
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
    pub fn handle_unlink(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        let request: UnlinkRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                let response = UnlinkResponse {
                    result: Err(VfsError::InvalidPath(e.to_string())),
                };
                return self.send_response_via_debug(
                    msg.from_pid,
                    vfs_msg::MSG_VFS_UNLINK_RESPONSE,
                    &response,
                );
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

    // =========================================================================
    // Result handlers
    // =========================================================================

    /// Handle rmdir inode result
    pub fn handle_rmdir_inode_result(
        &mut self,
        client_pid: u32,
        path: &str,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        if result_type == storage_result::NOT_FOUND {
            let response = RmdirResponse {
                result: Err(VfsError::NotFound),
            };
            return self.send_response_via_debug(
                client_pid,
                vfs_msg::MSG_VFS_RMDIR_RESPONSE,
                &response,
            );
        }

        match serde_json::from_slice::<Inode>(data) {
            Ok(inode) if inode.is_directory() => self.start_storage_delete(
                &format!("inode:{}", path),
                PendingOp::DeleteInode {
                    client_pid,
                    response_tag: vfs_msg::MSG_VFS_RMDIR_RESPONSE,
                },
            ),
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

    /// Handle unlink inode result
    pub fn handle_unlink_inode_result(
        &mut self,
        client_pid: u32,
        path: &str,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        if result_type == storage_result::NOT_FOUND {
            let response = UnlinkResponse {
                result: Err(VfsError::NotFound),
            };
            return self.send_response_via_debug(
                client_pid,
                vfs_msg::MSG_VFS_UNLINK_RESPONSE,
                &response,
            );
        }

        match serde_json::from_slice::<Inode>(data) {
            Ok(inode) if inode.is_file() => {
                let _ = self.start_storage_delete(
                    &format!("content:{}", path),
                    PendingOp::DeleteContent {
                        client_pid: 0,
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
                self.send_response_via_debug(
                    client_pid,
                    vfs_msg::MSG_VFS_UNLINK_RESPONSE,
                    &response,
                )
            }
            Err(e) => {
                let response = UnlinkResponse {
                    result: Err(VfsError::StorageError(e.to_string())),
                };
                self.send_response_via_debug(
                    client_pid,
                    vfs_msg::MSG_VFS_UNLINK_RESPONSE,
                    &response,
                )
            }
        }
    }

    /// Handle delete inode result
    pub fn handle_delete_inode_result(
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
                result: if success {
                    Ok(())
                } else {
                    Err(VfsError::StorageError("Delete failed".into()))
                },
            };
            self.send_response_via_debug(client_pid, response_tag, &response)
        } else if response_tag == vfs_msg::MSG_VFS_UNLINK_RESPONSE {
            let response = UnlinkResponse {
                result: if success {
                    Ok(())
                } else {
                    Err(VfsError::StorageError("Delete failed".into()))
                },
            };
            self.send_response_via_debug(client_pid, response_tag, &response)
        } else {
            Ok(())
        }
    }

    /// Handle delete content result
    pub fn handle_delete_content_result(
        &self,
        _client_pid: u32,
        _path: &str,
        _result_type: u8,
    ) -> Result<(), AppError> {
        // Content delete is part of unlink - response sent after inode delete
        Ok(())
    }
}
