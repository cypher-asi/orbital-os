//! Keystore Service (PID 7)
//!
//! The KeystoreService provides secure storage for cryptographic keys,
//! isolated from general filesystem storage. It:
//! - Handles MSG_KEYSTORE_* IPC messages from processes
//! - Performs keystore operations via async syscalls (routed through supervisor to IndexedDB)
//! - Responds with MSG_KEYSTORE_*_RESPONSE messages
//!
//! # Safety Invariants (per zos-service.md Rule 0)
//!
//! ## Success Conditions
//! - A request succeeds only when ALL of:
//!   1. Request is valid JSON with required fields
//!   2. Key path is valid (starts with `/keys/`)
//!   3. Storage operation completes successfully
//!   4. Response is sent to the original caller
//!
//! ## Acceptable Partial Failure
//! - None (keystore operations are atomic key-value)
//!
//! ## Forbidden States
//! - Returning success before storage commit
//! - Silent fallthrough on parse errors (must return InvalidRequest)
//! - Unbounded pending operation growth (enforced via MAX_PENDING_OPS)
//!
//! # Architecture
//!
//! Keystore operations are event-driven using push-based async storage:
//!
//! ```text
//! Identity Service (PID 5)
//!        │
//!        │ IPC (MSG_KEYSTORE_READ)
//!        ▼
//! ┌─────────────────┐
//! │ KeystoreService │  ◄── This service (PID 7)
//! │   (Process)     │
//! └────────┬────────┘
//!          │
//!          │ SYS_KEYSTORE_READ syscall (returns request_id immediately)
//!          ▼
//! ┌─────────────────┐
//! │  Kernel/Axiom   │
//! └────────┬────────┘
//!          │
//!          │ HAL async keystore
//!          ▼
//! ┌─────────────────┐
//! │   Supervisor    │  ◄── Main thread
//! └────────┬────────┘
//!          │
//!          │ ZosKeystore.read()
//!          ▼
//! ┌─────────────────┐
//! │   IndexedDB     │  ◄── zos-keystore database
//! └────────┬────────┘
//!          │
//!          │ Promise resolves
//!          ▼
//! ┌─────────────────┐
//! │   Supervisor    │  ◄── notify_keystore_read_complete()
//! └────────┬────────┘
//!          │
//!          │ IPC (MSG_KEYSTORE_RESULT)
//!          ▼
//! ┌─────────────────┐
//! │ KeystoreService │  ◄── Matches request_id, sends response to client
//! └─────────────────┘
//! ```
//!
//! # Protocol
//!
//! Processes communicate with KeystoreService via IPC:
//!
//! - `MSG_KEYSTORE_READ (0xA000)`: Read key data
//! - `MSG_KEYSTORE_WRITE (0xA002)`: Write key data
//! - `MSG_KEYSTORE_DELETE (0xA004)`: Delete key
//! - `MSG_KEYSTORE_EXISTS (0xA006)`: Check if key exists
//! - `MSG_KEYSTORE_LIST (0xA008)`: List keys with prefix

extern crate alloc;

pub mod handlers;
pub mod types;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::manifests::KEYSTORE_MANIFEST;
use zos_apps::syscall;
use zos_apps::{AppContext, AppError, AppManifest, ControlFlow, Message, ZeroApp};
use zos_process::keystore_result;
use zos_process::MSG_KEYSTORE_RESULT;
use zos_ipc::keystore_svc;

use types::KeystoreError;

// =============================================================================
// Resource Limits (Rule 11)
// =============================================================================

/// Maximum number of pending keystore operations.
///
/// This prevents resource exhaustion from unbounded pending_ops map growth.
/// If exceeded, new operations return ResourceExhausted error.
pub const MAX_PENDING_OPS: usize = 256;

/// Maximum content size for key writes (1 MB).
///
/// This prevents resource exhaustion from very large write requests.
/// Keys are typically small cryptographic material.
pub const MAX_CONTENT_SIZE: usize = 1024 * 1024;

// =============================================================================
// Pending Keystore Operations
// =============================================================================

/// Common client context for pending operations.
///
/// Captures information needed to send responses:
/// - `pid`: The client process ID
/// - `reply_caps`: Capability slots for direct IPC reply
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

/// Tracks pending keystore operations awaiting results.
#[derive(Clone)]
pub enum PendingOp {
    /// Read operation
    Read {
        ctx: ClientContext,
        key: String,
    },
    /// Write operation
    Write {
        ctx: ClientContext,
        key: String,
    },
    /// Delete operation
    Delete {
        ctx: ClientContext,
        key: String,
    },
    /// Exists check operation
    Exists {
        ctx: ClientContext,
        key: String,
    },
    /// List keys operation
    List {
        ctx: ClientContext,
        prefix: String,
    },
}

// =============================================================================
// KeystoreService Application
// =============================================================================

/// Keystore Service - manages cryptographic key storage
#[derive(Default)]
pub struct KeystoreService {
    /// Whether we have registered with init
    registered: bool,
    /// Pending keystore operations: request_id -> operation context
    pending_ops: BTreeMap<u32, PendingOp>,
}

// =============================================================================
// Key Validation
// =============================================================================

/// Validate a keystore key path.
///
/// Returns `Ok(())` if the key is valid, or an error if invalid.
///
/// # Validation Rules
/// - Key must start with `/keys/`
/// - Key must not be empty
/// - Key must not contain null bytes
pub fn validate_key(key: &str) -> Result<(), KeystoreError> {
    if key.is_empty() {
        return Err(KeystoreError::InvalidKey("Key cannot be empty".into()));
    }
    if !key.starts_with("/keys/") {
        return Err(KeystoreError::InvalidKey(
            "Key must start with /keys/".into(),
        ));
    }
    if key.contains('\0') {
        return Err(KeystoreError::InvalidKey(
            "Key cannot contain null bytes".into(),
        ));
    }
    Ok(())
}

/// Format a keystore result type as a human-readable string.
pub fn result_type_name(result_type: u8) -> &'static str {
    match result_type {
        keystore_result::READ_OK => "READ_OK",
        keystore_result::WRITE_OK => "WRITE_OK",
        keystore_result::NOT_FOUND => "NOT_FOUND",
        keystore_result::ERROR => "ERROR",
        keystore_result::LIST_OK => "LIST_OK",
        keystore_result::EXISTS_OK => "EXISTS_OK",
        _ => "UNKNOWN",
    }
}

impl KeystoreService {
    // =========================================================================
    // Keystore syscall helpers
    // =========================================================================

    /// Start async keystore read and track the pending operation
    pub fn start_keystore_read(
        &mut self,
        key: &str,
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "KeystoreService: Too many pending operations ({}), rejecting read",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        match syscall::keystore_read_async(key) {
            Ok(request_id) => {
                let request_id = request_id as u32;
                syscall::debug(&format!(
                    "KeystoreService: keystore_read_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("KeystoreService: keystore_read_async failed: {}", e));
                Err(AppError::IpcError(format!("Keystore read failed: {}", e)))
            }
        }
    }

    /// Start async keystore write and track the pending operation
    pub fn start_keystore_write(
        &mut self,
        key: &str,
        value: &[u8],
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "KeystoreService: Too many pending operations ({}), rejecting write",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        match syscall::keystore_write_async(key, value) {
            Ok(request_id) => {
                let request_id = request_id as u32;
                syscall::debug(&format!(
                    "KeystoreService: keystore_write_async({}, {} bytes) -> request_id={}",
                    key,
                    value.len(),
                    request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("KeystoreService: keystore_write_async failed: {}", e));
                Err(AppError::IpcError(format!("Keystore write failed: {}", e)))
            }
        }
    }

    /// Start async keystore delete and track the pending operation
    pub fn start_keystore_delete(
        &mut self,
        key: &str,
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "KeystoreService: Too many pending operations ({}), rejecting delete",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        match syscall::keystore_delete_async(key) {
            Ok(request_id) => {
                let request_id = request_id as u32;
                syscall::debug(&format!(
                    "KeystoreService: keystore_delete_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!(
                    "KeystoreService: keystore_delete_async failed: {}",
                    e
                ));
                Err(AppError::IpcError(format!("Keystore delete failed: {}", e)))
            }
        }
    }

    /// Start async keystore exists check and track the pending operation
    pub fn start_keystore_exists(
        &mut self,
        key: &str,
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "KeystoreService: Too many pending operations ({}), rejecting exists",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        match syscall::keystore_exists_async(key) {
            Ok(request_id) => {
                let request_id = request_id as u32;
                syscall::debug(&format!(
                    "KeystoreService: keystore_exists_async({}) -> request_id={}",
                    key, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!(
                    "KeystoreService: keystore_exists_async failed: {}",
                    e
                ));
                Err(AppError::IpcError(format!("Keystore exists failed: {}", e)))
            }
        }
    }

    /// Start async keystore list and track the pending operation
    pub fn start_keystore_list(
        &mut self,
        prefix: &str,
        pending_op: PendingOp,
    ) -> Result<(), AppError> {
        // Rule 11: Check resource limit before starting new operation
        if self.pending_ops.len() >= MAX_PENDING_OPS {
            syscall::debug(&format!(
                "KeystoreService: Too many pending operations ({}), rejecting list",
                self.pending_ops.len()
            ));
            return Err(AppError::IpcError("Too many pending operations".into()));
        }

        match syscall::keystore_list_async(prefix) {
            Ok(request_id) => {
                let request_id = request_id as u32;
                syscall::debug(&format!(
                    "KeystoreService: keystore_list_async({}) -> request_id={}",
                    prefix, request_id
                ));
                self.pending_ops.insert(request_id, pending_op);
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!("KeystoreService: keystore_list_async failed: {}", e));
                Err(AppError::IpcError(format!("Keystore list failed: {}", e)))
            }
        }
    }

    // =========================================================================
    // Keystore result handler (main dispatcher)
    // =========================================================================

    /// Handle MSG_KEYSTORE_RESULT - async keystore operation completed
    fn handle_keystore_result(&mut self, _ctx: &AppContext, msg: &Message) -> Result<(), AppError> {
        // Parse keystore result
        // Format: [request_id: u32, result_type: u8, data_len: u32, data: [u8]]
        if msg.data.len() < 9 {
            syscall::debug("KeystoreService: keystore result too short");
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
            "KeystoreService: keystore result request_id={}, type={} ({}), data_len={}",
            request_id,
            result_type,
            result_type_name(result_type),
            data_len
        ));

        // Look up pending operation
        let pending_op = match self.pending_ops.remove(&request_id) {
            Some(op) => op,
            None => {
                syscall::debug(&format!(
                    "KeystoreService: unknown request_id {}",
                    request_id
                ));
                return Ok(());
            }
        };

        // Dispatch based on operation type
        match pending_op {
            PendingOp::Read { ctx, key } => {
                self.handle_read_result(&ctx, &key, result_type, data)
            }
            PendingOp::Write { ctx, key } => {
                self.handle_write_result(&ctx, &key, result_type)
            }
            PendingOp::Delete { ctx, key } => {
                self.handle_delete_result(&ctx, &key, result_type)
            }
            PendingOp::Exists { ctx, key } => {
                self.handle_exists_result(&ctx, &key, result_type, data)
            }
            PendingOp::List { ctx, prefix } => {
                self.handle_list_result(&ctx, &prefix, result_type, data)
            }
        }
    }

    // =========================================================================
    // Response helpers
    // =========================================================================

    /// Send response to client via direct IPC or debug channel fallback.
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
                        "KeystoreService: Sending response via reply cap slot {} (tag 0x{:x})",
                        reply_slot, tag
                    ));
                    match syscall::send(reply_slot, tag, &data) {
                        Ok(()) => {
                            syscall::debug("KeystoreService: Response sent via reply cap");
                            return Ok(());
                        }
                        Err(e) => {
                            syscall::debug(&format!(
                                "KeystoreService: Reply cap send failed ({}), falling back to debug channel",
                                e
                            ));
                        }
                    }
                }

                // Fallback: send via debug channel for supervisor to route
                let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
                syscall::debug(&format!("KEYSTORE:RESPONSE:{}:{:08x}:{}", ctx.pid, tag, hex));
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!(
                    "KeystoreService: Failed to serialize response: {}",
                    e
                ));
                Err(AppError::IpcError(format!("Serialization failed: {}", e)))
            }
        }
    }

    /// Send response via debug message only.
    pub fn send_response_via_debug<T: serde::Serialize>(
        &self,
        to_pid: u32,
        tag: u32,
        response: &T,
    ) -> Result<(), AppError> {
        match serde_json::to_vec(response) {
            Ok(data) => {
                let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
                syscall::debug(&format!("KEYSTORE:RESPONSE:{}:{:08x}:{}", to_pid, tag, hex));
                Ok(())
            }
            Err(e) => {
                syscall::debug(&format!(
                    "KeystoreService: Failed to serialize response: {}",
                    e
                ));
                Err(AppError::IpcError(format!("Serialization failed: {}", e)))
            }
        }
    }
}

impl ZeroApp for KeystoreService {
    fn manifest() -> &'static AppManifest {
        &KEYSTORE_MANIFEST
    }

    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError> {
        syscall::debug(&format!("KeystoreService starting (PID {})", ctx.pid));

        // Register with init as "keystore" service
        let service_name = "keystore";
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

        syscall::debug("KeystoreService: Registered with init");

        Ok(())
    }

    fn update(&mut self, _ctx: &AppContext) -> ControlFlow {
        ControlFlow::Yield
    }

    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError> {
        syscall::debug(&format!(
            "KeystoreService: Received message tag 0x{:x} from PID {}",
            msg.tag, msg.from_pid
        ));

        match msg.tag {
            MSG_KEYSTORE_RESULT => self.handle_keystore_result(ctx, &msg),
            keystore_svc::MSG_KEYSTORE_READ => self.handle_read(ctx, &msg),
            keystore_svc::MSG_KEYSTORE_WRITE => self.handle_write(ctx, &msg),
            keystore_svc::MSG_KEYSTORE_DELETE => self.handle_delete(ctx, &msg),
            keystore_svc::MSG_KEYSTORE_EXISTS => self.handle_exists(ctx, &msg),
            keystore_svc::MSG_KEYSTORE_LIST => self.handle_list(ctx, &msg),
            _ => {
                syscall::debug(&format!(
                    "KeystoreService: Unknown message tag 0x{:x}",
                    msg.tag
                ));
                Ok(())
            }
        }
    }

    fn shutdown(&mut self, _ctx: &AppContext) {
        syscall::debug("KeystoreService: shutting down");
    }
}
