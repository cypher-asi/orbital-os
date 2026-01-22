# Hardware Abstraction Layer

> The HAL provides a platform-independent interface for kernel operations.

## Overview

The Hardware Abstraction Layer (HAL) isolates the kernel from platform-specific details, enabling the same kernel code to run on:

1. **WASM (Browser)** - Web Workers for processes, JavaScript APIs for time/entropy
2. **QEMU** - Virtual hardware, preemptive scheduling
3. **Bare Metal** - Direct hardware access

## Files

| File                              | Description                          |
|-----------------------------------|--------------------------------------|
| [01-targets.md](01-targets.md)    | Target platforms and capabilities    |
| [02-privilege.md](02-privilege.md)| Privilege models per target          |
| [03-traits.md](03-traits.md)      | HAL trait interface                  |

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              Kernel                                      │
│                                                                         │
│   Uses HAL trait for all platform operations:                           │
│   - Process spawn/kill                                                  │
│   - Memory allocation                                                   │
│   - Time measurement                                                    │
│   - Entropy (random bytes)                                              │
│   - Debug output                                                        │
└─────────────────────────────────────┬───────────────────────────────────┘
                                      │ HAL trait
                  ┌───────────────────┼───────────────────┐
                  │                   │                   │
                  ▼                   ▼                   ▼
         ┌──────────────┐    ┌──────────────┐    ┌──────────────┐
         │   WASM HAL   │    │   QEMU HAL   │    │  Native HAL  │
         │              │    │              │    │              │
         │ Web Workers  │    │ VirtIO devs  │    │ Real HW      │
         │ JS APIs      │    │ APIC/HPET    │    │ APIC/NVMe    │
         │ IndexedDB    │    │ virtio-blk   │    │ drivers      │
         └──────────────┘    └──────────────┘    └──────────────┘
```

## HAL Trait Summary

```rust
pub trait HAL: Send + Sync + 'static {
    /// Handle to a spawned process
    type ProcessHandle: Clone + Send + Sync;
    
    // Process management
    fn spawn_process(&self, name: &str, binary: &[u8]) -> Result<Self::ProcessHandle, HalError>;
    fn kill_process(&self, handle: &Self::ProcessHandle) -> Result<(), HalError>;
    fn send_to_process(&self, handle: &Self::ProcessHandle, msg: &[u8]) -> Result<(), HalError>;
    fn is_process_alive(&self, handle: &Self::ProcessHandle) -> bool;
    fn get_process_memory_size(&self, handle: &Self::ProcessHandle) -> Result<usize, HalError>;
    
    // Memory
    fn allocate(&self, size: usize, align: usize) -> Result<*mut u8, HalError>;
    fn deallocate(&self, ptr: *mut u8, size: usize, align: usize);
    
    // Time & entropy
    fn now_nanos(&self) -> u64;
    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError>;
    
    // Debug
    fn debug_write(&self, msg: &str);
    
    // Message polling
    fn poll_messages(&self) -> Vec<(Self::ProcessHandle, Vec<u8>)>;
}
```

See [03-traits.md](03-traits.md) for full documentation.

## Platform Comparison

| Capability          | WASM               | QEMU              | Bare Metal        |
|--------------------|--------------------|-------------------|-------------------|
| Process isolation  | Web Workers        | Hardware VMM      | Hardware MMU      |
| Preemption         | No (cooperative)   | Yes (timer IRQ)   | Yes (APIC timer)  |
| Multi-threading    | No                 | Yes               | Yes               |
| Memory protection  | WASM linear memory | Page tables       | Page tables       |
| IRQ handling       | N/A                | IOAPIC            | APIC/MSI-X        |
| Storage            | IndexedDB          | virtio-blk        | NVMe/SATA         |
| Network            | Fetch API          | virtio-net        | NIC drivers       |
