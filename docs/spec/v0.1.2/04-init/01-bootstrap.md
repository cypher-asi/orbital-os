# Bootstrap Sequence

> The bootstrap sequence loads the CommitLog, reconstructs kernel state, and initializes essential system services.

## Overview

Bootstrap proceeds in phases, each building on the previous:

1. **Pre-boot**: Axiom loads CommitLog, replays to reconstruct state
2. **Phase 0**: Init itself starts (from genesis or restored state)
3. **Phase 1**: Core services (terminal, storage)
4. **Phase 2**: System services (network, auth)
5. **Phase 3**: User services (applications)

## State Reconstruction

Before any processes run, Axiom reconstructs kernel state from the CommitLog:

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Pre-Boot: State Reconstruction                  │
│                                                                     │
│  1. Load CommitLog from storage                                      │
│  ─────────────────────────────────────────────────────────────────  │
│  │ Read commits from IndexedDB (WASM) or disk (native)              │
│  │ Verify hash chain integrity                                       │
│  └──────────────────────────────────────────────────────────────── │
│                                                                     │
│  2. Find latest checkpoint (optimization)                            │
│  ─────────────────────────────────────────────────────────────────  │
│  │ If checkpoint exists: load snapshot, verify signature             │
│  │ If no checkpoint: start from genesis                              │
│  └──────────────────────────────────────────────────────────────── │
│                                                                     │
│  3. Replay commits                                                   │
│  ─────────────────────────────────────────────────────────────────  │
│  │ Apply each commit to state: reduce(state, commit) -> state'       │
│  │ Reconstruct: processes, CSpaces, endpoints                        │
│  └──────────────────────────────────────────────────────────────── │
│                                                                     │
│  4. Initialize kernel with reconstructed state                       │
│  ─────────────────────────────────────────────────────────────────  │
│  │ Kernel receives fully reconstructed state                         │
│  │ Init process is ready to run                                      │
│  └──────────────────────────────────────────────────────────────── │
└─────────────────────────────────────────────────────────────────────┘
```

### Boot Sequence Code

```rust
/// Boot sequence: reconstruct state from CommitLog.
///
/// # Steps
/// 1. Load CommitLog from storage
/// 2. Verify hash chain integrity
/// 3. Find latest checkpoint (optional optimization)
/// 4. Replay commits to reconstruct state
/// 5. Initialize kernel with reconstructed state
pub fn boot_sequence(storage: &Storage) -> Kernel {
    // 1. Load CommitLog from storage
    let commitlog = match storage.load_commit_log() {
        Ok(log) => log,
        Err(StorageError::NotFound) => {
            // First boot: create genesis
            return first_boot(storage);
        }
        Err(e) => panic!("Failed to load CommitLog: {:?}", e),
    };
    
    // 2. Verify hash chain (detect tampering)
    commitlog
        .verify_integrity()
        .expect("CommitLog integrity check failed");
    
    // 3. Find latest checkpoint for fast replay
    let state = if let Some((checkpoint_seq, checkpoint)) = commitlog.latest_checkpoint() {
        // Load and verify checkpoint
        let snapshot = storage
            .load_snapshot(checkpoint_seq)
            .expect("Checkpoint snapshot must exist");
        
        // Replay only commits after checkpoint
        replay_from(&commitlog, snapshot, checkpoint_seq + 1)
    } else {
        // No checkpoint: replay from genesis
        replay(&commitlog)
    };
    
    // 4. Initialize kernel with reconstructed state
    Kernel::init(state, commitlog)
}

/// First boot: create genesis state.
fn first_boot(storage: &Storage) -> Kernel {
    let genesis_config = GenesisConfig {
        init_processes: vec![
            InitProcess {
                pid: ProcessId(1),
                name: "init".to_string(),
                binary: BinaryRef::Path("/system/init.wasm".to_string()),
            },
        ],
        root_caps: vec![/* init's root capabilities */],
    };
    
    let commitlog = CommitLog::new(genesis_config, now());
    let state = replay(&commitlog);
    
    // Persist genesis
    storage.persist_commit_log(&commitlog)
        .expect("Failed to persist genesis");
    
    Kernel::init(state, commitlog)
}
```

### First Boot vs Recovery Boot

| Scenario | CommitLog | Action |
|----------|-----------|--------|
| First boot | Does not exist | Create genesis, init from scratch |
| Normal boot | Exists, valid | Replay to reconstruct state |
| Corrupted boot | Exists, invalid hash | Panic or recover from backup |

## Bootstrap Phases

```
┌─────────────────────────────────────────────────────────────────────┐
│                      Bootstrap Timeline                              │
│                                                                     │
│  Phase 0: Init                                                       │
│  ─────────────────────────────────────────────────────────────────  │
│  │ Init spawned by kernel                                           │
│  │ Init creates IPC endpoint                                        │
│  │ Init requests capabilities from kernel                           │
│  └──────────────────────────────────────────────────────────────── │
│                                                                     │
│  Phase 1: Core Services                                              │
│  ─────────────────────────────────────────────────────────────────  │
│  │ Spawn: terminal (console I/O)                                    │
│  │   └─ Grant: Console endpoint (read/write)                        │
│  │ Spawn: storage (IndexedDB/disk access)                           │
│  │   └─ Grant: Storage capabilities                                 │
│  │ Wait for ready signals                                           │
│  └──────────────────────────────────────────────────────────────── │
│                                                                     │
│  Phase 2: System Services                                            │
│  ─────────────────────────────────────────────────────────────────  │
│  │ Spawn: network (Fetch API/NIC)                                   │
│  │   └─ Grant: Network capabilities                                 │
│  │ Spawn: auth (identity/permissions)                               │
│  │   └─ Grant: Auth capabilities                                    │
│  │ Wait for ready signals                                           │
│  └──────────────────────────────────────────────────────────────── │
│                                                                     │
│  Phase 3: User Services (optional)                                   │
│  ─────────────────────────────────────────────────────────────────  │
│  │ Read startup configuration                                       │
│  │ Spawn user-requested services                                    │
│  │ Enter supervision loop                                           │
│  └──────────────────────────────────────────────────────────────── │
└─────────────────────────────────────────────────────────────────────┘
```

## Service Dependencies

```
         ┌─────────┐
         │  init   │
         └────┬────┘
              │
    ┌─────────┼─────────┐
    │         │         │
    ▼         ▼         ▼
┌────────┐ ┌────────┐ ┌────────┐
│terminal│ │storage │ │ auth   │
└───┬────┘ └───┬────┘ └───┬────┘
    │          │          │
    │    ┌─────┴──────┐   │
    │    │            │   │
    │    ▼            ▼   │
    │ ┌────────┐ ┌────────┤
    │ │network │ │ apps   │◀── depends on storage + auth
    │ └────────┘ └────────┘
    │
    ▼
 (user I/O)
```

## Spawn Protocol

Init uses IPC to request the kernel spawn services:

```rust
/// Request to spawn a new service.
#[derive(Clone, Debug)]
pub struct SpawnRequest {
    /// Service name
    pub name: String,
    /// Binary path or inline binary
    pub binary: SpawnBinary,
    /// Capabilities to grant
    pub capabilities: Vec<CapabilityGrant>,
    /// Restart policy
    pub restart_policy: RestartPolicy,
}

pub enum SpawnBinary {
    /// Load from storage
    Path(String),
    /// Inline binary (for embedded services)
    Inline(Vec<u8>),
}

/// Capability to grant to spawned service.
pub struct CapabilityGrant {
    /// Capability to copy from init's CSpace
    pub source_slot: CapSlot,
    /// Permissions to grant (attenuated from source)
    pub permissions: Permissions,
}
```

### Spawn Message Flow

```
┌─────────────────┐                    ┌─────────────────┐
│      Init       │                    │     Kernel      │
│                 │                    │                 │
│  1. Build       │                    │                 │
│     SpawnRequest│                    │                 │
│                 │                    │                 │
│  2. Send to     │     SPAWN_REQ      │                 │
│     kernel      │───────────────────▶│  3. Validate    │
│                 │                    │     request     │
│                 │                    │                 │
│                 │                    │  4. Load binary │
│                 │                    │                 │
│                 │                    │  5. Create      │
│                 │                    │     process     │
│                 │                    │                 │
│                 │                    │  6. Grant caps  │
│                 │                    │                 │
│                 │     SPAWN_RESP     │  7. Start       │
│  8. Record PID  │◀───────────────────│     process     │
│                 │    { pid: 2 }      │                 │
│                 │                    │                 │
│                 │                    │                 │
│                 │     READY          │                 │
│  9. Mark ready  │◀───────────────────│◀── Service      │
│                 │    { from: 2 }     │    signals      │
└─────────────────┘                    └─────────────────┘
```

## Service Ready Protocol

Services signal readiness via IPC:

```rust
/// Service sends this when ready to handle requests.
pub const MSG_SERVICE_READY: u32 = 0x1000;

// In service code:
fn main() {
    // Initialize...
    
    // Signal ready
    send(init_endpoint, MSG_SERVICE_READY, &[]);
    
    // Start handling requests
    loop {
        let msg = receive_blocking(my_endpoint);
        handle(msg);
    }
}
```

## Bootstrap Configuration

Bootstrap can be configured via a simple configuration:

```rust
/// Bootstrap configuration (loaded from storage or embedded).
#[derive(Clone, Debug)]
pub struct BootstrapConfig {
    /// Services to start in order
    pub services: Vec<ServiceConfig>,
}

#[derive(Clone, Debug)]
pub struct ServiceConfig {
    /// Service name
    pub name: String,
    /// Binary path
    pub binary: String,
    /// Phase to start in
    pub phase: u8,
    /// Dependencies (service names)
    pub depends_on: Vec<String>,
    /// Restart policy
    pub restart: RestartPolicy,
    /// Environment variables / arguments
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug)]
pub enum RestartPolicy {
    /// Never restart
    Never,
    /// Restart on non-zero exit
    OnFailure,
    /// Always restart
    Always,
    /// Restart with backoff
    WithBackoff { initial_ms: u32, max_ms: u32 },
}
```

### Example Configuration

```toml
# /etc/Zero/init.toml (future)

[[services]]
name = "terminal"
binary = "/system/terminal.wasm"
phase = 1
restart = "always"

[[services]]
name = "storage"
binary = "/system/storage.wasm"
phase = 1
restart = "always"

[[services]]
name = "network"
binary = "/system/network.wasm"
phase = 2
depends_on = ["storage"]
restart = "on_failure"

[[services]]
name = "shell"
binary = "/system/shell.wasm"
phase = 3
depends_on = ["terminal", "storage"]
restart = "never"
```

## WASM Bootstrap

On WASM, bootstrap is simpler (fewer services, no persistent storage at boot):

```rust
fn bootstrap() {
    debug("init: bootstrap starting");
    
    // Phase 1: Terminal (for debug output)
    let terminal = spawn_and_wait("terminal");
    debug("init: terminal ready");
    
    // Phase 2: Network (optional, may fail)
    match spawn_and_wait_timeout("network", 5000) {
        Ok(pid) => debug("init: network ready"),
        Err(_) => debug("init: network not available"),
    }
    
    debug("init: bootstrap complete");
}

fn spawn_and_wait(name: &str) -> ProcessId {
    let pid = spawn(name);
    
    // Wait for ready message
    loop {
        if let Some(msg) = receive(my_endpoint) {
            if msg.tag == MSG_SERVICE_READY && msg.from == pid {
                return pid;
            }
        }
        yield_now();
    }
}
```

## Error Handling

Bootstrap errors are handled differently by phase:

| Phase | Error Handling                                |
|-------|-----------------------------------------------|
| 0     | Kernel panic (init failed to start)           |
| 1     | Retry with backoff, panic if persistent       |
| 2     | Log error, continue without service           |
| 3     | Log error, continue without service           |

```rust
fn handle_spawn_failure(phase: u8, name: &str, error: Error) {
    match phase {
        0 => panic!("init failed: {}", error),
        1 => {
            // Core service, must succeed
            for attempt in 0..5 {
                sleep(100 * (1 << attempt));  // Exponential backoff
                if let Ok(pid) = spawn(name) {
                    return pid;
                }
            }
            panic!("core service {} failed to start", name);
        }
        _ => {
            // Non-critical, log and continue
            debug(&format!("warning: {} failed to start: {}", name, error));
        }
    }
}
```
