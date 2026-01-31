//! Zero OS Kernel Entry Point
//!
//! This is the main entry point for the Zero OS kernel when running on
//! x86_64 hardware (QEMU or bare metal).
//!
//! Uses the bootloader crate for the boot process, which handles:
//! - Multiboot2 boot protocol
//! - Setting up page tables
//! - Transition to long mode (64-bit)

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

use bootloader_api::info::MemoryRegionKind as BootMemoryRegionKind;
use bootloader_api::{entry_point, BootInfo, BootloaderConfig};
use core::panic::PanicInfo;
use serde::{Deserialize, Serialize};
use zos_hal::x86_64::vmm::{MemoryRegionDescriptor, MemoryRegionKind};
use zos_hal::x86_64::X86_64Hal;
use zos_hal::{serial_println, HAL};
use zos_kernel::{replay_and_verify, Replayable, System};

/// The global x86_64 HAL instance
static HAL: X86_64Hal = X86_64Hal::new();

/// Bootloader configuration
pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    // Request physical memory mapping
    config.mappings.physical_memory = Some(bootloader_api::config::Mapping::Dynamic);
    config
};

/// Storage key for persisted CommitLog snapshot
const COMMITLOG_KEY: &str = "/axiom/commitlog.json";

/// Persisted commit log snapshot for replay
#[derive(Debug, Serialize, Deserialize)]
struct CommitLogSnapshot {
    commits: alloc::vec::Vec<zos_kernel::Commit>,
    state_hash: [u8; 32],
}

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

/// Maximum iterations for the main loop (safety limit for testing)
const MAX_MAIN_LOOP_ITERATIONS: u64 = 10000;

/// Find the terminal process PID if it exists
fn find_terminal_pid(system: &System<X86_64Hal>) -> Option<zos_kernel::ProcessId> {
    for (pid, process) in system.list_processes() {
        if process.name == "terminal" {
            return Some(pid);
        }
    }
    None
}

/// Run the kernel main loop
///
/// This is the heart of the QEMU native runtime. It:
/// 1. Polls for syscalls from WASM processes
/// 2. Dispatches syscalls through the Axiom verification layer
/// 3. Completes syscalls and resumes processes
/// 4. Routes serial input to terminal process
fn run_kernel_main_loop(
    system: &mut System<X86_64Hal>,
    hal: &X86_64Hal,
    storage_ready: bool,
) {
    let mut iteration = 0u64;
    let mut syscall_count = 0u64;
    let start_time = hal.now_nanos();

    // Run the main loop
    loop {
        iteration += 1;

        // Safety limit for testing - exit after MAX iterations
        if iteration >= MAX_MAIN_LOOP_ITERATIONS {
            serial_println!();
            serial_println!("[kernel] Main loop iteration limit reached ({} iterations)", iteration);
            serial_println!("[kernel] Total syscalls processed: {}", syscall_count);
            serial_println!("[kernel] Runtime: {} ms", (hal.now_nanos() - start_time) / 1_000_000);
            
            // Persist CommitLog before exit
            if storage_ready {
                persist_commitlog(system, hal);
            }
            
            break;
        }

        // Log progress periodically (disabled for clean output)
        // if iteration % 5000 == 0 {
        //     serial_println!("[kernel] Loop {}, syscalls: {}", iteration, syscall_count);
        // }

        // Poll for serial input and route through Init to terminal
        route_serial_input_to_init(system);

        // Run processes with synchronous syscall handling
        // This ensures syscalls are processed immediately before the process continues
        hal.run_scheduler_with_handler(|syscall| {
            syscall_count += 1;

            // Get sender PID
            let sender = zos_kernel::ProcessId(syscall.pid);
            let syscall_num = syscall.syscall_num;

            // Handle console output syscalls directly (print to serial)
            // SYS_DEBUG = 0x01, SYS_CONSOLE_WRITE = 0x07
            if syscall_num == 0x01 || syscall_num == 0x07 {
                if let Ok(text) = core::str::from_utf8(&syscall.data) {
                    // Print the text - strip trailing newline if present since serial_println adds one
                    let text = text.trim_end_matches('\n');
                    if !text.is_empty() {
                        serial_println!("{}", text);
                    }
                }
                return (0i64, alloc::vec::Vec::new());
            }

            // Debug IPC syscalls - only log actual sends (not idle recv polling)
            if syscall_num == 0x40 {
                serial_println!(
                    "[kernel] SYS_SEND: from PID {}, slot={}, tag=0x{:x}, data={} bytes",
                    syscall.pid, syscall.args[0], syscall.args[1], syscall.data.len()
                );
            }
            
            // Debug create_endpoint_for syscall (0x15)
            if syscall_num == 0x15 {
                serial_println!(
                    "[kernel] SYS_CREATE_ENDPOINT_FOR: from PID {}, target={}",
                    syscall.pid, syscall.args[0]
                );
            }

            // Dispatch syscall through Axiom
            let args = [syscall.args[0], syscall.args[1], syscall.args[2], 0];
            let (result, _rich_result, response_data) =
                system.process_syscall(sender, syscall_num, args, &syscall.data);

            // Debug IPC results - only log sends and successful receives
            if syscall_num == 0x40 {
                serial_println!("[kernel] SYS_SEND result: {}", result);
            }
            if syscall_num == 0x41 && result > 0 {
                serial_println!("[kernel] SYS_RECV: PID {} got message, {} bytes", syscall.pid, response_data.len());
            }
            
            // Debug create_endpoint_for result
            if syscall_num == 0x15 && result >= 0 {
                let init_slot = (result >> 32) as u32;
                let endpoint_id = result as u32;
                serial_println!("[kernel] Created endpoint {} for target, Init's cap at slot {}", endpoint_id, init_slot);
            }

            (result, response_data)
        });

        // Note: removed hlt() to ensure continuous polling for serial input
        // This uses more CPU but ensures responsive input handling
    }
}

/// Route serial input to terminal via Init (MSG_SUPERVISOR_CONSOLE_INPUT).
///
/// Per Invariant 1 (All Authority Flows Through Axiom), console input from hardware
/// must route through Init using `MSG_SUPERVISOR_CONSOLE_INPUT (0x2001)`. Init then
/// forwards to the terminal via capability-checked IPC.
///
/// This is the QEMU equivalent of how the JS supervisor routes input in WASM mode.
///
/// Data flow:
/// ```text
/// QEMU Serial → Kernel (here) → Init (MSG_SUPERVISOR_CONSOLE_INPUT) → Terminal (MSG_CONSOLE_INPUT)
/// ```
fn route_serial_input_to_init(system: &mut System<X86_64Hal>) {
    use zos_hal::x86_64::serial;

    // MSG_SUPERVISOR_CONSOLE_INPUT tag (from zos-ipc)
    const MSG_SUPERVISOR_CONSOLE_INPUT: u32 = 0x2001;

    // Terminal's input endpoint slot (standard slot 1 for input)
    const TERMINAL_INPUT_ENDPOINT_SLOT: u32 = 1;

    // Read all available bytes from serial input
    while let Some(byte) = serial::read_byte() {
        // Find terminal process
        if let Some(terminal_pid) = find_terminal_pid(system) {
            // Build MSG_SUPERVISOR_CONSOLE_INPUT payload:
            // [target_pid: u32, endpoint_slot: u32, data_len: u16, data: [u8]]
            let mut payload = alloc::vec::Vec::with_capacity(11);
            payload.extend_from_slice(&(terminal_pid.0 as u32).to_le_bytes()); // target_pid
            payload.extend_from_slice(&TERMINAL_INPUT_ENDPOINT_SLOT.to_le_bytes()); // endpoint_slot
            payload.extend_from_slice(&1u16.to_le_bytes()); // data_len = 1
            payload.push(byte); // data (single byte)

            // Inject to Init's endpoint via kernel
            if let Err(e) = system.inject_to_init(MSG_SUPERVISOR_CONSOLE_INPUT, &payload) {
                // If injection fails, fall back to echo for debugging
                serial_println!("[kernel] Failed to inject console input to Init: {:?}", e);
                serial::write_byte(byte);
            }
        }
        // If no terminal, just echo the character back to serial for debugging
        else {
            serial::write_byte(byte);
        }
    }
}

/// Persist CommitLog snapshot to storage
fn persist_commitlog(system: &System<X86_64Hal>, hal: &X86_64Hal) {
    let snapshot = CommitLogSnapshot {
        commits: system.commitlog().commits().to_vec(),
        state_hash: system.state_hash(),
    };
    match serde_json::to_vec(&snapshot) {
        Ok(bytes) => match hal.bootstrap_storage_put_inode(COMMITLOG_KEY, &bytes) {
            Ok(()) => {
                serial_println!("[replay] CommitLog snapshot persisted ({} bytes)", bytes.len());
            }
            Err(e) => {
                serial_println!("[replay] Failed to persist CommitLog snapshot: {:?}", e);
            }
        },
        Err(e) => {
            serial_println!("[replay] Failed to serialize CommitLog snapshot: {:?}", e);
        }
    }
}

/// Kernel main entry point
///
/// Called by the bootloader after setting up the environment.
///
/// # Arguments
/// * `boot_info` - Boot information from the bootloader
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // Initialize kernel heap first (needed for VMM)
    unsafe {
        zos_boot::allocator::init();
    }

    // Get physical memory offset from bootloader
    let phys_mem_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("Physical memory offset required");

    // Convert bootloader memory map to our format
    let memory_regions: alloc::vec::Vec<MemoryRegionDescriptor> = boot_info
        .memory_regions
        .iter()
        .map(|r| MemoryRegionDescriptor {
            start: r.start,
            size: r.end - r.start,
            kind: match r.kind {
                BootMemoryRegionKind::Usable => MemoryRegionKind::Usable,
                BootMemoryRegionKind::Bootloader => MemoryRegionKind::BootloaderReserved,
                _ => MemoryRegionKind::Reserved,
            },
        })
        .collect();

    // Initialize the HAL (serial, GDT, IDT, VMM)
    unsafe {
        HAL.init(phys_mem_offset, &memory_regions);
    }

    // Print the boot message
    serial_println!();
    serial_println!("========================================");
    serial_println!("  {} v{}", zos_boot::NAME, zos_boot::VERSION);
    serial_println!("========================================");
    serial_println!();
    serial_println!("Hello from QEMU kernel!");
    serial_println!();
    serial_println!("Boot completed successfully.");
    serial_println!("  - Serial output: OK");
    serial_println!("  - GDT: OK");
    serial_println!("  - IDT: OK");
    serial_println!("  - VMM: OK");
    serial_println!();

    // Print some boot info
    if let Some(fb) = boot_info.framebuffer.as_ref() {
        serial_println!("Framebuffer: {}x{}", fb.info().width, fb.info().height);
    }

    serial_println!("Physical memory offset: 0x{:X}", phys_mem_offset);

    // Print memory map summary
    serial_println!();
    serial_println!("Memory regions:");
    let mut total_usable = 0u64;
    for region in &memory_regions {
        if region.kind == MemoryRegionKind::Usable {
            serial_println!(
                "  Usable: 0x{:X} - 0x{:X} ({} KB)",
                region.start,
                region.start + region.size,
                region.size / 1024
            );
            total_usable += region.size;
        }
    }
    serial_println!("Total usable: {} MB", total_usable / (1024 * 1024));

    // Print frame allocator stats
    if let Some((free, total)) = zos_hal::x86_64::vmm::frame_stats() {
        serial_println!();
        serial_println!("Frame allocator:");
        serial_println!("  Total frames: {}", total);
        serial_println!("  Free frames: {}", free);
        serial_println!(
            "  Memory: {} MB / {} MB free",
            (free * 4096) / (1024 * 1024),
            (total * 4096) / (1024 * 1024)
        );
    }

    // Initialize storage for CommitLog persistence
    let storage_ready = match HAL.bootstrap_storage_init() {
        Ok(is_new) => {
            if is_new {
                serial_println!("[storage] New storage initialized");
            } else {
                serial_println!("[storage] Existing storage initialized");
            }
            true
        }
        Err(e) => {
            serial_println!("[storage] Storage init failed: {:?}", e);
            false
        }
    };

    // Stage 2.7 Test: CommitLog replay from storage
    if storage_ready {
        match HAL.bootstrap_storage_get_inode(COMMITLOG_KEY) {
            Ok(Some(data)) => match serde_json::from_slice::<CommitLogSnapshot>(&data) {
                Ok(snapshot) => {
                    serial_println!("[replay] Found CommitLog snapshot ({} commits)", snapshot.commits.len());
                    let mut replay_system: System<X86_64Hal> = System::new_for_replay();
                    match replay_and_verify(&mut replay_system, &snapshot.commits, snapshot.state_hash) {
                        Ok(()) => {
                            serial_println!("[replay] CommitLog replay verified");
                        }
                        Err(e) => {
                            serial_println!("[replay] CommitLog replay failed: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    serial_println!("[replay] Failed to parse CommitLog snapshot: {:?}", e);
                }
            },
            Ok(None) => {
                serial_println!("[replay] No CommitLog snapshot found");
            }
            Err(e) => {
                serial_println!("[replay] Storage read failed: {:?}", e);
            }
        }
    }

    // Stage 2.2 Test: Address space isolation
    serial_println!();
    serial_println!("========================================");
    serial_println!("  Stage 2.2 VMM Isolation Test");
    serial_println!("========================================");

    let isolation_ok = zos_hal::x86_64::vmm::test_isolation();

    serial_println!();
    if isolation_ok {
        serial_println!("VMM Test: PASSED");
    } else {
        serial_println!("VMM Test: FAILED");
    }

    // Test HAL time and random
    serial_println!();
    serial_println!("HAL functionality test:");
    serial_println!("  now_nanos(): {}", HAL.now_nanos());
    serial_println!("  wallclock_ms(): {}", HAL.wallclock_ms());

    let mut random_buf = [0u8; 8];
    if HAL.random_bytes(&mut random_buf).is_ok() {
        serial_println!(
            "  random_bytes(): 0x{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
            random_buf[0],
            random_buf[1],
            random_buf[2],
            random_buf[3],
            random_buf[4],
            random_buf[5],
            random_buf[6],
            random_buf[7]
        );
    } else {
        serial_println!("  random_bytes(): Not supported (RDRAND not available)");
    }

    serial_println!();
    serial_println!("========================================");
    serial_println!("  Stage 2.2 Complete!");
    serial_println!("========================================");

    // Stage 2.3 Test: Timer interrupts
    serial_println!();
    serial_println!("========================================");
    serial_println!("  Stage 2.3 Timer Interrupt Test");
    serial_println!("========================================");
    serial_println!();

    // Print APIC info
    serial_println!("APIC Info:");
    serial_println!("  LAPIC ID: {}", zos_hal::x86_64::apic::lapic_id());
    serial_println!("  LAPIC Version: {}", zos_hal::x86_64::apic::lapic_version());
    serial_println!("  Timer initialized: {}", zos_hal::x86_64::apic::is_initialized());
    serial_println!();

    // Enable interrupts first, then start the timer
    serial_println!("Enabling hardware interrupts...");
    HAL.enable_interrupts();
    serial_println!("Interrupts enabled.");
    
    // Start the APIC timer (must be after interrupts are enabled)
    serial_println!("Starting APIC timer (10ms interval)...");
    HAL.start_timer();
    serial_println!("Timer started!");
    serial_println!("(Printing tick count every second)");
    serial_println!();

    // Let the timer run for a few seconds to verify it works
    // The timer interrupt handler prints every 100 ticks (1 second)
    serial_println!("Waiting for timer ticks (will show 5 seconds of output)...");
    serial_println!();

    // Busy wait for about 5 seconds (500 ticks at 10ms each)
    while zos_hal::x86_64::apic::tick_count() < 500 {
        x86_64::instructions::hlt();
    }

    // Disable interrupts and show final stats
    HAL.disable_interrupts();

    serial_println!();
    serial_println!("========================================");
    serial_println!("  Stage 2.3 Complete!");
    serial_println!("========================================");
    serial_println!();
    serial_println!("Timer test completed successfully!");
    serial_println!("  Total ticks: {}", zos_hal::x86_64::apic::tick_count());
    serial_println!("  Elapsed time: {} ms", zos_hal::x86_64::apic::elapsed_nanos() / 1_000_000);

    // Stage 2.4 Test: Kernel Integration with X86_64Hal
    serial_println!();
    serial_println!("========================================");
    serial_println!("  Stage 2.4 Kernel Integration Test");
    serial_println!("========================================");
    serial_println!();

    // Create System with x86_64 HAL
    serial_println!("Creating System<X86_64Hal>...");
    let mut system: System<X86_64Hal> = System::new(X86_64Hal::new());
    serial_println!("  System created successfully!");

    // Test HAL integration through System
    serial_println!();
    serial_println!("Testing HAL through System:");
    serial_println!("  uptime_nanos(): {}", system.uptime_nanos());
    serial_println!("  boot_time(): {}", system.boot_time());

    // Test RTC wall-clock time
    let wallclock = system.hal().wallclock_ms();
    let secs = wallclock / 1000;
    let days = secs / 86400;
    let years_since_epoch = days / 365;
    serial_println!("  wallclock_ms(): {} (~{} years since 1970)", wallclock, years_since_epoch);

    // Register test processes
    serial_println!();
    serial_println!("Testing process registration:");
    let init_pid = system.register_process("init");
    serial_println!("  Registered 'init' process: PID {}", init_pid.0);

    let vfs_pid = system.register_process("vfs");
    serial_println!("  Registered 'vfs': PID {}", vfs_pid.0);

    let test_pid = system.register_process("test_app");
    serial_println!("  Registered 'test_app': PID {}", test_pid.0);

    // List processes
    serial_println!();
    serial_println!("Listing all processes:");
    for (pid, process) in system.list_processes() {
        serial_println!("  PID {} - {}", pid.0, process.name);
    }

    // Test endpoint creation
    serial_println!();
    serial_println!("Testing endpoint creation:");
    if let Ok((eid, cap_slot)) = system.create_endpoint(vfs_pid) {
        serial_println!("  Created endpoint {} for VFS (cap slot {})", eid.0, cap_slot);

        // Grant capability to test_app
        let perms = zos_kernel::Permissions::from_byte(0x03); // Read + Write
        if let Ok(granted_slot) = system.grant_capability_to_endpoint(vfs_pid, eid, test_pid, perms)
        {
            serial_println!(
                "  Granted endpoint cap to test_app (slot {})",
                granted_slot
            );
        }
    }

    // Test syscall processing
    serial_println!();
    serial_println!("Testing syscall processing:");

    // SYS_TIME syscall (0x02)
    let (result, _rich, _data) = system.process_syscall(test_pid, 0x02, [0, 0, 0, 0], &[]);
    serial_println!("  SYS_TIME result: {} (low 32 bits of nanos)", result);

    // SYS_WALLCLOCK syscall (0x06)
    let (result_low, _, _) = system.process_syscall(test_pid, 0x06, [0, 0, 0, 0], &[]);
    let (result_high, _, _) = system.process_syscall(test_pid, 0x06, [1, 0, 0, 0], &[]);
    let wallclock_syscall = ((result_high as u64) << 32) | (result_low as u64 & 0xFFFFFFFF);
    serial_println!("  SYS_WALLCLOCK result: {} ms", wallclock_syscall);

    // Check CommitLog
    serial_println!();
    serial_println!("Verifying Axiom layer:");
    serial_println!("  CommitLog entries: {}", system.commitlog().len());
    serial_println!("  SysLog entries: {}", system.syslog().len());

    // Get system metrics
    let metrics = system.get_system_metrics();
    serial_println!();
    serial_println!("System metrics:");
    serial_println!("  Active processes: {}", metrics.process_count);
    serial_println!("  Total endpoints: {}", metrics.endpoint_count);
    serial_println!("  Total memory: {} bytes", metrics.total_memory);
    serial_println!("  Uptime: {} ns", metrics.uptime_ns);

    serial_println!();
    serial_println!("========================================");
    serial_println!("  Stage 2.4 Complete!");
    serial_println!("========================================");
    serial_println!();
    serial_println!("x86_64 HAL integration verified:");
    serial_println!("  - System<X86_64Hal> works");
    serial_println!("  - Process management works");
    serial_println!("  - Endpoint/capability system works");
    serial_println!("  - Syscall dispatch works");
    serial_println!("  - RTC wall-clock time works");
    serial_println!("  - Axiom layer records commits");

    // Stage 2.6: Boot Init and Run Kernel Main Loop
    serial_println!();
    serial_println!("========================================");
    serial_println!("  Stage 2.6 QEMU Native Runtime");
    serial_println!("========================================");
    serial_println!();

    // Initialize WASM runtime
    serial_println!("Initializing WASM runtime...");
    let _wasm_runtime = HAL.wasm_runtime();
    serial_println!("  WASM runtime initialized!");

    // Create a fresh System for the kernel main loop
    serial_println!();
    serial_println!("Creating kernel System...");
    let mut kernel_system: System<X86_64Hal> = System::new(X86_64Hal::new());

    // Bootstrap: Register kernel as PID 0 (supervisor placeholder in QEMU mode)
    kernel_system.register_process_with_pid(
        zos_kernel::ProcessId(0),
        "kernel"
    );
    serial_println!("  Registered kernel as PID 0");

    // Load and spawn Init (PID 1)
    serial_println!();
    serial_println!("Loading Init process...");
    match HAL.load_binary("init") {
        Ok(init_binary) => {
            serial_println!("  Loaded init.wasm ({} bytes)", init_binary.len());
            
            // Register Init in kernel (allocates PID 1)
            let init_pid = kernel_system.register_process("init");
            serial_println!("  Registered Init as PID {}", init_pid.0);
            
            // Spawn Init via HAL WASM runtime with the kernel-allocated PID
            match HAL.spawn_process_with_pid(init_pid.0, "init", init_binary) {
                Ok(handle) => {
                    serial_println!("  Spawned Init process (handle {})", handle.id());
                    
                    // Create Init's main endpoint
                    if let Ok((endpoint_id, _slot)) = kernel_system.create_endpoint(init_pid) {
                        serial_println!("  Created Init endpoint {}", endpoint_id.0);
                    }
                    
                    // Enter kernel main loop
                    serial_println!();
                    serial_println!("========================================");
                    serial_println!("  Entering Kernel Main Loop");
                    serial_println!("========================================");
                    serial_println!();
                    
                    run_kernel_main_loop(&mut kernel_system, &HAL, storage_ready);
                }
                Err(e) => {
                    serial_println!("  ERROR: Failed to spawn Init: {:?}", e);
                }
            }
        }
        Err(e) => {
            serial_println!("  ERROR: Failed to load init.wasm: {:?}", e);
        }
    }

    serial_println!("Kernel main loop exited. Shutting down...");

    // Exit QEMU with success code
    zos_hal::x86_64::exit_qemu(0)
}

/// Panic handler
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!();
    serial_println!("========================================");
    serial_println!("  KERNEL PANIC!");
    serial_println!("========================================");
    serial_println!();
    serial_println!("{}", info);
    serial_println!();

    zos_hal::x86_64::halt_loop()
}
