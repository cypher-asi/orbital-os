# Privilege Model

> Each target platform has different privilege separation mechanisms.

## Overview

Zero OS uses capability-based security at the software level (Axiom), but the underlying privilege enforcement differs by platform:

| Platform    | Privilege Mechanism            | Kernel Runs At      |
|-------------|--------------------------------|---------------------|
| WASM        | JavaScript sandbox             | Same-origin frame   |
| QEMU        | x86 ring levels + VMM          | Ring 0              |
| Bare Metal  | x86 ring levels + VMM          | Ring 0              |

## WASM (Browser)

### Privilege Levels

```
┌─────────────────────────────────────────────────────────────────────┐
│                       Browser Process                                │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                    Same-Origin Sandbox                      │    │
│  │                                                            │    │
│  │  ┌──────────────────────────────────────────────────────┐ │    │
│  │  │              Main Thread (Supervisor)                 │ │    │
│  │  │                                                      │ │    │
│  │  │  • Full DOM access                                   │ │    │
│  │  │  • Can spawn Workers                                 │ │    │
│  │  │  • Can access IndexedDB                              │ │    │
│  │  │  • Can make Fetch requests                           │ │    │
│  │  │  • Runs kernel + Zero_web WASM                    │ │    │
│  │  └──────────────────────────────────────────────────────┘ │    │
│  │                         │                                  │    │
│  │                    postMessage                             │    │
│  │                         │                                  │    │
│  │  ┌──────────────────────▼───────────────────────────────┐ │    │
│  │  │                  Web Workers                          │ │    │
│  │  │                                                      │ │    │
│  │  │  • No DOM access                                     │ │    │
│  │  │  • Can only postMessage to parent                    │ │    │
│  │  │  • Own WASM linear memory                            │ │    │
│  │  │  • Cannot spawn sub-workers (restricted)             │ │    │
│  │  │  • Each worker = one Zero process                 │ │    │
│  │  └──────────────────────────────────────────────────────┘ │    │
│  │                                                            │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                     │
│  Outside sandbox: Other tabs, origins, browser chrome               │
└─────────────────────────────────────────────────────────────────────┘
```

### Isolation Properties

| Property              | Enforcement                                |
|-----------------------|--------------------------------------------|
| Process memory        | Separate WASM linear memory per Worker     |
| IPC                   | Structured clone via postMessage           |
| Syscalls              | Import functions, supervisor validates     |
| Capability checking   | Supervisor-side (JavaScript + kernel WASM) |
| Storage               | IndexedDB (supervisor controls access)     |

### Trust Boundaries

1. **Browser ↔ OS**: Browser is TCB, provides isolation
2. **Supervisor ↔ Workers**: postMessage, structured clone
3. **Worker ↔ Worker**: No direct communication (must go through supervisor)

### Limitations

- No hardware protection between supervisor and workers (trust supervisor)
- postMessage overhead for IPC
- No preemption (cooperative scheduling)
- Single-threaded processes

## QEMU / Bare Metal (x86_64)

### Privilege Levels

```
┌─────────────────────────────────────────────────────────────────────┐
│                           Ring 0 (Kernel)                            │
│                                                                     │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │                     Zero Kernel                          │    │
│  │                                                            │    │
│  │  • Full CPU access                                         │    │
│  │  • All I/O ports                                          │    │
│  │  • Page table manipulation                                 │    │
│  │  • Interrupt handling                                      │    │
│  │  • APIC configuration                                      │    │
│  │  • Axiom capability system                                 │    │
│  └────────────────────────────────────────────────────────────┘    │
│                              │                                      │
│                         syscall                                     │
│                              │                                      │
├──────────────────────────────▼──────────────────────────────────────┤
│                           Ring 3 (User)                              │
│                                                                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                 │
│  │  Process 1  │  │  Process 2  │  │  Process N  │                 │
│  │             │  │             │  │             │                 │
│  │  • Own page │  │  • Own page │  │  • Own page │                 │
│  │    tables   │  │    tables   │  │    tables   │                 │
│  │  • Can only │  │  • Can only │  │  • Can only │                 │
│  │    syscall  │  │    syscall  │  │    syscall  │                 │
│  │  • No I/O   │  │  • No I/O   │  │  • No I/O   │                 │
│  └─────────────┘  └─────────────┘  └─────────────┘                 │
└─────────────────────────────────────────────────────────────────────┘
```

### Memory Protection

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Physical Memory                               │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                    Kernel Space (high half)                    │ │
│  │                                                                │ │
│  │  Mapped in all page tables, Ring 0 only                        │ │
│  │  Contains: kernel code, kernel heap, kernel stacks             │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                 │
│  │  Process 1  │  │  Process 2  │  │  Process N  │                 │
│  │  Address    │  │  Address    │  │  Address    │                 │
│  │  Space      │  │  Space      │  │  Space      │                 │
│  │             │  │             │  │             │                 │
│  │  Ring 3     │  │  Ring 3     │  │  Ring 3     │                 │
│  │  accessible │  │  accessible │  │  accessible │                 │
│  └─────────────┘  └─────────────┘  └─────────────┘                 │
│                                                                     │
│  Each process has unique CR3 (page table root)                      │
└─────────────────────────────────────────────────────────────────────┘
```

### x86_64 Protection Features

| Feature         | Description                                         |
|-----------------|-----------------------------------------------------|
| Ring levels     | 4 rings (we use 0 for kernel, 3 for user)          |
| Page tables     | 4-level paging with U/S bit                         |
| NX bit          | No-execute for data pages                           |
| SMEP            | Supervisor Mode Execution Prevention               |
| SMAP            | Supervisor Mode Access Prevention                   |
| CR3             | Per-process page table base                         |
| SYSCALL/SYSRET  | Fast privilege transition                           |

### Isolation Properties

| Property              | Enforcement                                |
|-----------------------|--------------------------------------------|
| Process memory        | Separate page tables (CR3)                 |
| Kernel memory         | Supervisor-only pages (U/S bit = 0)        |
| IPC                   | Kernel-mediated, capability checked        |
| Syscalls              | SYSCALL instruction, Ring 3 → Ring 0       |
| I/O                   | Ring 0 only (IOPL = 0)                     |
| Interrupts            | IDT in kernel space, Ring 0 handlers       |

### Capability + Hardware Interaction

```
┌─────────────────────────────────────────────────────────────────────┐
│  User Process (Ring 3)                                               │
│                                                                     │
│  Wants to send IPC message to endpoint                               │
│                                                                     │
│  1. Process holds capability for endpoint in slot 5                  │
│  2. Process calls: syscall(SYS_SEND, 5, tag, data_ptr, len)         │
└─────────────────────────────────────────────────────────────────┬───┘
                                                                  │
                                            SYSCALL instruction   │
                                                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│  Kernel (Ring 0)                                                     │
│                                                                     │
│  3. Trap handler saves registers                                     │
│  4. axiom_check(pid, slot=5, WRITE, ObjectType::Endpoint)           │
│     → Returns capability reference (or error)                        │
│  5. If granted: ipc_send(capability, tag, data)                     │
│  6. SYSRET back to user                                              │
└─────────────────────────────────────────────────────────────────────┘
```

## Summary Table

| Aspect            | WASM                  | QEMU/Bare Metal       |
|-------------------|-----------------------|-----------------------|
| TCB               | Browser + supervisor  | Kernel only           |
| Process isolation | Web Workers           | Page tables           |
| Kernel runs at    | JS + WASM (same-origin)| Ring 0              |
| User runs at      | Worker WASM           | Ring 3                |
| Syscall mechanism | Import function       | SYSCALL instruction   |
| Preemption        | Cooperative           | Timer interrupt       |
| Memory safety     | WASM linear memory    | Page tables + bounds  |

## Security Considerations

### WASM

- **Trusted Supervisor**: The JavaScript supervisor must be trusted. A malicious supervisor could forge capability checks.
- **Same-Origin Policy**: All code must come from the same origin.
- **No Hardware Isolation**: Software-only isolation via WASM semantics.

### QEMU / Bare Metal

- **Hardware-Enforced**: Ring levels and page tables enforced by CPU.
- **Kernel as TCB**: Only the kernel runs at Ring 0.
- **Spectre/Meltdown**: Modern mitigations (KPTI, retpolines) required.
- **Secure Boot**: Can verify kernel integrity at boot.
