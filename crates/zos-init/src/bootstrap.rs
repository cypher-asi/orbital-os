//! Boot sequence for Init process
//!
//! Handles the initial spawning of core system services.

use crate::Init;
use zos_process as syscall;

impl Init {
    /// Boot sequence - spawn PermissionManager, VfsService, IdentityService, and initial apps
    pub fn boot_sequence(&mut self) {
        self.log("Starting boot sequence...");

        // 1. Spawn PermissionManager (PID 2) - the capability authority
        self.log("Spawning PermissionManager (PID 2)...");
        syscall::debug("INIT:SPAWN:permission_manager");

        // 2. Spawn VfsService (PID 3) - virtual filesystem service
        // NOTE: VFS must be spawned before IdentityService since identity needs VFS
        self.log("Spawning VfsService (PID 3)...");
        syscall::debug("INIT:SPAWN:vfs_service");

        // 3. Spawn IdentityService (PID 4) - user identity and key management
        self.log("Spawning IdentityService (PID 4)...");
        syscall::debug("INIT:SPAWN:identity_service");

        // 4. Spawn TimeService (PID 5) - time settings management
        self.log("Spawning TimeService (PID 5)...");
        syscall::debug("INIT:SPAWN:time_service");

        // NOTE: Terminal is no longer auto-spawned here.
        // Each terminal window is spawned by the Desktop component via launchTerminal(),
        // which creates a process and links it to a window for proper lifecycle management.
        // This enables process isolation (each window has its own terminal process).

        self.boot_complete = true;
        self.log("Boot sequence complete");
        self.log("  PermissionManager: handles capability requests");
        self.log("  VfsService: handles filesystem operations");
        self.log("  IdentityService: handles identity and key management");
        self.log("  TimeService: handles time settings");
        self.log("  Terminal: spawned per-window by Desktop");
        self.log("Init entering minimal idle state");
    }
}
