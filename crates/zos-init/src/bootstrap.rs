//! Boot sequence for Init process
//!
//! Handles the initial spawning of core system services.
//!
//! # Platform Behavior
//!
//! The boot sequence uses the pure microkernel spawn model:
//!
//! - **QEMU**: Init loads binaries via `SYS_LOAD_BINARY` syscall and spawns them
//!   directly via `SYS_SPAWN_PROCESS` syscall. The HAL returns embedded binaries.
//!
//! - **WASM**: `SYS_LOAD_BINARY` returns `NOT_SUPPORTED` (-3). Init falls back to
//!   sending debug messages (`INIT:SPAWN:{name}`) which the Supervisor intercepts
//!   to handle async binary fetching and spawn.

#[cfg(target_arch = "wasm32")]
use alloc::format;
#[cfg(not(target_arch = "wasm32"))]
use std::format;

use crate::Init;
use zos_process as syscall;
use zos_process::syscall_error;

impl Init {
    /// Boot sequence - spawn PermissionService, VfsService, KeystoreService, IdentityService, and TimeService
    pub fn boot_sequence(&mut self) {
        self.log("Starting boot sequence (pure microkernel)...");

        // 1. Spawn PermissionService (PID 2) - the capability authority
        self.log("Spawning PermissionService (PID 2)...");
        self.spawn_service("permission_service");

        // 2. Spawn VfsService (PID 3) - virtual filesystem service
        // NOTE: VFS must be spawned before IdentityService since identity needs VFS
        self.log("Spawning VfsService (PID 3)...");
        self.spawn_service("vfs_service");

        // 3. Spawn KeystoreService (PID 4) - secure key storage
        // NOTE: Keystore must be spawned before IdentityService since identity uses keystore
        // for all /keys/ path operations (Invariant 32)
        self.log("Spawning KeystoreService (PID 4)...");
        self.spawn_service("keystore_service");

        // 4. Spawn IdentityService (PID 5) - user identity and key management
        self.log("Spawning IdentityService (PID 5)...");
        self.spawn_service("identity_service");

        // 5. Spawn TimeService (PID 6) - time settings management
        self.log("Spawning TimeService (PID 6)...");
        self.spawn_service("time_service");

        // NOTE: Terminal is no longer auto-spawned here.
        // Each terminal window is spawned by the Desktop component via launchTerminal(),
        // which creates a process and links it to a window for proper lifecycle management.
        // This enables process isolation (each window has its own terminal process).

        self.boot_complete = true;
        self.log("Boot sequence complete");
        self.log("  PermissionService: handles capability requests");
        self.log("  VfsService: handles filesystem operations");
        self.log("  KeystoreService: handles secure key storage");
        self.log("  IdentityService: handles identity and key management");
        self.log("  TimeService: handles time settings");
        self.log("  Terminal: spawned per-window by Desktop");
        self.log("Init entering minimal idle state");
    }

    /// Spawn a service using the pure microkernel approach.
    ///
    /// This method tries the pure microkernel path first (QEMU) and falls back
    /// to the Supervisor async flow (WASM) if binary loading is not supported.
    fn spawn_service(&mut self, name: &str) {
        // Try pure microkernel approach first (works on QEMU)
        match syscall::load_binary(name) {
            Ok(binary) => {
                // QEMU path: Got binary, spawn directly via syscall
                self.log(&format!("Loaded {} ({} bytes)", name, binary.len()));
                
                match syscall::spawn_process(name, &binary) {
                    Ok(pid) => {
                        self.log(&format!("Spawned {} as PID {}", name, pid));
                        
                        // Setup endpoint and capability for the new process
                        if let Ok((endpoint_id, slot)) = syscall::create_endpoint_for(pid) {
                            self.service_cap_slots.insert(pid, slot);
                            self.log(&format!(
                                "Created endpoint {} for {} (cap slot {})",
                                endpoint_id, name, slot
                            ));
                        }
                    }
                    Err(e) => {
                        self.log(&format!("Failed to spawn {}: error {}", name, e));
                    }
                }
            }
            Err(e) if e == syscall_error::NOT_SUPPORTED => {
                // WASM path: Binary loading not supported on this platform
                // Fall back to Supervisor async flow via debug message
                // This maintains backward compatibility with browser-based WASM mode
                self.log(&format!("Platform uses async spawn for {}", name));
                syscall::debug(&format!("INIT:SPAWN:{}", name));
            }
            Err(e) => {
                // Unexpected error (e.g., NOT_FOUND on QEMU means missing binary)
                self.log(&format!("Failed to load {}: error {}", name, e));
            }
        }
    }
}
