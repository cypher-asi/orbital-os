# Syscall ABI

> The canonical system call interface for Zero OS.

## Overview

Syscalls are the only way user processes interact with the kernel. All syscalls pass through Axiom for capability checking before execution.

## Number Ranges

| Range         | Category        | Description                              |
|---------------|-----------------|------------------------------------------|
| `0x00 - 0x0F` | Misc            | Debug, info, time                        |
| `0x10 - 0x1F` | Thread          | Create, exit, yield, sleep               |
| `0x20 - 0x2F` | Memory          | Map, unmap, protect                      |
| `0x30 - 0x3F` | Capability      | Grant, revoke, transfer, inspect         |
| `0x40 - 0x4F` | IPC             | Send, receive, call, reply               |
| `0x50 - 0x5F` | IRQ             | Register handler, ack                    |
| `0x60 - 0x6F` | I/O             | Port read/write (x86)                    |
| `0xF0 - 0xFF` | Reserved        | Internal/debugging                       |

## Syscall Table

### Misc (0x00 - 0x0F)

| Number | Name           | Args                          | Returns        | WASM Phase 1 |
|--------|----------------|-------------------------------|----------------|--------------|
| `0x01` | `SYS_DEBUG`    | msg_ptr, msg_len              | 0              | Yes          |
| `0x02` | `SYS_GET_TIME` | 0=low32, 1=high32             | time bits      | Yes          |
| `0x03` | `SYS_GET_PID`  | (none)                        | pid            | Yes          |
| `0x04` | `SYS_LIST_CAPS`| buf_ptr, buf_len              | count          | Yes          |
| `0x05` | `SYS_LIST_PROCS`| buf_ptr, buf_len             | count          | Yes          |

### Thread (0x10 - 0x1F)

| Number | Name              | Args                       | Returns           | WASM Phase 1 |
|--------|-------------------|----------------------------|-------------------|--------------|
| `0x10` | `SYS_THREAD_CREATE`| entry_fn, stack_ptr, arg  | tid or error      | No           |
| `0x11` | `SYS_EXIT`        | exit_code                  | (never returns)   | Yes          |
| `0x12` | `SYS_YIELD`       | (none)                     | 0                 | Yes          |
| `0x13` | `SYS_SLEEP`       | nanos_low, nanos_high      | 0                 | No           |
| `0x14` | `SYS_THREAD_JOIN` | tid                        | exit_code         | No           |

### Memory (0x20 - 0x2F)

| Number | Name           | Args                          | Returns        | WASM Phase 1 |
|--------|----------------|-------------------------------|----------------|--------------|
| `0x20` | `SYS_MMAP`     | vaddr, size, perms, cap_slot  | addr or error  | No           |
| `0x21` | `SYS_MUNMAP`   | vaddr, size                   | 0 or error     | No           |
| `0x22` | `SYS_MPROTECT` | vaddr, size, new_perms        | 0 or error     | No           |

### Capability (0x30 - 0x3F)

| Number | Name              | Args                           | Returns        | WASM Phase 1 |
|--------|-------------------|--------------------------------|----------------|--------------|
| `0x30` | `SYS_CAP_GRANT`   | from_slot, to_pid, perms       | new_slot       | Yes          |
| `0x31` | `SYS_CAP_REVOKE`  | slot                           | 0 or error     | Yes          |
| `0x32` | `SYS_CAP_DELETE`  | slot                           | 0 or error     | Yes          |
| `0x33` | `SYS_CAP_INSPECT` | slot, buf_ptr                  | 0 or error     | Yes          |
| `0x34` | `SYS_CAP_DERIVE`  | slot, new_perms                | new_slot       | Yes          |
| `0x35` | `SYS_EP_CREATE`   | (none)                         | slot           | Yes          |

### IPC (0x40 - 0x4F)

| Number | Name           | Args                               | Returns        | WASM Phase 1 |
|--------|----------------|------------------------------------|----------------|--------------|
| `0x40` | `SYS_SEND`     | ep_slot, tag, data_ptr, data_len   | 0 or error     | Yes          |
| `0x41` | `SYS_RECEIVE`  | ep_slot, buf_ptr, buf_len          | msg_len or 0   | Yes          |
| `0x42` | `SYS_CALL`     | ep_slot, tag, data, buf            | reply_len      | Yes          |
| `0x43` | `SYS_REPLY`    | msg_id, data_ptr, data_len         | 0 or error     | Yes          |
| `0x44` | `SYS_SEND_CAP` | ep_slot, tag, cap_slots...         | 0 or error     | Yes          |

### IRQ (0x50 - 0x5F)

| Number | Name              | Args                    | Returns        | WASM Phase 1 |
|--------|-------------------|-------------------------|----------------|--------------|
| `0x50` | `SYS_IRQ_REGISTER`| irq_num, ep_slot        | 0 or error     | No           |
| `0x51` | `SYS_IRQ_ACK`     | irq_num                 | 0 or error     | No           |
| `0x52` | `SYS_IRQ_MASK`    | irq_num                 | 0 or error     | No           |
| `0x53` | `SYS_IRQ_UNMASK`  | irq_num                 | 0 or error     | No           |

### I/O (0x60 - 0x6F)

| Number | Name           | Args                    | Returns        | WASM Phase 1 |
|--------|----------------|-------------------------|----------------|--------------|
| `0x60` | `SYS_IO_IN8`   | port_cap_slot, offset   | byte value     | No           |
| `0x61` | `SYS_IO_IN16`  | port_cap_slot, offset   | word value     | No           |
| `0x62` | `SYS_IO_IN32`  | port_cap_slot, offset   | dword value    | No           |
| `0x63` | `SYS_IO_OUT8`  | port_cap_slot, offset, val | 0           | No           |
| `0x64` | `SYS_IO_OUT16` | port_cap_slot, offset, val | 0           | No           |
| `0x65` | `SYS_IO_OUT32` | port_cap_slot, offset, val | 0           | No           |

## Legacy Aliases (Phase 1 Compatibility)

To maintain compatibility with existing `zos-process` code, the kernel accepts legacy syscall numbers and maps them to the canonical ABI:

| Legacy Number | Legacy Name           | Canonical Number | Canonical Name    |
|---------------|-----------------------|------------------|-------------------|
| 1             | `SYS_DEBUG`           | `0x01`           | `SYS_DEBUG`       |
| 2             | `SYS_CREATE_ENDPOINT` | `0x35`           | `SYS_EP_CREATE`   |
| 3             | `SYS_SEND`            | `0x40`           | `SYS_SEND`        |
| 4             | `SYS_RECEIVE`         | `0x41`           | `SYS_RECEIVE`     |
| 5             | `SYS_LIST_CAPS`       | `0x04`           | `SYS_LIST_CAPS`   |
| 6             | `SYS_LIST_PROCESSES`  | `0x05`           | `SYS_LIST_PROCS`  |
| 7             | `SYS_EXIT`            | `0x11`           | `SYS_EXIT`        |
| 8             | `SYS_GET_TIME`        | `0x02`           | `SYS_GET_TIME`    |
| 9             | `SYS_YIELD`           | `0x12`           | `SYS_YIELD`       |

### Migration Code

```rust
/// Map legacy syscall number to canonical number.
/// Returns None if the number is already canonical or unknown.
pub fn legacy_to_canonical(legacy: u32) -> Option<u32> {
    match legacy {
        1 => Some(0x01),  // SYS_DEBUG
        2 => Some(0x35),  // SYS_CREATE_ENDPOINT -> SYS_EP_CREATE
        3 => Some(0x40),  // SYS_SEND
        4 => Some(0x41),  // SYS_RECEIVE
        5 => Some(0x04),  // SYS_LIST_CAPS
        6 => Some(0x05),  // SYS_LIST_PROCESSES
        7 => Some(0x11),  // SYS_EXIT
        8 => Some(0x02),  // SYS_GET_TIME
        9 => Some(0x12),  // SYS_YIELD
        _ => None,        // Already canonical or unknown
    }
}

/// Normalize syscall number (accept both legacy and canonical).
pub fn normalize_syscall(num: u32) -> u32 {
    legacy_to_canonical(num).unwrap_or(num)
}
```

## Syscall Invocation

### WASM ABI

On WASM, syscalls are invoked via imported host functions:

```rust
// Process-side (zos-process)
extern "C" {
    /// Make a syscall.
    /// 
    /// # Arguments
    /// - syscall_num: Syscall number (canonical or legacy)
    /// - arg1, arg2, arg3: Syscall-specific arguments
    ///
    /// # Returns
    /// - Result value (interpretation depends on syscall)
    fn Zero_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> u32;
    
    /// Send bytes to kernel (for data arguments).
    fn Zero_send_bytes(ptr: *const u8, len: u32);
    
    /// Receive bytes from kernel (for results with data).
    fn Zero_recv_bytes(ptr: *mut u8, max_len: u32) -> u32;
}
```

### Native ABI (Future)

On native targets (QEMU, bare metal), syscalls use the platform's syscall instruction:

```rust
// x86_64: SYSCALL instruction
// - RAX: syscall number
// - RDI, RSI, RDX, R10, R8, R9: arguments
// - RAX: return value
// - RCX, R11: clobbered

#[cfg(target_arch = "x86_64")]
pub unsafe fn syscall(num: u64, a1: u64, a2: u64, a3: u64) -> u64 {
    let ret: u64;
    core::arch::asm!(
        "syscall",
        inlateout("rax") num => ret,
        in("rdi") a1,
        in("rsi") a2,
        in("rdx") a3,
        lateout("rcx") _,
        lateout("r11") _,
    );
    ret
}
```

## Error Codes

| Code  | Name            | Description                              |
|-------|-----------------|------------------------------------------|
| 0     | `OK`            | Success                                  |
| 1     | `EPERM`         | Permission denied (capability check)     |
| 2     | `ENOENT`        | Object not found                         |
| 3     | `EINVAL`        | Invalid argument                         |
| 4     | `ENOSYS`        | Syscall not implemented                  |
| 5     | `EAGAIN`        | Would block (try again)                  |
| 6     | `ENOMEM`        | Out of memory                            |
| 7     | `EBADF`         | Invalid capability slot                  |
| 8     | `EBUSY`         | Resource busy                            |
| 9     | `EEXIST`        | Already exists                           |
| 10    | `EOVERFLOW`     | Buffer too small                         |

## Syscall Constants (Rust)

```rust
//! Syscall numbers for Zero OS.

// === Misc ===
pub const SYS_DEBUG: u32 = 0x01;
pub const SYS_GET_TIME: u32 = 0x02;
pub const SYS_GET_PID: u32 = 0x03;
pub const SYS_LIST_CAPS: u32 = 0x04;
pub const SYS_LIST_PROCS: u32 = 0x05;

// === Thread ===
pub const SYS_THREAD_CREATE: u32 = 0x10;
pub const SYS_EXIT: u32 = 0x11;
pub const SYS_YIELD: u32 = 0x12;
pub const SYS_SLEEP: u32 = 0x13;
pub const SYS_THREAD_JOIN: u32 = 0x14;

// === Memory ===
pub const SYS_MMAP: u32 = 0x20;
pub const SYS_MUNMAP: u32 = 0x21;
pub const SYS_MPROTECT: u32 = 0x22;

// === Capability ===
pub const SYS_CAP_GRANT: u32 = 0x30;
pub const SYS_CAP_REVOKE: u32 = 0x31;
pub const SYS_CAP_DELETE: u32 = 0x32;
pub const SYS_CAP_INSPECT: u32 = 0x33;
pub const SYS_CAP_DERIVE: u32 = 0x34;
pub const SYS_EP_CREATE: u32 = 0x35;

// === IPC ===
pub const SYS_SEND: u32 = 0x40;
pub const SYS_RECEIVE: u32 = 0x41;
pub const SYS_CALL: u32 = 0x42;
pub const SYS_REPLY: u32 = 0x43;
pub const SYS_SEND_CAP: u32 = 0x44;

// === IRQ ===
pub const SYS_IRQ_REGISTER: u32 = 0x50;
pub const SYS_IRQ_ACK: u32 = 0x51;
pub const SYS_IRQ_MASK: u32 = 0x52;
pub const SYS_IRQ_UNMASK: u32 = 0x53;

// === I/O ===
pub const SYS_IO_IN8: u32 = 0x60;
pub const SYS_IO_IN16: u32 = 0x61;
pub const SYS_IO_IN32: u32 = 0x62;
pub const SYS_IO_OUT8: u32 = 0x63;
pub const SYS_IO_OUT16: u32 = 0x64;
pub const SYS_IO_OUT32: u32 = 0x65;

// === Legacy Aliases (for backward compatibility) ===
pub mod legacy {
    pub const SYS_DEBUG: u32 = 1;
    pub const SYS_CREATE_ENDPOINT: u32 = 2;
    pub const SYS_SEND: u32 = 3;
    pub const SYS_RECEIVE: u32 = 4;
    pub const SYS_LIST_CAPS: u32 = 5;
    pub const SYS_LIST_PROCESSES: u32 = 6;
    pub const SYS_EXIT: u32 = 7;
    pub const SYS_GET_TIME: u32 = 8;
    pub const SYS_YIELD: u32 = 9;
}

// === Error Codes ===
pub const E_OK: u32 = 0;
pub const E_PERM: u32 = 1;
pub const E_NOENT: u32 = 2;
pub const E_INVAL: u32 = 3;
pub const E_NOSYS: u32 = 4;
pub const E_AGAIN: u32 = 5;
pub const E_NOMEM: u32 = 6;
pub const E_BADF: u32 = 7;
pub const E_BUSY: u32 = 8;
pub const E_EXIST: u32 = 9;
pub const E_OVERFLOW: u32 = 10;
```

## Phase 1 WASM Subset

The following syscalls are implemented in Phase 1 (WASM browser target):

- `SYS_DEBUG` (0x01)
- `SYS_GET_TIME` (0x02)
- `SYS_GET_PID` (0x03)
- `SYS_LIST_CAPS` (0x04)
- `SYS_LIST_PROCS` (0x05)
- `SYS_EXIT` (0x11)
- `SYS_YIELD` (0x12)
- `SYS_CAP_GRANT` (0x30)
- `SYS_CAP_REVOKE` (0x31)
- `SYS_CAP_DELETE` (0x32)
- `SYS_CAP_INSPECT` (0x33)
- `SYS_CAP_DERIVE` (0x34)
- `SYS_EP_CREATE` (0x35)
- `SYS_SEND` (0x40)
- `SYS_RECEIVE` (0x41)
- `SYS_CALL` (0x42)
- `SYS_REPLY` (0x43)
- `SYS_SEND_CAP` (0x44)

Not implemented in Phase 1:

- Thread creation (single-threaded WASM)
- Memory mapping (WASM manages linear memory)
- IRQ handling (no interrupts in WASM)
- I/O port access (no hardware in WASM)
