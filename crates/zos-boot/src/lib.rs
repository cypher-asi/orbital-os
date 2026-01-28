//! Zero OS Boot Crate
//!
//! This crate provides the bootloader and early initialization code for
//! running Zero OS on x86_64 hardware (QEMU and bare metal).
//!
//! # Boot Process
//!
//! 1. **Bootloader**: Uses the `bootloader` crate to set up the environment
//!    - Sets up initial page tables with physical memory mapping
//!    - Transitions to long mode (64-bit)
//!    - Jumps to Rust `kernel_main`
//!
//! 2. **Rust Initialization** (`main.rs`):
//!    - Initializes the kernel heap
//!    - Initializes the x86_64 HAL (serial, GDT, IDT, VMM)
//!    - Runs Stage 2.2 VMM isolation tests
//!
//! # Architecture
//!
//! Most x86_64-specific code now lives in `zos-hal` under the `x86_64` feature:
//! - Serial output
//! - GDT/TSS setup
//! - IDT/exception handlers
//! - VMM (page tables, frame allocator, address spaces)
//!
//! This crate only contains:
//! - Kernel heap allocator (static allocation)
//! - Boot constants (name, version)

#![no_std]

extern crate alloc;

pub mod allocator;

/// Kernel version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Kernel name
pub const NAME: &str = "Zero OS";
