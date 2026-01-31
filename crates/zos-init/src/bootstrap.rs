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
        self.spawn_service("permission");

        // 2. Spawn VfsService (PID 3) - virtual filesystem service
        // NOTE: VFS must be spawned before IdentityService since identity needs VFS
        self.log("Spawning VfsService (PID 3)...");
        self.spawn_service("vfs");

        // 3. Spawn KeystoreService (PID 4) - secure key storage
        // NOTE: Keystore must be spawned before IdentityService since identity uses keystore
        // for all /keys/ path operations (Invariant 32)
        self.log("Spawning KeystoreService (PID 4)...");
        self.spawn_service("keystore");

        // 4. Spawn IdentityService (PID 5) - user identity and key management
        #[cfg(not(feature = "skip-identity"))]
        {
            self.log("Spawning IdentityService (PID 5)...");
            self.spawn_service("identity");
        }
        #[cfg(feature = "skip-identity")]
        self.log("IdentityService skipped (QEMU mode)");

        // 5. Spawn TimeService (PID 6) - time settings management
        self.log("Spawning TimeService (PID 6)...");
        self.spawn_service("time");

        // 6. Spawn Terminal (PID 7) - interactive terminal for QEMU mode only
        // In QEMU mode, we need a terminal process running to receive serial input.
        // In browser WASM mode, terminals are spawned per-window by Desktop.
        // We detect QEMU mode at runtime by checking if load_binary succeeds.
        self.try_spawn_qemu_terminal();

        self.boot_complete = true;
        self.log("Boot sequence complete");
        self.log("  PermissionService: handles capability requests");
        self.log("  VfsService: handles filesystem operations");
        self.log("  KeystoreService: handles secure key storage");
        #[cfg(not(feature = "skip-identity"))]
        self.log("  IdentityService: handles identity and key management");
        self.log("  TimeService: handles time settings");
        self.log("Init entering minimal idle state");
    }

    /// Try to spawn terminal for QEMU mode only.
    ///
    /// This uses runtime detection: if `load_binary("terminal")` succeeds, we're in
    /// QEMU mode and spawn the terminal. If it returns NOT_SUPPORTED, we're in browser
    /// mode where terminals are spawned per-window by Desktop, so we skip.
    fn try_spawn_qemu_terminal(&mut self) {
        // Try to load terminal binary - this only succeeds in QEMU mode
        match syscall::load_binary("terminal") {
            Ok(binary) => {
                // QEMU mode: spawn terminal for interactive serial console
                self.log(&format!("Spawning Terminal (PID 7) for QEMU console..."));
                self.log(&format!("Loaded terminal ({} bytes)", binary.len()));

                match syscall::spawn_process("terminal", &binary) {
                    Ok(pid) => {
                        self.log(&format!("Spawned terminal as PID {}", pid));

                        // Setup endpoint and capability for terminal
                        if let Ok((endpoint_id, slot)) = syscall::create_endpoint_for(pid) {
                            self.log(&format!(
                                "DEBUG: create_endpoint_for({}) returned endpoint={}, slot={}",
                                pid, endpoint_id, slot
                            ));
                            self.service_cap_slots.insert(pid, slot);
                            self.log(&format!(
                                "Created endpoint {} for terminal (cap slot {})",
                                endpoint_id, slot
                            ));
                        }
                    }
                    Err(e) => {
                        self.log(&format!("Failed to spawn terminal: error {}", e));
                    }
                }
            }
            Err(e) if e == syscall_error::NOT_SUPPORTED => {
                // Browser WASM mode: terminals are spawned per-window by Desktop
                self.log("Terminal: will be spawned per-window by Desktop (browser mode)");
            }
            Err(e) => {
                // QEMU mode but terminal binary not found
                self.log(&format!("Failed to load terminal: error {}", e));
            }
        }
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
                        match syscall::create_endpoint_for(pid) {
                            Ok((endpoint_id, slot)) => {
                                self.log(&format!(
                                    "DEBUG: create_endpoint_for({}) returned eid={}, slot={}",
                                    pid, endpoint_id, slot
                                ));
                                self.service_cap_slots.insert(pid, slot);
                                self.log(&format!(
                                    "Created endpoint {} for {} (cap slot {})",
                                    endpoint_id, name, slot
                                ));
                            }
                            Err(e) => {
                                self.log(&format!(
                                    "ERROR: create_endpoint_for({}) failed: {}",
                                    pid, e
                                ));
                            }
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
