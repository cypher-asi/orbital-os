//! VFS Service (PID 4)
//!
//! The VFS Service manages filesystem operations for Zero OS. It:
//! - Handles MSG_VFS_* IPC messages from processes
//! - Performs storage operations via async syscalls (routed through supervisor to IndexedDB)
//! - Responds with MSG_VFS_*_RESPONSE messages
//! - Enforces permission checks based on caller context
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
//! │   IndexedDB     │  ◄── Browser storage (zos-storage DB)
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
//!
//! # Note on Key Storage
//!
//! Cryptographic key storage (paths under `/keys/`) is handled by the
//! KeystoreService (PID 7), not VFS. This provides security isolation
//! by storing keys in a separate zos-keystore IndexedDB database.
//!
//! # Permission Model
//!
//! The VFS service enforces permissions based on caller context:
//! - System processes (PID 1-9) have full access
//! - User applications check owner/world permissions on inodes
//! - User ID is extracted from path (e.g., `/users/{user_id}/...`)

extern crate alloc;

pub mod handlers;

#[cfg(test)]
mod tests;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::manifests::VFS_SERVICE_MANIFEST;
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};
use zos_process::MSG_STORAGE_RESULT;
use zos_vfs::ipc::vfs_msg;
use zos_vfs::service::{PermissionContext, ProcessClass};

// =============================================================================
// Resource Limits (Rule 11)
// =============================================================================

/// Maximum number of pending storage operations.
///
/// This prevents resource exhaustion from unbounded pending_ops map growth.
/// If exceeded, new operations return ResourceExhausted error.
pub const MAX_PENDING_OPS: usize = 1024;

/// Maximum content size for file writes (16 MB).
///
/// This prevents resource exhaustion from very large write requests.
/// If exceeded, write operations return ContentTooLarge error.
pub const MAX_CONTENT_SIZE: usize = 16 * 1024 * 1024;

// =============================================================================
// Storage Key Helpers
// =============================================================================

/// Build a storage key for an inode.
#[inline]
pub fn inode_key(path: &str) -> String {
    format!("inode:{}", path)
}

/// Build a storage key for file content.
#[inline]
pub fn content_key(path: &str) -> String {
    format!("content:{}", path)
}

/// Format a storage result type as a human-readable string.
pub fn result_type_name(result_type: u8) -> &'static str {
    use zos_process::storage_result;
    match result_type {
        storage_result::READ_OK => "READ_OK",
        storage_result::WRITE_OK => "WRITE_OK",
        storage_result::NOT_FOUND => "NOT_FOUND",
        storage_result::ERROR => "ERROR",
        storage_result::LIST_OK => "LIST_OK",
        storage_result::EXISTS_OK => "EXISTS_OK",
        _ => "UNKNOWN",
    }
}

// =============================================================================
// Pending Storage Operations
// =============================================================================

/// Common client context for pending operations.
///
/// Captures information needed to send responses:
/// - `pid`: The client process ID
/// - `reply_caps`: Capability slots for direct IPC reply (transferred from request)
#[derive(Clone, Debug)]
pub struct ClientContext {
    /// Client process ID
    pub pid: u32,
    /// Reply capability slots (for direct IPC response)
    pub reply_caps: Vec<u32>,
}

impl ClientContext {
    /// Create a new client context from a message.
    pub fn from_message(msg: &Message) -> Self {
        Self {
            pid: msg.from_pid,
            reply_caps: msg.cap_slots.clone(),
        }
    }
}

/// Tracks pending storage operations awaiting results
#[derive(Clone)]
pub enum PendingOp {
    /// Get inode for stat/exists/directory operations
    GetInode {
        ctx: ClientContext,
        path: String,
        op_type: InodeOpType,
        /// Permission context for access control
        perm_ctx: PermissionContext,
    },
    /// Get file content for read
    GetContent {
        ctx: ClientContext,
        path: String,
        /// Permission context for access control
        perm_ctx: PermissionContext,
    },
    /// Put inode (after put, send response if ctx is Some)
    ///
    /// When `ctx` is `None`, this is an intermediate step in a multi-step operation
    /// and no response should be sent (e.g., inode write before content write).
    PutInode {
        ctx: Option<ClientContext>,
        response_tag: u32,
    },
    /// Put content (after put, send response)
    PutContent {
        ctx: ClientContext,
        path: String,
    },
    /// Delete inode (after delete, send response if ctx is Some)
    ///
    /// When `ctx` is `None`, this is an intermediate step in a multi-step operation.
    DeleteInode {
        ctx: Option<ClientContext>,
        response_tag: u32,
    },
    /// Delete content (intermediate step, no response sent)
    DeleteContent {
        #[allow(dead_code)]
        path: String,
    },
    /// List children for readdir
    ListChildren {
        ctx: ClientContext,
        #[allow(dead_code)]
        path: String,
        /// Permission context for access control
        perm_ctx: PermissionContext,
    },
    /// Check exists
    ExistsCheck {
        ctx: ClientContext,
        #[allow(dead_code)]
        path: String,
    },
    /// Check if path exists for mkdir (dedicated variant for clarity)
    ///
    /// This uses storage_exists_async and expects EXISTS_OK result type.
    CheckExistsForMkdir {
        ctx: ClientContext,
        path: String,
        create_parents: bool,
        perm_ctx: PermissionContext,
    },
    /// Write file operation - tracks the state machine for atomic-ish writes
    ///
    /// Stages:
    /// 1. Check parent exists and is directory, check permissions
    /// 2. Write content first
    /// 3. Write inode (only after content succeeds)
    /// 4. Send response (only after inode succeeds)
    WriteFileOp {
        ctx: ClientContext,
        path: String,
        perm_ctx: PermissionContext,
        stage: WriteFileStage,
    },
    /// Mkdir operation - tracks the state machine for directory creation
    ///
    /// Stages (create_parents=false):
    /// 1. Check if path already exists
    /// 2. Check parent exists and is directory, check write permission
    /// 3. Write inode
    ///
    /// Stages (create_parents=true):
    /// 1. Check if target path already exists (if yes, success)
    /// 2. CreatingParents stage - iterate through each parent path
    /// 3. Write inode for target
    MkdirOp {
        ctx: ClientContext,
        path: String,
        perm_ctx: PermissionContext,
        stage: MkdirStage,
        /// Whether to create parent directories if they don't exist
        create_parents: bool,
    },
    /// Readdir operation - tracks the state machine for directory listing
    ///
    /// Stages:
    /// 1. Read directory inode
    /// 2. Check read permission
    /// 3. List children
    ReaddirOp {
        ctx: ClientContext,
        path: String,
        perm_ctx: PermissionContext,
        stage: ReaddirStage,
    },
    /// Unlink operation - tracks the state machine for file deletion
    ///
    /// Stages:
    /// 1. Read inode to verify it's a file and check permissions
    /// 2. Delete content (must complete first)
    /// 3. Delete inode (only after content delete succeeds)
    UnlinkOp {
        ctx: ClientContext,
        path: String,
        perm_ctx: PermissionContext,
        stage: UnlinkStage,
    },
}

/// Stages for the WriteFile operation state machine.
///
/// This ensures we respond success only after both content and inode are committed.
#[derive(Clone)]
pub enum WriteFileStage {
    /// Checking parent directory exists and permissions
    CheckingParent {
        /// The content to write
        content: Vec<u8>,
    },
    /// Writing content (stage 1 of 2)
    WritingContent {
        /// Size for inode metadata
        content_len: u64,
    },
    /// Writing inode metadata (stage 2 of 2)
    WritingInode,
}

/// Stages for the Mkdir operation state machine.
///
/// This ensures parent permissions are checked before creating the directory.
#[derive(Clone)]
pub enum MkdirStage {
    /// Checking if path already exists
    CheckingExists,
    /// Checking parent directory exists and we have write permission
    CheckingParent,
    /// Writing inode
    WritingInode,
    /// Creating parent directories recursively (for create_parents=true)
    /// Contains list of paths to create and current index
    CreatingParents {
        /// All directory paths from root to target (e.g., ["/home", "/home/1", "/home/1/.zos"])
        paths: Vec<String>,
        /// Current index in paths we're processing
        index: usize,
        /// Whether we're currently writing an inode (vs checking existence)
        writing: bool,
    },
}

/// Stages for the Readdir operation state machine.
///
/// This ensures directory permissions are checked before listing.
#[derive(Clone)]
pub enum ReaddirStage {
    /// Reading directory inode to check permissions
    ReadingInode,
    /// Listing children
    ListingChildren,
}

/// Stages for the Unlink (file delete) operation state machine.
///
/// This ensures content is deleted before inode to avoid dangling references.
#[derive(Clone)]
pub enum UnlinkStage {
    /// Reading inode to verify it's a file and check permissions
    ReadingInode,
    /// Deleting content (must complete before inode delete)
    DeletingContent,
    /// Deleting inode (final step)
    DeletingInode,
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

// =============================================================================
// Path Validation
// =============================================================================

/// Validate a VFS path for correctness.
///
/// Returns `Ok(())` if the path is valid, or an error message if invalid.
///
/// # Validation Rules
/// - Path must start with '/'
/// - Path must not contain '..' traversal sequences
/// - Path must not contain '.' (current directory) components (must be normalized)
/// - Path must not contain null bytes
/// - Path must not be empty
/// - Path must not have trailing slashes (except "/" root)
/// - Path must not have empty components (double slashes like "//")
///
/// # Security Notes
///
/// This validation is critical for security. All paths should be validated
/// before use in storage operations to prevent:
/// - Path traversal attacks (../)
/// - Ambiguous path resolution (./foo vs foo)
/// - Key collision via trailing slashes (/a/b/ vs /a/b)
pub fn validate_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("Path cannot be empty");
    }
    if !path.starts_with('/') {
        return Err("Path must be absolute (start with '/')");
    }
    if path.contains('\0') {
        return Err("Path cannot contain null bytes");
    }

    // Root path is always valid
    if path == "/" {
        return Ok(());
    }

    // Check for trailing slash (except root)
    if path.ends_with('/') {
        return Err("Path cannot have trailing slash (except root)");
    }

    // Check for path traversal and invalid components
    for component in path.split('/') {
        if component == ".." {
            return Err("Path cannot contain '..' traversal");
        }
        if component == "." {
            return Err("Path cannot contain '.' (use normalized path)");
        }
        // Empty component means double slash (like "/a//b")
        // Skip leading empty component from split on "/"
    }

    // Check for double slashes (empty components between slashes)
    if path.contains("//") {
        return Err("Path cannot contain double slashes");
    }

    Ok(())
}

/// Build list of parent paths from root to the given path.
///
/// Example: "/home/1/.zos/identity" returns:
/// ["/home", "/home/1", "/home/1/.zos", "/home/1/.zos/identity"]
///
/// Returns empty vec for "/" root path.
pub fn build_parent_paths(path: &str) -> Vec<String> {
    if path == "/" || path.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut current = String::new();

    for component in path.split('/').filter(|s| !s.is_empty()) {
        current.push('/');
        current.push_str(component);
        result.push(current.clone());
    }

    result
}

// =============================================================================
// Permission Context Derivation
// =============================================================================

/// Derive PermissionContext from the calling process PID and target path.
///
/// # Permission Model
///
/// - **System processes** (PID 1-9): Full access (system class)
///   - User ID still extracted from path for ownership assignment
/// - **User applications** (PID >= 10): Check owner/world permissions
///   - User ID extracted from path: `/users/{user_id}/...` or `/home/{user_id}/...`
///   - If path doesn't contain user ID, treated as "other" (world permissions)
pub fn derive_permission_context(from_pid: u32, path: &str) -> PermissionContext {
    // Extract user ID from path if present (needed for ownership assignment)
    // Paths like /users/12345/... or /home/12345/... contain the user ID
    let user_id = extract_user_id_from_path(path);

    // System processes (init, vfs, identity, time services) have system class
    // but still use path-extracted user_id for setting file ownership
    if from_pid < 10 {
        return PermissionContext {
            user_id,
            process_class: ProcessClass::System,
        };
    }

    PermissionContext {
        user_id,
        process_class: ProcessClass::Application,
    }
}

/// Extract user ID from a VFS path.
///
/// Recognizes patterns:
/// - `/users/{user_id}/...`
/// - `/home/{user_id}/...`
/// - `/identity/{user_id}/...`
fn extract_user_id_from_path(path: &str) -> Option<u128> {
    let parts: Vec<&str> = path.split('/').collect();

    // Look for /users/{id}, /home/{id}, or /identity/{id}
    for (i, part) in parts.iter().enumerate() {
        if (*part == "users" || *part == "home" || *part == "identity") && i + 1 < parts.len() {
            if let Ok(id) = parts[i + 1].parse::<u128>() {
                return Some(id);
            }
        }
    }

    None
}

impl VfsService {
    // =========================================================================
    // Storage syscall helpers
    // =========================================================================

    /// Start async storage read and track the pending operation
    pub fn start_storage_read(&mut self, key: &str, pending_op: PendingOp) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "VfsService: Too many pending operations ({}), rejecting read",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

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
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "VfsService: Too many pending operations ({}), rejecting write",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

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
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "VfsService: Too many pending operations ({}), rejecting delete",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

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
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "VfsService: Too many pending operations ({}), rejecting list",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

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
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "VfsService: Too many pending operations ({}), rejecting exists check",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

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
                syscall::debug(&format!("VfsService: unknown request_id {}", request_id));
                return Ok(());
            }
        };

        // Dispatch based on operation type and result
        match pending_op {
            PendingOp::GetInode {
                ctx: client_ctx,
                path,
                op_type,
                perm_ctx,
            } => self.handle_inode_result(ctx, &client_ctx, &path, op_type, &perm_ctx, result_type, data),
            PendingOp::GetContent { ctx: client_ctx, path, perm_ctx: _ } => {
                // Permission already checked during inode fetch
                self.handle_content_result(&client_ctx, &path, result_type, data)
            }
            PendingOp::PutInode {
                ctx: client_ctx,
                response_tag,
            } => self.handle_put_inode_result(client_ctx.as_ref(), response_tag, result_type),
            PendingOp::PutContent { ctx: client_ctx, path } => {
                self.handle_put_content_result(&client_ctx, &path, result_type)
            }
            PendingOp::DeleteInode {
                ctx: client_ctx,
                response_tag,
            } => self.handle_delete_inode_result(client_ctx.as_ref(), response_tag, result_type),
            PendingOp::DeleteContent { path } => {
                self.handle_delete_content_result(&path, result_type)
            }
            PendingOp::ListChildren {
                ctx: client_ctx,
                path: _,
                perm_ctx: _,
            } => {
                // Permission check would require fetching directory inode first
                // For now, allow list (world-readable directories)
                self.handle_list_children_result(&client_ctx, result_type, data)
            }
            PendingOp::ExistsCheck {
                ctx: client_ctx,
                path: _,
            } => self.handle_exists_result(&client_ctx, result_type, data),
            PendingOp::CheckExistsForMkdir {
                ctx: client_ctx,
                path,
                create_parents,
                perm_ctx: _,
            } => self.handle_check_exists_for_mkdir_result(&client_ctx, &path, create_parents, result_type, data),
            PendingOp::WriteFileOp {
                ctx: client_ctx,
                path,
                perm_ctx,
                stage,
            } => self.handle_write_file_op_result(&client_ctx, &path, &perm_ctx, stage, result_type, data),
            PendingOp::MkdirOp {
                ctx: client_ctx,
                path,
                perm_ctx,
                stage,
                create_parents,
            } => self.handle_mkdir_op_result(&client_ctx, &path, &perm_ctx, stage, create_parents, result_type, data),
            PendingOp::ReaddirOp {
                ctx: client_ctx,
                path,
                perm_ctx,
                stage,
            } => self.handle_readdir_op_result(&client_ctx, &path, &perm_ctx, stage, result_type, data),
            PendingOp::UnlinkOp {
                ctx: client_ctx,
                path,
                perm_ctx,
                stage,
            } => self.handle_unlink_op_result(&client_ctx, &path, &perm_ctx, stage, result_type, data),
        }
    }

    /// Handle inode read result - dispatches to specific handlers
    fn handle_inode_result(
        &mut self,
        _ctx: &AppContext,
        client_ctx: &ClientContext,
        path: &str,
        op_type: InodeOpType,
        perm_ctx: &PermissionContext,
        result_type: u8,
        data: &[u8],
    ) -> Result<(), AppError> {
        match op_type {
            InodeOpType::Stat => self.handle_stat_inode_result(client_ctx, perm_ctx, result_type, data),
            InodeOpType::Exists => self.handle_exists_inode_result(client_ctx, result_type),
            InodeOpType::ReadFile => {
                self.handle_read_file_inode_result(client_ctx, path, perm_ctx, result_type, data)
            }
            InodeOpType::MkdirCheckParent { create_parents: _ } => {
                self.handle_mkdir_inode_result(client_ctx, path, result_type, data)
            }
            InodeOpType::WriteFileCheckParent { content } => {
                self.handle_write_file_inode_result(client_ctx, path, perm_ctx, result_type, data, content)
            }
            InodeOpType::Rmdir { recursive: _ } => {
                self.handle_rmdir_inode_result(client_ctx, path, perm_ctx, result_type, data)
            }
            InodeOpType::Unlink => {
                self.handle_unlink_inode_result(client_ctx, path, perm_ctx, result_type, data)
            }
            InodeOpType::Readdir => Ok(()), // readdir uses ListChildren
        }
    }

    // =========================================================================
    // Response helpers
    // =========================================================================

    /// Send response to client, trying direct IPC first, then debug channel fallback.
    ///
    /// # Direct IPC (Preferred)
    ///
    /// If the client provided reply capability slots in the request, we try to
    /// send the response directly via IPC. This is more efficient and doesn't
    /// require the supervisor to parse debug messages.
    ///
    /// # Debug Channel Fallback
    ///
    /// If direct IPC fails (no reply caps or send error), we fall back to the
    /// debug channel. The supervisor parses VFS:RESPONSE messages and routes
    /// them to the appropriate client.
    pub fn send_response<T: serde::Serialize>(
        &self,
        ctx: &ClientContext,
        tag: u32,
        response: &T,
    ) -> Result<(), AppError> {
        match serde_json::to_vec(response) {
            Ok(data) => {
                // Try direct IPC via reply capability first
                if let Some(&reply_slot) = ctx.reply_caps.first() {
                    syscall::debug(&format!(
                        "VfsService: Sending response via reply cap slot {} (tag 0x{:x})",
                        reply_slot, tag
                    ));
                    match syscall::send(reply_slot, tag, &data) {
                        Ok(()) => {
                            syscall::debug("VfsService: Response sent via reply cap");
                            return Ok(());
                        }
                        Err(e) => {
                            syscall::debug(&format!(
                                "VfsService: Reply cap send failed ({}), falling back to debug channel",
                                e
                            ));
                        }
                    }
                }

                // Fallback: send via debug channel for supervisor to route
                let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
                syscall::debug(&format!("VFS:RESPONSE:{}:{:08x}:{}", ctx.pid, tag, hex));
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("VfsService: Failed to serialize response: {}", e));
                Err(AppError::IpcError(format!("Serialization failed: {}", e)))
            }
        }
    }

    /// Legacy: Send response via debug message only (no direct IPC).
    ///
    /// Used when we don't have a ClientContext (e.g., during intermediate operations).
    /// Prefer `send_response` when ClientContext is available.
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
                syscall::debug(&format!("VfsService: Unknown message tag 0x{:x}", msg.tag));
                Ok(())
            }
        }
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("VfsService: shutting down");
    }
}
