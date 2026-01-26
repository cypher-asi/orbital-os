//! Write operation handlers for VFS Service
//!
//! Handles: write, mkdir operations
//!
//! # Safety Properties
//!
//! This module enforces the following security invariants:
//!
//! 1. **Fail-closed permission checks**: If parent inode cannot be parsed or is
//!    not a directory, the operation is denied (never "continue anyway").
//!
//! 2. **Strict result type handling**: Unexpected storage result types are treated
//!    as errors, not silently ignored.
//!
//! 3. **Atomic-ish writes**: Write operations respond success only after both
//!    content AND inode are committed. Content is written first, then inode.
//!    If inode fails after content succeeds, we have orphan content (acceptable)
//!    rather than an inode pointing to missing content (data loss).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, Message};
use zos_process::storage_result;
use zos_vfs::ipc::{vfs_msg, MkdirRequest, MkdirResponse, WriteFileRequest, WriteFileResponse};
use zos_vfs::service::{check_write, PermissionContext};
use zos_vfs::Inode;
use zos_vfs::{parent_path, VfsError};

use super::super::{
    content_key, derive_permission_context, inode_key, result_type_name, validate_path,
    ClientContext, MkdirStage, PendingOp, VfsService, WriteFileStage, MAX_CONTENT_SIZE,
};

impl VfsService {
    // =========================================================================
    // Response helpers (reduce boilerplate)
    // =========================================================================

    /// Send a write error response to the client.
    fn send_write_error(&self, client_ctx: &ClientContext, error: VfsError) -> Result<(), AppError> {
        let response = WriteFileResponse {
            result: Err(error),
        };
        self.send_response(client_ctx, vfs_msg::MSG_VFS_WRITE_RESPONSE, &response)
    }

    /// Send a write error response via debug channel (when no ClientContext available).
    fn send_write_error_via_debug(&self, to_pid: u32, error: VfsError) -> Result<(), AppError> {
        let response = WriteFileResponse {
            result: Err(error),
        };
        self.send_response_via_debug(to_pid, vfs_msg::MSG_VFS_WRITE_RESPONSE, &response)
    }

    /// Send a mkdir error response to the client.
    fn send_mkdir_error(&self, client_ctx: &ClientContext, error: VfsError) -> Result<(), AppError> {
        let response = MkdirResponse {
            result: Err(error),
        };
        self.send_response(client_ctx, vfs_msg::MSG_VFS_MKDIR_RESPONSE, &response)
    }

    /// Send a mkdir error response via debug channel (when no ClientContext available).
    fn send_mkdir_error_via_debug(&self, to_pid: u32, error: VfsError) -> Result<(), AppError> {
        let response = MkdirResponse {
            result: Err(error),
        };
        self.send_response_via_debug(to_pid, vfs_msg::MSG_VFS_MKDIR_RESPONSE, &response)
    }

    // =========================================================================
    // Request handlers (start async operations)
    // =========================================================================

    /// Handle MSG_VFS_WRITE - write file content
    ///
    /// This starts the write file state machine:
    /// 1. Check parent exists and is directory, check permissions
    /// 2. Write content first
    /// 3. Write inode (only after content succeeds)
    /// 4. Send response (only after inode succeeds)
    pub fn handle_write(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse request
        let request: WriteFileRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                return self.send_write_error_via_debug(
                    msg.from_pid,
                    VfsError::InvalidRequest(format!("Failed to parse request: {}", e)),
                );
            }
        };

        // Validate path
        if let Err(reason) = validate_path(&request.path) {
            return self.send_write_error_via_debug(
                msg.from_pid,
                VfsError::InvalidPath(String::from(reason)),
            );
        }

        // Rule 11: Enforce content size limit
        if request.content.len() > MAX_CONTENT_SIZE {
            return self.send_write_error_via_debug(
                msg.from_pid,
                VfsError::InvalidRequest(format!(
                    "Content too large: {} bytes exceeds limit of {} bytes",
                    request.content.len(),
                    MAX_CONTENT_SIZE
                )),
            );
        }

        // Reject writing to root
        if request.path == "/" {
            return self.send_write_error_via_debug(
                msg.from_pid,
                VfsError::InvalidPath("Cannot write to root directory".into()),
            );
        }

        syscall::debug(&format!(
            "VfsService: write {} ({} bytes)",
            request.path,
            request.content.len()
        ));

        // Derive permission context from caller
        let perm_ctx = derive_permission_context(msg.from_pid, &request.path);
        let client_ctx = ClientContext::from_message(msg);

        // Use inode/content pattern for VFS operations
        let parent = parent_path(&request.path);

        // Check parent exists (will also check write permission on parent)
        self.start_storage_read(
            &inode_key(&parent),
            PendingOp::WriteFileOp {
                ctx: client_ctx,
                path: request.path,
                perm_ctx,
                stage: WriteFileStage::CheckingParent {
                    content: request.content,
                },
            },
        )
    }

    /// Handle MSG_VFS_MKDIR - create directory
    pub fn handle_mkdir(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse request
        let request: MkdirRequest = match serde_json::from_slice(&msg.data) {
            Ok(r) => r,
            Err(e) => {
                return self.send_mkdir_error_via_debug(
                    msg.from_pid,
                    VfsError::InvalidRequest(format!("Failed to parse request: {}", e)),
                );
            }
        };

        // Rule 8: Reject create_parents flag (not implemented)
        if request.create_parents {
            return self.send_mkdir_error_via_debug(
                msg.from_pid,
                VfsError::NotSupported("create_parents not implemented".into()),
            );
        }

        // Validate path
        if let Err(reason) = validate_path(&request.path) {
            return self.send_mkdir_error_via_debug(
                msg.from_pid,
                VfsError::InvalidPath(String::from(reason)),
            );
        }

        syscall::debug(&format!("VfsService: mkdir {}", request.path));

        // Derive permission context from caller (for parent directory check)
        let perm_ctx = derive_permission_context(msg.from_pid, &request.path);
        let client_ctx = ClientContext::from_message(msg);

        // First check if already exists using dedicated exists check
        self.start_storage_exists(
            &inode_key(&request.path),
            PendingOp::MkdirOp {
                ctx: client_ctx,
                path: request.path,
                perm_ctx,
                stage: MkdirStage::CheckingExists,
            },
        )
    }

    // =========================================================================
    // Result handlers
    // =========================================================================

    /// Handle exists check result for mkdir
    ///
    /// This is the first step of mkdir: check if the path already exists.
    pub fn handle_check_exists_for_mkdir_result(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        _create_parents: bool,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        // Handle result type strictly
        match result_type {
            storage_result::EXISTS_OK => {
                let exists = !data.is_empty() && data[0] == 1;
                if exists {
                    return self.send_mkdir_error(client_ctx, VfsError::AlreadyExists);
                }
                // Path doesn't exist - proceed to create
            }
            storage_result::NOT_FOUND => {
                // Key not found = doesn't exist, proceed to create
            }
            _ => {
                // Unexpected result type - fail closed
                syscall::debug(&format!(
                    "VfsService: mkdir {} exists check failed with unexpected result: {} ({})",
                    path,
                    result_type,
                    result_type_name(result_type)
                ));
                return self.send_mkdir_error(
                    client_ctx,
                    VfsError::StorageError(format!(
                        "Exists check failed: unexpected result type {} ({})",
                        result_type,
                        result_type_name(result_type)
                    )),
                );
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
                return self.send_mkdir_error(
                    client_ctx,
                    VfsError::StorageError(format!("Failed to serialize inode: {}", e)),
                );
            }
        };

        self.start_storage_write(
            &inode_key(path),
            &inode_json,
            PendingOp::PutInode {
                ctx: Some(client_ctx.clone()),
                response_tag: vfs_msg::MSG_VFS_MKDIR_RESPONSE,
            },
        )
    }

    /// Handle mkdir inode result (checking if path already exists)
    ///
    /// DEPRECATED: Use CheckExistsForMkdir variant instead. This handler remains
    /// for backward compatibility with InodeOpType::MkdirCheckParent.
    pub fn handle_mkdir_inode_result(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        // Handle result type strictly
        match result_type {
            storage_result::EXISTS_OK => {
                let exists = !data.is_empty() && data[0] == 1;
                if exists {
                    return self.send_mkdir_error(client_ctx, VfsError::AlreadyExists);
                }
                // Proceed to create
            }
            storage_result::NOT_FOUND => {
                // Doesn't exist - proceed to create
            }
            storage_result::READ_OK => {
                // Path exists (we got data) - this shouldn't happen with exists check
                // but handle it gracefully
                return self.send_mkdir_error(client_ctx, VfsError::AlreadyExists);
            }
            _ => {
                // Unexpected result type - fail closed
                syscall::debug(&format!(
                    "VfsService: mkdir {} failed with unexpected result: {} ({})",
                    path,
                    result_type,
                    result_type_name(result_type)
                ));
                return self.send_mkdir_error(
                    client_ctx,
                    VfsError::StorageError(format!(
                        "Check exists failed: unexpected result type {} ({})",
                        result_type,
                        result_type_name(result_type)
                    )),
                );
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
                return self.send_mkdir_error(
                    client_ctx,
                    VfsError::StorageError(format!("Failed to serialize inode: {}", e)),
                );
            }
        };

        self.start_storage_write(
            &inode_key(path),
            &inode_json,
            PendingOp::PutInode {
                ctx: Some(client_ctx.clone()),
                response_tag: vfs_msg::MSG_VFS_MKDIR_RESPONSE,
            },
        )
    }

    /// Handle write file operation result (state machine)
    ///
    /// This handler implements the write file state machine:
    /// 1. CheckingParent: Verify parent exists, is directory, check permissions
    /// 2. WritingContent: Content write completed, now write inode
    /// 3. WritingInode: Inode write completed, send success response
    pub fn handle_write_file_op_result(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        perm_ctx: &PermissionContext,
        stage: WriteFileStage,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        match stage {
            WriteFileStage::CheckingParent { content } => {
                self.handle_write_checking_parent(client_ctx, path, perm_ctx, result_type, data, content)
            }
            WriteFileStage::WritingContent { content_len } => {
                self.handle_write_content_done(client_ctx, path, perm_ctx, content_len, result_type)
            }
            WriteFileStage::WritingInode => {
                self.handle_write_inode_done(client_ctx, path, result_type)
            }
        }
    }

    /// Stage 1: Check parent directory exists, is a directory, and we have permission
    fn handle_write_checking_parent(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        perm_ctx: &PermissionContext,
        result_type: u8,
        data: &[u8],
        content: Vec<u8>,
    ) -> Result<(), AppError> {
        // Handle result type strictly - only READ_OK is acceptable for parent check
        match result_type {
            storage_result::READ_OK => {
                // Good - parse and validate parent
            }
            storage_result::NOT_FOUND => {
                syscall::debug(&format!(
                    "VfsService: write {} failed - parent directory not found",
                    path
                ));
                // More specific error: parent doesn't exist
                return self.send_write_error(client_ctx, VfsError::NotFound);
            }
            _ => {
                // Unexpected result type - fail closed
                syscall::debug(&format!(
                    "VfsService: write {} parent check failed with unexpected result: {} ({})",
                    path,
                    result_type,
                    result_type_name(result_type)
                ));
                return self.send_write_error(
                    client_ctx,
                    VfsError::StorageError(format!(
                        "Parent read failed: unexpected result type {} ({})",
                        result_type,
                        result_type_name(result_type)
                    )),
                );
            }
        }

        // Parse parent inode - FAIL CLOSED on parse error
        let parent_inode = match serde_json::from_slice::<Inode>(data) {
            Ok(inode) => inode,
            Err(e) => {
                // SECURITY: Fail closed - corrupt/malicious parent blob could bypass permission check
                syscall::debug(&format!(
                    "VfsService: SECURITY: Failed to parse parent inode for {}: {} (denying write)",
                    path, e
                ));
                return self.send_write_error(
                    client_ctx,
                    VfsError::StorageError(format!(
                        "Parent inode corrupt or invalid: {}",
                        e
                    )),
                );
            }
        };

        // SECURITY: Verify parent is actually a directory
        if !parent_inode.is_directory() {
            syscall::debug(&format!(
                "VfsService: write {} failed - parent is not a directory (type: {:?})",
                path, parent_inode.inode_type
            ));
            return self.send_write_error(client_ctx, VfsError::NotADirectory);
        }

        // Check write permission on parent directory
        if !check_write(&parent_inode, perm_ctx) {
            syscall::debug(&format!(
                "VfsService: Permission denied for write {} (pid={})",
                path, client_ctx.pid
            ));
            return self.send_write_error(client_ctx, VfsError::PermissionDenied);
        }

        // Permission granted - write content FIRST
        // This ensures we never have an inode pointing to missing content
        let content_len = content.len() as u64;
        self.start_storage_write(
            &content_key(path),
            &content,
            PendingOp::WriteFileOp {
                ctx: client_ctx.clone(),
                path: path.to_string(),
                perm_ctx: perm_ctx.clone(),
                stage: WriteFileStage::WritingContent { content_len },
            },
        )
    }

    /// Stage 2: Content write completed - now write inode
    fn handle_write_content_done(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        perm_ctx: &PermissionContext,
        content_len: u64,
        result_type: u8,
    ) -> Result<(), AppError> {
        // Content write must succeed before we write inode
        if result_type != storage_result::WRITE_OK {
            syscall::debug(&format!(
                "VfsService: write {} content write failed: {} ({})",
                path,
                result_type,
                result_type_name(result_type)
            ));
            return self.send_write_error(
                client_ctx,
                VfsError::StorageError(format!(
                    "Content write failed: {} ({})",
                    result_type,
                    result_type_name(result_type)
                )),
            );
        }

        // Create the file inode
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let parent = parent_path(path);
        let now = syscall::get_wallclock();

        // Set owner_id based on permission context (user writes own their files)
        let owner_id = perm_ctx.user_id;

        let inode = Inode::new_file(
            path.to_string(),
            parent,
            name,
            owner_id,
            content_len,
            None, // TODO: compute content hash
            now,
        );

        let inode_json = match serde_json::to_vec(&inode) {
            Ok(j) => j,
            Err(e) => {
                // Content is now orphaned - this is acceptable (can be GC'd later)
                // Better than having an inode pointing to missing content
                syscall::debug(&format!(
                    "VfsService: write {} inode serialization failed after content write: {}",
                    path, e
                ));
                return self.send_write_error(
                    client_ctx,
                    VfsError::StorageError(format!("Failed to serialize inode: {}", e)),
                );
            }
        };

        // Write inode (stage 2)
        self.start_storage_write(
            &inode_key(path),
            &inode_json,
            PendingOp::WriteFileOp {
                ctx: client_ctx.clone(),
                path: path.to_string(),
                perm_ctx: perm_ctx.clone(),
                stage: WriteFileStage::WritingInode,
            },
        )
    }

    /// Stage 3: Inode write completed - send response
    fn handle_write_inode_done(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        result_type: u8,
    ) -> Result<(), AppError> {
        if result_type != storage_result::WRITE_OK {
            // Inode write failed - content is orphaned but that's acceptable
            syscall::debug(&format!(
                "VfsService: write {} inode write failed: {} ({}) - content is orphaned",
                path,
                result_type,
                result_type_name(result_type)
            ));
            return self.send_write_error(
                client_ctx,
                VfsError::StorageError(format!(
                    "Inode write failed: {} ({})",
                    result_type,
                    result_type_name(result_type)
                )),
            );
        }

        // Both content and inode written successfully
        syscall::debug(&format!("VfsService: write {} completed successfully", path));
        let response = WriteFileResponse { result: Ok(()) };
        self.send_response(client_ctx, vfs_msg::MSG_VFS_WRITE_RESPONSE, &response)
    }

    /// Handle write file inode result (checking parent exists and permissions)
    ///
    /// DEPRECATED: This is the old non-state-machine handler. Use WriteFileOp instead
    /// for new code. Kept for backward compatibility with InodeOpType::WriteFileCheckParent.
    pub fn handle_write_file_inode_result(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        perm_ctx: &PermissionContext,
        result_type: u8,
        data: &[u8],
        content: Vec<u8>,
    ) -> Result<(), AppError> {
        // Redirect to the new state machine implementation
        self.handle_write_checking_parent(client_ctx, path, perm_ctx, result_type, data, content)
    }

    /// Handle put inode result
    ///
    /// If `client_ctx` is `None`, this is an intermediate step and no response is sent.
    pub fn handle_put_inode_result(
        &mut self,
        client_ctx: Option<&ClientContext>,
        response_tag: u32,
        result_type: u8,
    ) -> Result<(), AppError> {
        // If no client context, this is an intermediate step - no response needed
        let client_ctx = match client_ctx {
            Some(ctx) => ctx,
            None => return Ok(()),
        };

        let success = result_type == storage_result::WRITE_OK;

        if response_tag == vfs_msg::MSG_VFS_MKDIR_RESPONSE {
            let response = MkdirResponse {
                result: if success {
                    Ok(())
                } else {
                    Err(VfsError::StorageError(format!(
                        "Mkdir inode write failed: {} ({})",
                        result_type,
                        result_type_name(result_type)
                    )))
                },
            };
            self.send_response(client_ctx, response_tag, &response)
        } else {
            // Generic success for other operations
            Ok(())
        }
    }

    /// Handle put content result
    ///
    /// DEPRECATED: This handler is for the old non-state-machine write path.
    /// New writes use WriteFileOp state machine which handles content results internally.
    pub fn handle_put_content_result(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        result_type: u8,
    ) -> Result<(), AppError> {
        if result_type != storage_result::WRITE_OK {
            syscall::debug(&format!(
                "VfsService: write {} content write failed: {} ({})",
                path,
                result_type,
                result_type_name(result_type)
            ));
            return self.send_write_error(
                client_ctx,
                VfsError::StorageError(format!(
                    "Content write failed: {} ({})",
                    result_type,
                    result_type_name(result_type)
                )),
            );
        }

        // Note: This old path doesn't verify inode write succeeded before responding
        // Use WriteFileOp for proper atomic-ish writes
        let response = WriteFileResponse { result: Ok(()) };
        self.send_response(client_ctx, vfs_msg::MSG_VFS_WRITE_RESPONSE, &response)
    }

    // =========================================================================
    // Mkdir State Machine
    // =========================================================================

    /// Handle mkdir operation result (state machine)
    ///
    /// This handler implements the mkdir state machine:
    /// 1. CheckingExists: Verify path doesn't already exist
    /// 2. CheckingParent: Verify parent exists, is directory, check permissions
    /// 3. WritingInode: Write the directory inode
    pub fn handle_mkdir_op_result(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        perm_ctx: &PermissionContext,
        stage: MkdirStage,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        match stage {
            MkdirStage::CheckingExists => {
                self.handle_mkdir_checking_exists(client_ctx, path, perm_ctx, result_type, data)
            }
            MkdirStage::CheckingParent => {
                self.handle_mkdir_checking_parent(client_ctx, path, perm_ctx, result_type, data)
            }
            MkdirStage::WritingInode => {
                self.handle_mkdir_writing_inode(client_ctx, path, result_type)
            }
        }
    }

    /// Stage 1: Check if path already exists
    fn handle_mkdir_checking_exists(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        perm_ctx: &PermissionContext,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        // Handle result type strictly
        match result_type {
            storage_result::EXISTS_OK => {
                let exists = !data.is_empty() && data[0] == 1;
                if exists {
                    return self.send_mkdir_error(client_ctx, VfsError::AlreadyExists);
                }
                // Path doesn't exist - proceed to check parent
            }
            storage_result::NOT_FOUND => {
                // Key not found = doesn't exist, proceed to check parent
            }
            _ => {
                // Unexpected result type - fail closed
                syscall::debug(&format!(
                    "VfsService: mkdir {} exists check failed with unexpected result: {} ({})",
                    path,
                    result_type,
                    result_type_name(result_type)
                ));
                return self.send_mkdir_error(
                    client_ctx,
                    VfsError::StorageError(format!(
                        "Exists check failed: unexpected result type {} ({})",
                        result_type,
                        result_type_name(result_type)
                    )),
                );
            }
        }

        // Get parent path and check it exists with proper permissions
        // Rule 4: Never skip permission check, even for root
        let parent = parent_path(path);

        self.start_storage_read(
            &inode_key(&parent),
            PendingOp::MkdirOp {
                ctx: client_ctx.clone(),
                path: path.to_string(),
                perm_ctx: perm_ctx.clone(),
                stage: MkdirStage::CheckingParent,
            },
        )
    }

    /// Stage 2: Check parent directory exists, is a directory, and we have permission
    fn handle_mkdir_checking_parent(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        perm_ctx: &PermissionContext,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        // Handle result type strictly - only READ_OK is acceptable for parent check
        match result_type {
            storage_result::READ_OK => {
                // Good - parse and validate parent
            }
            storage_result::NOT_FOUND => {
                syscall::debug(&format!(
                    "VfsService: mkdir {} failed - parent directory not found",
                    path
                ));
                return self.send_mkdir_error(client_ctx, VfsError::NotFound);
            }
            _ => {
                // Unexpected result type - fail closed
                syscall::debug(&format!(
                    "VfsService: mkdir {} parent check failed with unexpected result: {} ({})",
                    path,
                    result_type,
                    result_type_name(result_type)
                ));
                return self.send_mkdir_error(
                    client_ctx,
                    VfsError::StorageError(format!(
                        "Parent read failed: unexpected result type {} ({})",
                        result_type,
                        result_type_name(result_type)
                    )),
                );
            }
        }

        // Parse parent inode - FAIL CLOSED on parse error
        let parent_inode = match serde_json::from_slice::<Inode>(data) {
            Ok(inode) => inode,
            Err(e) => {
                // SECURITY: Fail closed - corrupt/malicious parent blob could bypass permission check
                syscall::debug(&format!(
                    "VfsService: SECURITY: Failed to parse parent inode for mkdir {}: {} (denying)",
                    path, e
                ));
                return self.send_mkdir_error(
                    client_ctx,
                    VfsError::StorageError(format!(
                        "Parent inode corrupt or invalid: {}",
                        e
                    )),
                );
            }
        };

        // SECURITY: Verify parent is actually a directory
        if !parent_inode.is_directory() {
            syscall::debug(&format!(
                "VfsService: mkdir {} failed - parent is not a directory (type: {:?})",
                path, parent_inode.inode_type
            ));
            return self.send_mkdir_error(client_ctx, VfsError::NotADirectory);
        }

        // Check write permission on parent directory
        if !check_write(&parent_inode, perm_ctx) {
            syscall::debug(&format!(
                "VfsService: Permission denied for mkdir {} (pid={})",
                path, client_ctx.pid
            ));
            return self.send_mkdir_error(client_ctx, VfsError::PermissionDenied);
        }

        // Permission granted - create the directory inode
        self.create_directory_inode(client_ctx, path, perm_ctx)
    }

    /// Helper: Create and write a directory inode
    fn create_directory_inode(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        perm_ctx: &PermissionContext,
    ) -> Result<(), AppError> {
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        let parent = parent_path(path);
        let now = syscall::get_wallclock();

        // Set owner_id based on permission context
        let owner_id = perm_ctx.user_id;

        let inode = Inode::new_directory(path.to_string(), parent, name, owner_id, now);

        let inode_json = match serde_json::to_vec(&inode) {
            Ok(j) => j,
            Err(e) => {
                return self.send_mkdir_error(
                    client_ctx,
                    VfsError::StorageError(format!("Failed to serialize inode: {}", e)),
                );
            }
        };

        self.start_storage_write(
            &inode_key(path),
            &inode_json,
            PendingOp::MkdirOp {
                ctx: client_ctx.clone(),
                path: path.to_string(),
                perm_ctx: perm_ctx.clone(),
                stage: MkdirStage::WritingInode,
            },
        )
    }

    /// Stage 3: Inode write completed - send response
    fn handle_mkdir_writing_inode(
        &mut self,
        client_ctx: &ClientContext,
        path: &str,
        result_type: u8,
    ) -> Result<(), AppError> {
        if result_type != storage_result::WRITE_OK {
            syscall::debug(&format!(
                "VfsService: mkdir {} inode write failed: {} ({})",
                path,
                result_type,
                result_type_name(result_type)
            ));
            return self.send_mkdir_error(
                client_ctx,
                VfsError::StorageError(format!(
                    "Inode write failed: {} ({})",
                    result_type,
                    result_type_name(result_type)
                )),
            );
        }

        syscall::debug(&format!("VfsService: mkdir {} completed successfully", path));
        let response = MkdirResponse { result: Ok(()) };
        self.send_response(client_ctx, vfs_msg::MSG_VFS_MKDIR_RESPONSE, &response)
    }
}
