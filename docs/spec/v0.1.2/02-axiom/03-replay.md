# Replay: State Reconstruction

> State reconstruction replays the CommitLog to rebuild kernel state. This enables deterministic recovery, verification, and debugging.

## Overview

Replay provides:

1. **State Reconstruction**: Rebuild kernel state from CommitLog
2. **Deterministic Recovery**: Same commits always produce same state
3. **Checkpoint Optimization**: Skip to checkpoint, replay from there
4. **Verification**: Compare replayed state to expected state

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           State Reconstruction                              │
│                                                                             │
│   CommitLog                                                                 │
│   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐     │
│   │ Genesis │──▶│ Commit  │──▶│ Commit  │──▶│Checkpoint──▶│ Commit  │     │
│   │ seq: 0  │   │ seq: 1  │   │ seq: 2  │   │ seq: 100│   │ seq: 101│     │
│   └─────────┘   └─────────┘   └─────────┘   └─────────┘   └─────────┘     │
│        │              │              │              │              │        │
│        ▼              ▼              ▼              ▼              ▼        │
│   ┌─────────────────────────────────────────────────────────────────────┐  │
│   │                         apply(state, commit)                        │  │
│   │                                                                     │  │
│   │   reduce(genesis, commits) -> final_state                           │  │
│   └─────────────────────────────────────────────────────────────────────┘  │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Core Replay Function

The fundamental replay operation:

```rust
/// Replay the CommitLog to reconstruct kernel state.
///
/// # Properties
/// - **Deterministic**: Same commits always produce same state
/// - **Pure**: No side effects during replay
/// - **Atomic**: Each commit is applied atomically
///
/// # Complexity
/// O(n) where n is the number of commits
pub fn replay(commitlog: &CommitLog) -> KernelState {
    commitlog
        .commits
        .iter()
        .fold(KernelState::empty(), |mut state, commit| {
            apply(&mut state, commit);
            state
        })
}

/// Replay from a specific sequence number.
///
/// # Arguments
/// - `commitlog`: The commit log to replay
/// - `initial_state`: State to start from
/// - `start_seq`: First commit to apply
///
/// # Use Case
/// Used with checkpoints to skip replaying the entire log.
pub fn replay_from(
    commitlog: &CommitLog,
    mut state: KernelState,
    start_seq: u64,
) -> KernelState {
    for commit in commitlog.since(start_seq) {
        apply(&mut state, commit);
    }
    state
}
```

## Boot Sequence

On system boot, state is reconstructed from the CommitLog:

```rust
/// Boot sequence: reconstruct state from CommitLog.
///
/// # Steps
/// 1. Load CommitLog from storage
/// 2. Verify hash chain integrity
/// 3. Find latest checkpoint (optional optimization)
/// 4. Replay commits to reconstruct state
/// 5. Initialize kernel with reconstructed state
///
/// # Panics
/// Panics if CommitLog is corrupted or cannot be loaded.
pub fn boot_sequence(storage: &Storage) -> Kernel {
    // 1. Load CommitLog from storage
    let commitlog = storage
        .load_commit_log()
        .expect("CommitLog must be loadable");
    
    // 2. Verify hash chain (detect tampering)
    commitlog
        .verify_integrity()
        .expect("CommitLog must be intact");
    
    // 3. Find latest checkpoint for fast replay
    let state = if let Some((checkpoint_seq, checkpoint)) = commitlog.latest_checkpoint() {
        // Verify checkpoint signature
        let (state_hash, signature) = match &checkpoint.commit_type {
            CommitType::Checkpoint { state_hash, signature } => (state_hash, signature),
            _ => unreachable!(),
        };
        
        verify_checkpoint_signature(state_hash, signature)
            .expect("Checkpoint signature must be valid");
        
        // Load snapshot and replay from checkpoint
        let snapshot = storage
            .load_snapshot(checkpoint_seq)
            .expect("Checkpoint snapshot must exist");
        
        // Verify snapshot matches expected hash
        assert_eq!(
            snapshot.compute_hash(),
            *state_hash,
            "Snapshot hash mismatch"
        );
        
        // Replay only commits after checkpoint
        replay_from(&commitlog, snapshot, checkpoint_seq + 1)
    } else {
        // No checkpoint: replay from genesis
        replay(&commitlog)
    };
    
    // 4. Initialize kernel with reconstructed state
    Kernel::init(state, commitlog)
}
```

## Checkpoint Optimization

Checkpoints enable fast boot by skipping old commits:

```
Full Replay (no checkpoint):
┌─────────────────────────────────────────────────────────────────────────────┐
│ Genesis ──▶ Commit 1 ──▶ Commit 2 ──▶ ... ──▶ Commit 10000 ──▶ State       │
│                                                                             │
│ Time: O(n) where n = 10000 commits                                          │
└─────────────────────────────────────────────────────────────────────────────┘

Checkpoint Replay:
┌─────────────────────────────────────────────────────────────────────────────┐
│ [Skip] ──────────────────────▶ Checkpoint @ 9900 ──▶ ... ──▶ State         │
│                                     │                                       │
│                                     └─▶ Load snapshot (O(1))                │
│                                                                             │
│ Time: O(m) where m = 100 commits since checkpoint                           │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Checkpoint Strategy

```rust
/// Checkpoint creation strategy.
pub struct CheckpointStrategy {
    /// Create checkpoint every N commits
    pub interval_commits: u64,
    /// Create checkpoint every N seconds
    pub interval_seconds: u64,
    /// Maximum commits without checkpoint
    pub max_commits_without_checkpoint: u64,
}

impl Default for CheckpointStrategy {
    fn default() -> Self {
        Self {
            interval_commits: 1000,
            interval_seconds: 3600,  // 1 hour
            max_commits_without_checkpoint: 10000,
        }
    }
}

/// Check if a checkpoint should be created.
pub fn should_checkpoint(
    commitlog: &CommitLog,
    strategy: &CheckpointStrategy,
    current_time: u64,
) -> bool {
    let (last_checkpoint_seq, last_checkpoint) = match commitlog.latest_checkpoint() {
        Some(cp) => cp,
        None => return commitlog.current_seq() >= strategy.interval_commits,
    };
    
    let commits_since = commitlog.current_seq() - last_checkpoint_seq;
    let time_since = current_time - last_checkpoint.timestamp;
    
    commits_since >= strategy.interval_commits ||
    time_since >= strategy.interval_seconds * 1_000_000_000 ||
    commits_since >= strategy.max_commits_without_checkpoint
}
```

### Snapshot Storage

Checkpoints require storing state snapshots:

```rust
/// Store a state snapshot for a checkpoint.
///
/// # Arguments
/// - `storage`: Storage backend
/// - `seq`: Checkpoint sequence number
/// - `state`: Kernel state to snapshot
pub fn store_snapshot(
    storage: &mut Storage,
    seq: u64,
    state: &KernelState,
) -> Result<(), StorageError> {
    let serialized = serialize_state(state)?;
    storage.write(&format!("snapshots/{}", seq), &serialized)
}

/// Load a state snapshot.
pub fn load_snapshot(
    storage: &Storage,
    seq: u64,
) -> Result<KernelState, StorageError> {
    let data = storage.read(&format!("snapshots/{}", seq))?;
    deserialize_state(&data)
}
```

## KernelState Structure

The complete kernel state that gets reconstructed:

```rust
/// Complete kernel state reconstructed from CommitLog.
///
/// # Invariants
/// - All state is derivable from CommitLog
/// - State hash is deterministic for same commits
pub struct KernelState {
    /// Process table: PID -> Process
    pub processes: BTreeMap<ProcessId, Process>,
    
    /// Capability spaces: PID -> CSpace
    pub cspaces: BTreeMap<ProcessId, CapabilitySpace>,
    
    /// IPC endpoints: EID -> Endpoint
    pub endpoints: BTreeMap<EndpointId, Endpoint>,
    
    /// VMM states (for native targets): PID -> VmmState
    pub vmm_states: BTreeMap<ProcessId, VmmState>,
    
    /// ID generators (reconstructed from max IDs in commits)
    pub next_pid: u64,
    pub next_eid: u64,
    pub next_cap_id: u64,
}

impl KernelState {
    /// Create an empty state (before genesis).
    pub fn empty() -> Self {
        Self {
            processes: BTreeMap::new(),
            cspaces: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            vmm_states: BTreeMap::new(),
            next_pid: 1,
            next_eid: 1,
            next_cap_id: 1,
        }
    }
    
    /// Compute deterministic hash of the state.
    ///
    /// Used for checkpoint verification.
    pub fn compute_hash(&self) -> [u8; 32] {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        
        // Hash processes
        for (pid, proc) in &self.processes {
            hasher.update(&pid.0.to_le_bytes());
            hasher.update(&serialize_process(proc));
        }
        
        // Hash cspaces
        for (pid, cspace) in &self.cspaces {
            hasher.update(&pid.0.to_le_bytes());
            hasher.update(&serialize_cspace(cspace));
        }
        
        // Hash endpoints
        for (eid, endpoint) in &self.endpoints {
            hasher.update(&eid.0.to_le_bytes());
            hasher.update(&serialize_endpoint(endpoint));
        }
        
        // Hash ID generators
        hasher.update(&self.next_pid.to_le_bytes());
        hasher.update(&self.next_eid.to_le_bytes());
        hasher.update(&self.next_cap_id.to_le_bytes());
        
        hasher.finalize().into()
    }
}
```

## Verification

Replay enables state verification:

```rust
/// Verify that replayed state matches expected state.
///
/// # Use Cases
/// - Verify checkpoint integrity
/// - Detect state corruption
/// - Validate CommitLog consistency
pub fn verify_state(
    commitlog: &CommitLog,
    expected_hash: [u8; 32],
    at_seq: u64,
) -> Result<(), VerificationError> {
    // Replay up to the specified sequence
    let commits_to_replay = commitlog.get_range(0, at_seq + 1);
    let state = commits_to_replay
        .iter()
        .fold(KernelState::empty(), |mut s, c| {
            apply(&mut s, c);
            s
        });
    
    // Compare hashes
    let actual_hash = state.compute_hash();
    if actual_hash != expected_hash {
        return Err(VerificationError::HashMismatch {
            expected: expected_hash,
            actual: actual_hash,
            at_seq,
        });
    }
    
    Ok(())
}

/// Verification errors.
#[derive(Clone, Debug)]
pub enum VerificationError {
    /// State hash doesn't match expected
    HashMismatch {
        expected: [u8; 32],
        actual: [u8; 32],
        at_seq: u64,
    },
    /// Commit chain is broken
    ChainBroken { at_seq: u64 },
    /// Checkpoint signature invalid
    InvalidSignature { at_seq: u64 },
}
```

## Debugging with Replay

Replay enables powerful debugging:

```rust
/// Debug: reconstruct state at any point in history.
///
/// # Use Case
/// "What was the state when this bug occurred?"
pub fn state_at_seq(commitlog: &CommitLog, seq: u64) -> KernelState {
    commitlog
        .get_range(0, seq + 1)
        .iter()
        .fold(KernelState::empty(), |mut s, c| {
            apply(&mut s, c);
            s
        })
}

/// Debug: trace how a capability was created.
///
/// # Use Case
/// "How did PID 5 get this capability?"
pub fn trace_capability(
    commitlog: &CommitLog,
    pid: ProcessId,
    slot: CapSlot,
) -> Vec<&Commit> {
    commitlog
        .commits
        .iter()
        .filter(|c| {
            matches!(
                &c.commit_type,
                CommitType::CapInserted { pid: p, slot: s, .. }
                | CommitType::CapUpdated { pid: p, slot: s, .. }
                | CommitType::CapRemoved { pid: p, slot: s }
                if *p == pid && *s == slot
            )
        })
        .collect()
}

/// Debug: trace a process's lifecycle.
pub fn trace_process(commitlog: &CommitLog, pid: ProcessId) -> Vec<&Commit> {
    commitlog.commits_for_process(pid)
}

/// Debug: find what syscall caused a commit.
pub fn trace_cause(commitlog: &CommitLog, commit: &Commit, syslog: &SysLog) -> Option<&SysEvent> {
    commit.caused_by.as_ref().and_then(|id| syslog.get_by_id(id))
}
```

## First Boot (No CommitLog)

On first boot, there is no CommitLog. Genesis creates initial state:

```rust
/// First boot: create genesis CommitLog.
///
/// # Arguments
/// - `config`: Genesis configuration (init process, root caps)
/// - `timestamp`: Boot timestamp
pub fn first_boot(config: GenesisConfig, timestamp: u64) -> (CommitLog, KernelState) {
    // Create CommitLog with genesis
    let commitlog = CommitLog::new(config.clone(), timestamp);
    
    // Replay to get initial state
    let state = replay(&commitlog);
    
    (commitlog, state)
}

/// Default genesis configuration.
pub fn default_genesis() -> GenesisConfig {
    GenesisConfig {
        init_processes: vec![
            InitProcess {
                pid: ProcessId(1),
                name: "init".to_string(),
                binary: BinaryRef::Path("/system/init.wasm".to_string()),
            },
        ],
        root_caps: vec![
            // Init gets console capability
            RootCapability {
                holder: ProcessId(1),
                cap: Capability {
                    id: 1,
                    object_type: ObjectType::Console,
                    object_id: 0,
                    permissions: Permissions::full(),
                    generation: 0,
                    expires_at: 0,
                },
            },
            // Init gets spawn capability
            RootCapability {
                holder: ProcessId(1),
                cap: Capability {
                    id: 2,
                    object_type: ObjectType::Process,
                    object_id: 0,  // Root process cap
                    permissions: Permissions::full(),
                    generation: 0,
                    expires_at: 0,
                },
            },
        ],
    }
}
```

## WASM Boot Sequence

On WASM, boot loads from IndexedDB:

```rust
#[cfg(target_arch = "wasm32")]
pub async fn wasm_boot(db: &IndexedDb) -> Result<Kernel, BootError> {
    // Try to load existing CommitLog
    match CommitLog::load(db).await {
        Ok(commitlog) => {
            // Existing system: verify and replay
            commitlog.verify_integrity()?;
            
            let state = if let Some((seq, _)) = commitlog.latest_checkpoint() {
                let snapshot = load_snapshot(db, seq).await?;
                replay_from(&commitlog, snapshot, seq + 1)
            } else {
                replay(&commitlog)
            };
            
            Ok(Kernel::init(state, commitlog))
        }
        Err(StorageError::NoGenesis) => {
            // First boot: create genesis
            let (commitlog, state) = first_boot(
                default_genesis(),
                performance_now() as u64,
            );
            
            // Persist genesis
            commitlog.persist(db, 0).await?;
            
            Ok(Kernel::init(state, commitlog))
        }
        Err(e) => Err(BootError::Storage(e)),
    }
}
```

## Determinism Guarantees

Replay provides strong determinism guarantees:

```rust
/// Property: Same CommitLog always produces same state.
///
/// This is testable:
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn replay_is_deterministic() {
        let commitlog = create_test_commitlog();
        
        let state1 = replay(&commitlog);
        let state2 = replay(&commitlog);
        
        assert_eq!(state1.compute_hash(), state2.compute_hash());
    }
    
    #[test]
    fn replay_order_independent_of_timing() {
        // Create same commits with different timestamps
        let mut log1 = CommitLog::new(default_genesis(), 1000);
        let mut log2 = CommitLog::new(default_genesis(), 2000);
        
        // Add same commits (different timestamps don't affect state)
        log1.append(
            CommitType::ProcessCreated { 
                pid: ProcessId(2), 
                parent: ProcessId(1),
                binary: BinaryRef::Path("/test".into()),
            },
            None,
            1001,
        );
        log2.append(
            CommitType::ProcessCreated { 
                pid: ProcessId(2), 
                parent: ProcessId(1),
                binary: BinaryRef::Path("/test".into()),
            },
            None,
            5000,  // Different timestamp
        );
        
        let state1 = replay(&log1);
        let state2 = replay(&log2);
        
        // State is same (timestamps don't affect state content)
        assert_eq!(
            state1.processes.len(),
            state2.processes.len()
        );
    }
}
```

## Performance Considerations

Replay performance optimizations:

1. **Checkpoint Intervals**: Tune based on typical boot time requirements
2. **Snapshot Compression**: Compress snapshots to reduce storage
3. **Incremental Hashing**: Use incremental hash for state verification
4. **Parallel Replay**: Some commits may be parallelizable (future optimization)

```rust
/// Estimate replay time.
pub fn estimate_replay_time(
    commitlog: &CommitLog,
    commits_per_second: u64,
) -> Duration {
    let commits_to_replay = match commitlog.latest_checkpoint() {
        Some((seq, _)) => commitlog.current_seq() - seq,
        None => commitlog.current_seq(),
    };
    
    Duration::from_secs(commits_to_replay / commits_per_second)
}
```
