# Target Platforms

> Zero OS supports three deployment targets with increasing hardware access.

## Target Overview

| Target      | Description                          | Use Case                     |
|-------------|--------------------------------------|------------------------------|
| WASM        | Browser-hosted WebAssembly           | Development, demos, portable |
| QEMU        | Virtual machine (x86_64)             | Testing, development         |
| Bare Metal  | Direct hardware (x86_64)             | Production deployment        |

## WASM Target (Phase 1)

The primary development target. Runs in any modern browser.

### Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                          Browser Tab                                 │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                   Main Thread (Supervisor)                   │   │
│  │                                                             │   │
│  │  ┌──────────────┐   ┌──────────────┐   ┌──────────────┐    │   │
│  │  │ Zero_web  │   │   Kernel     │   │   UI/DOM     │    │   │
│  │  │ (WASM)       │◄──│ (Rust/WASM)  │──▶│ (HTML/CSS)   │    │   │
│  │  └──────────────┘   └──────────────┘   └──────────────┘    │   │
│  │         │                  │                                │   │
│  │         │ postMessage      │ postMessage                    │   │
│  │         ▼                  ▼                                │   │
│  │  ┌─────────────────────────────────────────────────────┐   │   │
│  │  │              Web Workers (Processes)                 │   │   │
│  │  │                                                     │   │   │
│  │  │  ┌──────────┐  ┌──────────┐  ┌──────────┐          │   │   │
│  │  │  │ Worker 1 │  │ Worker 2 │  │ Worker N │          │   │   │
│  │  │  │ (init)   │  │ (term)   │  │ (app)    │          │   │   │
│  │  │  └──────────┘  └──────────┘  └──────────┘          │   │   │
│  │  └─────────────────────────────────────────────────────┘   │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  Storage: IndexedDB       Time: performance.now()                   │
│  Network: Fetch API       Random: crypto.getRandomValues()          │
└─────────────────────────────────────────────────────────────────────┘
```

### Capabilities

| Feature          | Implementation                 | Limitations                    |
|------------------|--------------------------------|--------------------------------|
| Process spawn    | `new Worker()`                 | Same-origin only               |
| Process IPC      | `postMessage()`                | Must be serializable           |
| Memory           | WASM linear memory             | Max ~4GB per worker            |
| Scheduling       | Cooperative (`yield`)          | No preemption                  |
| Time             | `performance.now()`            | Coarse resolution              |
| Random           | `crypto.getRandomValues()`     | Synchronous                    |
| Storage          | IndexedDB                      | Async, quota limits            |
| Network          | Fetch API                      | CORS restrictions              |

### Compilation

```bash
# Build for WASM target
cargo build --target wasm32-unknown-unknown --release

# Optimize with wasm-opt (optional)
wasm-opt -O3 target/wasm32-unknown-unknown/release/*.wasm -o optimized.wasm
```

## QEMU Target (Phase 2)

Virtual machine target for testing preemptive scheduling and hardware access.

### Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                           QEMU x86_64                                 │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │                        Zero Kernel                           │ │
│  │                                                                 │ │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐       │ │
│  │  │ Scheduler │  │   VMM    │  │   IPC    │  │  Axiom   │       │ │
│  │  │ (APIC)   │  │ (x86 PT) │  │          │  │          │       │ │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────┘       │ │
│  │                                                                 │ │
│  │  ┌─────────────────────────────────────────────────────────┐   │ │
│  │  │                    HAL (QEMU)                            │   │ │
│  │  │                                                         │   │ │
│  │  │  APIC Timer    VirtIO-blk    VirtIO-net    VirtIO-rng  │   │ │
│  │  └─────────────────────────────────────────────────────────┘   │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │                      QEMU Virtual Hardware                      │ │
│  │                                                                 │ │
│  │  CPU (KVM)    RAM    VirtIO devices    Serial console          │ │
│  └────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────┘
```

### Capabilities

| Feature          | Implementation                 | Notes                          |
|------------------|--------------------------------|--------------------------------|
| Process spawn    | Fork + exec with page tables   | Full isolation                 |
| Scheduling       | APIC timer preemption          | Configurable quantum           |
| Memory           | x86_64 page tables             | 4-level paging                 |
| Time             | HPET / TSC                     | High resolution                |
| Random           | VirtIO-rng                     | Hardware entropy               |
| Storage          | VirtIO-blk                     | Block device                   |
| Network          | VirtIO-net                     | Ethernet frames                |
| Interrupts       | IOAPIC / MSI                   | Edge/level triggered           |

### Boot Sequence

1. QEMU loads kernel ELF at configured address
2. Kernel initializes: GDT, IDT, page tables
3. Kernel initializes APIC, HPET
4. Kernel mounts VirtIO devices
5. Init process started

## Bare Metal Target (Phase 7)

Production deployment on real hardware.

### Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│                        Physical Hardware                              │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │                        Zero Kernel                           │ │
│  │                                                                 │ │
│  │  (Same as QEMU, but with real hardware drivers)                 │ │
│  │                                                                 │ │
│  │  ┌─────────────────────────────────────────────────────────┐   │ │
│  │  │                    HAL (Native)                          │   │ │
│  │  │                                                         │   │ │
│  │  │  APIC    NVMe Driver    Intel NIC    RDRAND/RDSEED     │   │ │
│  │  └─────────────────────────────────────────────────────────┘   │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │                      Physical Hardware                          │ │
│  │                                                                 │ │
│  │  x86_64 CPU    DDR4 RAM    NVMe SSD    Intel/Realtek NIC       │ │
│  └────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────┘
```

### Additional Capabilities

| Feature          | Implementation                 | Notes                          |
|------------------|--------------------------------|--------------------------------|
| Random           | RDRAND / RDSEED                | CPU instruction                |
| Storage          | NVMe / AHCI drivers            | Real disk access               |
| Network          | Intel/Realtek drivers          | Real network                   |
| USB              | xHCI driver                    | USB devices                    |
| Graphics         | GOP / VESA                     | Basic display                  |

### Boot Sequence

1. UEFI loads bootloader
2. Bootloader loads kernel
3. Kernel initializes (same as QEMU)
4. Device enumeration via ACPI
5. Driver initialization
6. Init process started

## Feature Matrix

| Feature              | WASM      | QEMU      | Bare Metal |
|----------------------|-----------|-----------|------------|
| Process isolation    | Workers   | VMM       | VMM        |
| Preemptive sched     | No        | Yes       | Yes        |
| Multi-core           | No        | Yes       | Yes        |
| Real interrupts      | No        | Yes       | Yes        |
| Direct I/O           | No        | VirtIO    | Yes        |
| DMA                  | No        | VirtIO    | Yes        |
| Hardware RNG         | No        | VirtIO    | Yes        |
| Persistent storage   | IndexedDB | VirtIO    | Real disk  |
| Network              | Fetch     | VirtIO    | Real NIC   |
