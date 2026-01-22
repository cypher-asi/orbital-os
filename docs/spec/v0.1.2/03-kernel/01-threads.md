# Thread Model

> Threads are the unit of execution. On WASM, each process is single-threaded with cooperative scheduling.

## Overview

The thread subsystem manages:

1. **Thread Control Blocks (TCBs)**: Per-thread state
2. **Scheduling**: Deciding which thread runs next
3. **Context Switching**: Saving/restoring thread state

## Thread States

```
          ┌───────────────────────────────────────────────────┐
          │                                                   │
          ▼                                                   │
     ┌─────────┐     schedule      ┌─────────┐              │
     │  READY  │ ───────────────▶  │ RUNNING │              │
     └─────────┘                   └─────────┘              │
          ▲                            │                    │
          │                            │                    │
          │ unblock                    │ yield/preempt      │
          │                            │                    │
          │                            ▼                    │
     ┌─────────┐                  ┌─────────┐              │
     │ BLOCKED │ ◀────────────── │ WAITING │ ─────────────┘
     └─────────┘    block         └─────────┘    timeout
          │
          │ exit
          ▼
     ┌─────────┐
     │  ZOMBIE │
     └─────────┘
```

### States

| State     | Description                                      |
|-----------|--------------------------------------------------|
| `Ready`   | Runnable, waiting for CPU time                   |
| `Running` | Currently executing                              |
| `Waiting` | Blocked on IPC receive or sleep                  |
| `Blocked` | Blocked on resource (lock, I/O)                  |
| `Zombie`  | Exited, waiting for parent to collect status     |

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    /// Ready to run, in scheduler queue
    Ready,
    /// Currently running on a CPU
    Running,
    /// Waiting for IPC message or timeout
    Waiting { until: Option<u64> },
    /// Blocked on resource
    Blocked,
    /// Exited, waiting for cleanup
    Zombie { exit_code: i32 },
}
```

## Thread Control Block

```rust
/// Thread Control Block - per-thread kernel state.
pub struct Tcb {
    /// Thread ID (unique within process)
    pub tid: ThreadId,
    
    /// Owning process
    pub pid: ProcessId,
    
    /// Current state
    pub state: ThreadState,
    
    /// Saved CPU context (for context switch)
    pub context: ThreadContext,
    
    /// Kernel stack pointer
    pub kernel_stack: *mut u8,
    
    /// User stack pointer (saved on syscall entry)
    pub user_stack: usize,
    
    /// Scheduling priority (lower = higher priority)
    pub priority: u8,
    
    /// Time slice remaining (nanos)
    pub time_slice: u64,
    
    /// CPU affinity mask (for SMP)
    pub affinity: u64,
    
    /// Statistics
    pub stats: ThreadStats,
}

/// Thread statistics for debugging/monitoring.
#[derive(Clone, Debug, Default)]
pub struct ThreadStats {
    /// Total CPU time consumed (nanos)
    pub cpu_time: u64,
    /// Number of context switches
    pub context_switches: u64,
    /// Number of voluntary yields
    pub yields: u64,
    /// Number of syscalls
    pub syscalls: u64,
}
```

## Thread Context

Platform-specific saved state for context switching.

### WASM Context (Minimal)

On WASM, there's no true context switching—each Worker is single-threaded:

```rust
/// WASM thread context (minimal).
#[cfg(target_arch = "wasm32")]
pub struct ThreadContext {
    /// Pending syscall result (if any)
    pub pending_result: Option<SyscallResult>,
}
```

### x86_64 Context

On native x86_64, full register state must be saved:

```rust
/// x86_64 thread context.
#[cfg(target_arch = "x86_64")]
#[repr(C)]
pub struct ThreadContext {
    // Callee-saved registers
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    
    // Stack pointer
    pub rsp: u64,
    
    // Instruction pointer (return address)
    pub rip: u64,
    
    // Flags
    pub rflags: u64,
    
    // Segment selectors (usually constant)
    pub cs: u64,
    pub ss: u64,
    
    // FPU/SSE state (if used)
    pub fpu_state: Option<FpuState>,
}
```

## Scheduling

### WASM: Cooperative Scheduling

On WASM, scheduling is cooperative. Processes yield voluntarily:

```rust
// Process-side
pub fn yield_now() {
    unsafe { Zero_yield(); }
}

// Or implicitly when waiting for IPC:
pub fn receive_blocking(slot: u32) -> ReceivedMessage {
    loop {
        if let Some(msg) = receive(slot) {
            return msg;
        }
        yield_now();
    }
}
```

Supervisor-side scheduling loop (JavaScript):

```javascript
async function schedulerLoop() {
    while (true) {
        // Process messages from all workers
        for (const worker of workers) {
            await processWorkerMessages(worker);
        }
        
        // Yield to browser event loop
        await new Promise(r => setTimeout(r, 0));
    }
}
```

### Native: Preemptive Scheduling

On native targets, the scheduler is preemptive using timer interrupts:

```rust
/// Simple round-robin scheduler.
pub struct Scheduler {
    /// Ready queue
    ready: VecDeque<ThreadId>,
    /// Currently running thread per CPU
    current: [Option<ThreadId>; MAX_CPUS],
    /// Time slice duration (nanos)
    quantum: u64,
}

impl Scheduler {
    /// Called on timer interrupt.
    pub fn tick(&mut self, cpu: usize) -> Option<ThreadId> {
        let current = self.current[cpu]?;
        
        // Decrease time slice
        let tcb = get_tcb_mut(current);
        if tcb.time_slice > TICK_NANOS {
            tcb.time_slice -= TICK_NANOS;
            return None;  // Continue running
        }
        
        // Time slice exhausted, schedule next
        tcb.state = ThreadState::Ready;
        tcb.time_slice = self.quantum;
        self.ready.push_back(current);
        
        self.schedule(cpu)
    }
    
    /// Pick next thread to run.
    pub fn schedule(&mut self, cpu: usize) -> Option<ThreadId> {
        let next = self.ready.pop_front()?;
        self.current[cpu] = Some(next);
        get_tcb_mut(next).state = ThreadState::Running;
        Some(next)
    }
}
```

### Priority Scheduling (Future)

For more sophisticated scheduling:

```rust
/// Priority levels (0 = highest).
pub const PRIORITY_REALTIME: u8 = 0;
pub const PRIORITY_HIGH: u8 = 32;
pub const PRIORITY_NORMAL: u8 = 64;
pub const PRIORITY_LOW: u8 = 96;
pub const PRIORITY_IDLE: u8 = 128;

/// Priority queue with multiple levels.
pub struct PriorityScheduler {
    queues: [VecDeque<ThreadId>; 256],
    bitmap: [u64; 4],  // Quick lookup for non-empty queues
}
```

## Thread Operations

### Create Thread

```rust
/// Create a new thread in a process.
///
/// # Pre-conditions
/// - Caller has capability for the process with Write permission
/// - Process exists and is not a zombie
///
/// # Post-conditions
/// - New TCB created with state = Ready
/// - Thread added to scheduler
///
/// # WASM Note
/// Not supported on WASM Phase 1 (single-threaded processes).
pub fn thread_create(
    kernel: &mut Kernel,
    pid: ProcessId,
    entry: usize,
    stack: usize,
    arg: usize,
) -> Result<ThreadId, KernelError> {
    // Check capability (need Write permission on process)
    // ...
    
    // Allocate TID
    let tid = kernel.next_tid();
    
    // Create TCB
    let tcb = Tcb {
        tid,
        pid,
        state: ThreadState::Ready,
        context: ThreadContext::new(entry, stack, arg),
        kernel_stack: allocate_kernel_stack()?,
        user_stack: stack,
        priority: PRIORITY_NORMAL,
        time_slice: DEFAULT_QUANTUM,
        affinity: !0,  // All CPUs
        stats: ThreadStats::default(),
    };
    
    kernel.threads.insert(tid, tcb);
    kernel.scheduler.add(tid);
    
    Ok(tid)
}
```

### Exit Thread

```rust
/// Exit the current thread.
///
/// # Post-conditions
/// - Thread state set to Zombie
/// - Resources marked for cleanup
/// - If last thread in process, process becomes zombie
pub fn thread_exit(
    kernel: &mut Kernel,
    tid: ThreadId,
    exit_code: i32,
) {
    let tcb = kernel.threads.get_mut(&tid).expect("thread exists");
    tcb.state = ThreadState::Zombie { exit_code };
    
    // Remove from scheduler
    kernel.scheduler.remove(tid);
    
    // Check if this was the last thread
    let pid = tcb.pid;
    let remaining = kernel.threads.values()
        .filter(|t| t.pid == pid && !matches!(t.state, ThreadState::Zombie { .. }))
        .count();
    
    if remaining == 0 {
        // Process is done
        if let Some(proc) = kernel.processes.get_mut(&pid) {
            proc.state = ProcessState::Zombie;
        }
    }
    
    // Schedule next thread
    kernel.schedule();
}
```

### Yield

```rust
/// Voluntarily yield the CPU.
///
/// # Post-conditions
/// - Thread moved to back of ready queue
/// - Next thread scheduled
pub fn thread_yield(kernel: &mut Kernel, tid: ThreadId) {
    let tcb = kernel.threads.get_mut(&tid).expect("thread exists");
    tcb.state = ThreadState::Ready;
    tcb.stats.yields += 1;
    
    kernel.scheduler.add(tid);
    kernel.schedule();
}
```

### Block/Unblock

```rust
/// Block a thread waiting for a resource.
pub fn thread_block(kernel: &mut Kernel, tid: ThreadId) {
    let tcb = kernel.threads.get_mut(&tid).expect("thread exists");
    tcb.state = ThreadState::Blocked;
    kernel.schedule();
}

/// Unblock a thread (resource became available).
pub fn thread_unblock(kernel: &mut Kernel, tid: ThreadId) {
    let tcb = kernel.threads.get_mut(&tid).expect("thread exists");
    if matches!(tcb.state, ThreadState::Blocked) {
        tcb.state = ThreadState::Ready;
        kernel.scheduler.add(tid);
    }
}
```

## Context Switch

### x86_64 Context Switch

```rust
/// Switch from current thread to next thread.
///
/// # Safety
/// Must be called with interrupts disabled.
/// Must save all callee-saved registers.
#[cfg(target_arch = "x86_64")]
pub unsafe fn context_switch(from: &mut ThreadContext, to: &ThreadContext) {
    core::arch::asm!(
        // Save callee-saved registers
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        
        // Save stack pointer
        "mov [{from} + 48], rsp",
        
        // Load new stack pointer
        "mov rsp, [{to} + 48]",
        
        // Restore callee-saved registers
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",
        
        // Return to new thread
        "ret",
        from = in(reg) from,
        to = in(reg) to,
    );
}
```

## WASM Considerations

### Single-Threaded Model

On WASM Phase 1, each process is a single-threaded Web Worker:

- No `SYS_THREAD_CREATE` syscall
- No preemption (cooperative yield only)
- No kernel stack (syscalls are synchronous imports)
- Context is trivial (just pending results)

### Cooperative Yield

Processes must yield explicitly:

```rust
// In process code
loop {
    // Do work...
    
    // Yield to let other processes run
    Zero_process::yield_now();
}
```

### Blocking Operations

Blocking operations (like `receive_blocking`) internally loop with yields:

```rust
pub fn receive_blocking(slot: u32) -> ReceivedMessage {
    loop {
        if let Some(msg) = receive(slot) {
            return msg;
        }
        yield_now();  // Give other processes a chance
    }
}
```

## LOC Budget

Target: ~500 LOC for the thread subsystem.

| Component          | Estimated LOC |
|--------------------|---------------|
| TCB struct         | ~50           |
| Thread states      | ~30           |
| Scheduler          | ~150          |
| Context switch     | ~100          |
| Create/exit/yield  | ~120          |
| Block/unblock      | ~50           |
| **Total**          | **~500**      |
