//! Supervisor boot and initialization
//!
//! Handles kernel boot sequence and supervisor process initialization.
//!
//! # Bootstrap Exception
//!
//! Per the architectural invariants, all process lifecycle management should
//! flow through Init (PID 1) to ensure proper audit logging via SysLog.
//! However, Init itself cannot register itself - it needs to be created first.
//!
//! ## The Bootstrap Problem
//!
//! The Init-driven spawn protocol requires Init to be running to receive
//! spawn requests. But Init cannot spawn itself. This creates a bootstrap
//! problem that requires a special exception.
//!
//! ## The Bootstrap Exception
//!
//! The supervisor is **allowed to make direct kernel calls** for:
//!
//! 1. **Supervisor registration (PID 0)**: The supervisor registers itself
//!    as a kernel process to hold capabilities for IPC communication.
//!
//! 2. **Init creation (PID 1)**: The supervisor creates Init via direct
//!    kernel calls in `spawn_init()`.
//!
//! After Init is running, ALL other process creation should flow through
//! Init via the Init-driven spawn protocol (MSG_SUPERVISOR_SPAWN_PROCESS).
//!
//! ## Why This Is Acceptable
//!
//! 1. **One-time operation**: Bootstrap happens once at system start.
//!
//! 2. **Logged via Axiom**: Even direct kernel calls go through the Axiom
//!    gateway for commit logging.
//!
//! 3. **Trusted component**: The supervisor is a trusted system component,
//!    not untrusted userspace code.
//!
//! 4. **Necessary for bootstrapping**: There's no other way to start Init.
//!
//! ## Invariant Compliance
//!
//! This exception is documented and intentional. It does not violate the
//! spirit of the invariants because:
//!
//! - Invariant 9 (SysLog): Kernel methods still log commits to Axiom.
//! - Invariant 16 (Supervisor privilege): After bootstrap, supervisor uses
//!   capability-checked IPC for all operations.
//!
//! See `spawn.rs` for the Init-driven spawn protocol documentation.

use wasm_bindgen::prelude::*;
use zos_kernel::ProcessId;

use super::{log, Supervisor};
use crate::vfs;

#[wasm_bindgen]
impl Supervisor {
    /// Boot the kernel
    #[wasm_bindgen]
    pub fn boot(&mut self) {
        log("[supervisor] Booting Zero OS kernel...");

        self.write_console("Zero OS Kernel Bootstrap\n");
        self.write_console("===========================\n\n");

        // Initialize supervisor as a kernel process (PID 0)
        self.initialize_supervisor_process();

        log("[supervisor] Boot complete - call spawn_init() to start init process");
    }

    /// Spawn the init process (PID 1)
    /// Call this after boot() and after setting the spawn callback
    #[wasm_bindgen]
    pub fn spawn_init(&mut self) {
        if self.init_spawned {
            log("[supervisor] Init already spawned");
            return;
        }

        log("[supervisor] Requesting init spawn...");
        self.write_console("Starting init process...\n");
        self.request_spawn("init", "init");
    }

    /// Initialize the supervisor as a kernel process (PID 0).
    ///
    /// # Bootstrap Exception
    ///
    /// This is a **bootstrap exception** to the Init-driven spawn protocol.
    /// The supervisor registers itself via a direct kernel call because:
    ///
    /// 1. Init doesn't exist yet to handle spawn requests
    /// 2. The supervisor needs to exist in the process table to hold
    ///    capabilities for IPC communication
    ///
    /// This direct kernel call is acceptable because:
    /// - It's a one-time bootstrap operation
    /// - It's logged via Axiom commit logging
    /// - After Init starts, all other spawns go through Init
    ///
    /// # Capabilities
    ///
    /// The supervisor is registered in the process table and will receive
    /// capabilities to Init, PermissionManager, and terminal endpoints
    /// during their spawn process. It uses capability-checked IPC.
    pub(crate) fn initialize_supervisor_process(&mut self) {
        if self.supervisor_initialized {
            log("[supervisor] Already initialized");
            return;
        }

        // BOOTSTRAP EXCEPTION: Direct kernel call for supervisor registration.
        // This is the first of two allowed direct kernel calls during bootstrap.
        // See module-level documentation for justification.
        self.supervisor_pid = self
            .system
            .register_process_with_pid(ProcessId(0), "supervisor");
        log(&format!(
            "[supervisor] Registered supervisor process as PID {} (bootstrap exception)",
            self.supervisor_pid.0
        ));

        // Note: Supervisor capabilities are granted during process spawn:
        // - Init's endpoint capability granted when Init spawns
        // - PM's endpoint capability granted when PM spawns
        // - Terminal endpoint capabilities granted when terminals spawn

        self.supervisor_initialized = true;
        log("[supervisor] Supervisor initialized - capabilities granted during spawn");
    }

    /// Initialize ZosStorage (VFS IndexedDB storage).
    ///
    /// This must be called before using VFS operations. It initializes the
    /// `zos-userspace` IndexedDB database and creates the root filesystem
    /// structure if it doesn't exist.
    ///
    /// ## Bootstrap Storage Access Pattern
    ///
    /// This method uses the internal `vfs` module for async IndexedDB operations.
    /// This is a bootstrap exception - after Init starts, all storage access
    /// should flow through processes using syscalls routed via HAL.
    ///
    /// The HAL provides sync bootstrap_storage_* methods for cache reads, but
    /// async operations (init, writes) require the vfs module during bootstrap.
    ///
    /// Returns a JsValue indicating success (true) or an error message.
    #[wasm_bindgen]
    pub async fn init_vfs_storage(&mut self) -> Result<JsValue, JsValue> {
        log("[supervisor] Initializing ZosStorage...");

        // Initialize the IndexedDB database (async, populates caches)
        let result = vfs::init().await;
        if result.is_falsy() {
            return Err(JsValue::from_str("Failed to initialize ZosStorage"));
        }

        // Check if root exists (uses async vfs, caches now populated)
        let root = vfs::getInode("/").await;
        if root.is_null() || root.is_undefined() {
            log("[supervisor] Creating root filesystem structure...");

            // Create root directory
            let root_inode = vfs::create_root_inode();
            vfs::putInode("/", root_inode).await;

            // Create standard directories
            let dirs = [
                ("/system", "/", "system"),
                ("/system/config", "/system", "config"),
                ("/system/services", "/system", "services"),
                ("/system/settings", "/system", "settings"),
                ("/users", "/", "users"),
                ("/tmp", "/", "tmp"),
                ("/home", "/", "home"),
            ];

            for (path, parent, name) in dirs {
                let inode = vfs::create_dir_inode(path, parent, name);
                vfs::putInode(path, inode).await;
            }

            // Create default user home directory structure (user ID 1)
            // This is the default test/demo user: 00000000000000000000000000000001
            let user_id = 1u128;
            let user_home = format!("/home/{}", user_id);
            let user_zos = format!("{}/.zos", user_home);
            let user_identity = format!("{}/identity", user_zos);
            let user_machine = format!("{}/machine", user_identity);
            
            let default_user_dirs = vec![
                (user_home.clone(), "/home".to_string(), format!("{}", user_id)),
                (user_zos.clone(), user_home.clone(), ".zos".to_string()),
                (user_identity.clone(), user_zos.clone(), "identity".to_string()),
                (user_machine.clone(), user_identity.clone(), "machine".to_string()),
            ];

            for (path, parent, name) in default_user_dirs {
                let inode = vfs::create_dir_inode(&path, &parent, &name);
                vfs::putInode(&path, inode).await;
            }

            log("[supervisor] Root filesystem created with default user home");
        } else {
            log("[supervisor] ZosStorage already initialized");
        }

        // Get inode count for logging
        let count = vfs::getInodeCount().await;
        if let Some(n) = count.as_f64() {
            log(&format!(
                "[supervisor] ZosStorage ready with {} inodes",
                n as u64
            ));
        }

        Ok(JsValue::from_bool(true))
    }

    /// Clear ZosStorage (for testing/reset).
    ///
    /// ## Bootstrap Exception
    ///
    /// This uses the internal vfs module for async clear operation.
    #[wasm_bindgen]
    pub async fn clear_vfs_storage(&mut self) -> Result<JsValue, JsValue> {
        log("[supervisor] Clearing ZosStorage...");
        vfs::clear().await;
        log("[supervisor] ZosStorage cleared");
        Ok(JsValue::from_bool(true))
    }
}
