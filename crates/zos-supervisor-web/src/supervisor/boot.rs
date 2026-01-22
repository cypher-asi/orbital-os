//! Supervisor boot and initialization
//!
//! Handles kernel boot sequence and supervisor process initialization.

use zos_kernel::ProcessId;
use wasm_bindgen::prelude::*;

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
    /// The supervisor is registered in the process table for auditing purposes,
    /// but does NOT have endpoints or capabilities. It uses privileged kernel
    /// APIs instead of IPC for console I/O.
    pub(crate) fn initialize_supervisor_process(&mut self) {
        if self.supervisor_initialized {
            log("[supervisor] Already initialized");
            return;
        }

        // Register supervisor as PID 0 in the kernel (for auditing)
        self.supervisor_pid = self
            .kernel
            .register_process_with_pid(ProcessId(0), "supervisor");
        log(&format!(
            "[supervisor] Registered supervisor process as PID {}",
            self.supervisor_pid.0
        ));

        // Note: Supervisor does NOT create endpoints - it uses privileged kernel APIs:
        // - drain_console_output(): Get console output from processes
        // - deliver_console_input(): Send keyboard input to terminal

        self.supervisor_initialized = true;
        log("[supervisor] Supervisor initialized - uses privileged kernel APIs (no endpoints)");
    }

    /// Initialize VFS IndexedDB storage.
    ///
    /// This must be called before using VFS operations. It initializes the
    /// `zos-userspace` IndexedDB database and creates the root filesystem
    /// structure if it doesn't exist.
    ///
    /// Returns a JsValue indicating success (true) or an error message.
    #[wasm_bindgen]
    pub async fn init_vfs_storage(&mut self) -> Result<JsValue, JsValue> {
        log("[supervisor] Initializing VFS storage...");

        // Initialize the IndexedDB database
        let result = vfs::init().await;
        if result.is_falsy() {
            return Err(JsValue::from_str("Failed to initialize VFS storage"));
        }

        // Check if root exists
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
                ("/users", "/", "users"),
                ("/tmp", "/", "tmp"),
                ("/home", "/", "home"),
            ];

            for (path, parent, name) in dirs {
                let inode = vfs::create_dir_inode(path, parent, name);
                vfs::putInode(path, inode).await;
            }

            log("[supervisor] Root filesystem created");
        } else {
            log("[supervisor] VFS storage already initialized");
        }

        // Get inode count for logging
        let count = vfs::getInodeCount().await;
        if let Some(n) = count.as_f64() {
            log(&format!("[supervisor] VFS ready with {} inodes", n as u64));
        }

        Ok(JsValue::from_bool(true))
    }

    /// Clear VFS storage (for testing/reset).
    #[wasm_bindgen]
    pub async fn clear_vfs_storage(&mut self) -> Result<JsValue, JsValue> {
        log("[supervisor] Clearing VFS storage...");
        vfs::clear().await;
        log("[supervisor] VFS storage cleared");
        Ok(JsValue::from_bool(true))
    }
}
