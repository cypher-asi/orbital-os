# CommitLog: State Change Record

> The CommitLog records all state mutations as an append-only, hash-chained log. It is the source of truth for deterministic replay.

## Overview

The CommitLog provides:

1. **State Mutations**: Records every change to kernel state
2. **Hash Chain**: Tamper-evident chain of commits
3. **Deterministic Replay**: `reduce(genesis, commits) -> state`
4. **Checkpoints**: Periodic signed state snapshots

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│ Commit (Genesis)│◀───│ Commit          │◀───│ Commit          │
│                 │    │                 │    │                 │
│ prev: 0x000...  │    │ prev: 0xabc...  │    │ prev: 0xdef...  │
│ seq: 0          │    │ seq: 1          │    │ seq: 2          │
│ type: Genesis   │    │ type: CapInsert │    │ type: ProcessEx │
│ caused_by: None │    │ caused_by: e123 │    │ caused_by: e456 │
│ id: 0xabc...    │    │ id: 0xdef...    │    │ id: 0x123...    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

## Data Structures

### Commit

A state mutation event:

```rust
/// A state mutation event in the CommitLog.
///
/// # Invariants
/// - Commits are append-only; once written, never modified
/// - `prev_commit` must match the hash of the previous commit
/// - `seq` must be exactly `prev_seq + 1`
/// - `id` is computed as SHA-256 of the commit contents
#[derive(Clone, Debug)]
pub struct Commit {
    // ═══════════════════════════════════════════════════════════════════════
    // HEADER (hash chain)
    // ═══════════════════════════════════════════════════════════════════════
    
    /// Hash of this commit (SHA-256)
    pub id: CommitId,
    
    /// Hash of the previous commit (chain integrity)
    pub prev_commit: CommitId,
    
    /// Monotonic sequence number (never reused, no gaps)
    pub seq: u64,
    
    /// Timestamp (nanoseconds since boot)
    pub timestamp: u64,
    
    // ═══════════════════════════════════════════════════════════════════════
    // BODY
    // ═══════════════════════════════════════════════════════════════════════
    
    /// The state change
    pub commit_type: CommitType,
    
    /// Which SysEvent caused this (for tracing)
    pub caused_by: Option<EventId>,
}

/// SHA-256 hash identifying a Commit.
pub type CommitId = [u8; 32];

/// Zero hash (used for genesis commit's prev_commit).
pub const ZERO_HASH: CommitId = [0u8; 32];
```

### CommitType

All state mutation types:

```rust
/// Types of state mutations recorded in the CommitLog.
///
/// # Note
/// Failed operations generate NO commits. The CommitLog only
/// contains successful state changes.
#[derive(Clone, Debug)]
pub enum CommitType {
    // ═══════════════════════════════════════════════════════════════════════
    // GENESIS
    // ═══════════════════════════════════════════════════════════════════════
    
    /// Initial state (first commit, no previous state).
    Genesis {
        /// Initial system configuration
        config: GenesisConfig,
    },
    
    // ═══════════════════════════════════════════════════════════════════════
    // PROCESS LIFECYCLE
    // ═══════════════════════════════════════════════════════════════════════
    
    /// A new process was created.
    ProcessCreated {
        pid: ProcessId,
        parent: ProcessId,
        binary: BinaryRef,
    },
    
    /// A process exited.
    ProcessExited {
        pid: ProcessId,
        code: i32,
    },
    
    // ═══════════════════════════════════════════════════════════════════════
    // CAPABILITY CHANGES
    // ═══════════════════════════════════════════════════════════════════════
    
    /// A capability was inserted into a CSpace.
    CapInserted {
        pid: ProcessId,
        slot: CapSlot,
        cap: Capability,
    },
    
    /// A capability was removed from a CSpace.
    CapRemoved {
        pid: ProcessId,
        slot: CapSlot,
    },
    
    /// A capability's permissions were updated.
    CapUpdated {
        pid: ProcessId,
        slot: CapSlot,
        new_perms: Permissions,
    },
    
    // ═══════════════════════════════════════════════════════════════════════
    // IPC ENDPOINT CHANGES
    // ═══════════════════════════════════════════════════════════════════════
    
    /// An IPC endpoint was created.
    EndpointCreated {
        id: EndpointId,
        owner: ProcessId,
    },
    
    /// An IPC endpoint was destroyed.
    EndpointDestroyed {
        id: EndpointId,
    },
    
    // ═══════════════════════════════════════════════════════════════════════
    // MEMORY CHANGES (for VMM state)
    // ═══════════════════════════════════════════════════════════════════════
    
    /// Memory was mapped into a process's address space.
    MemoryMapped {
        pid: ProcessId,
        vaddr: u64,
        size: u64,
        perms: MemoryPermissions,
    },
    
    /// Memory was unmapped from a process's address space.
    MemoryUnmapped {
        pid: ProcessId,
        vaddr: u64,
        size: u64,
    },
    
    // ═══════════════════════════════════════════════════════════════════════
    // CHECKPOINTS
    // ═══════════════════════════════════════════════════════════════════════
    
    /// Periodic checkpoint with signed state hash.
    Checkpoint {
        /// Hash of the complete kernel state at this point
        state_hash: [u8; 32],
        /// Signature over state_hash (for verification)
        signature: Signature,
    },
}
```

### GenesisConfig

Initial system configuration:

```rust
/// Genesis configuration for the initial system state.
#[derive(Clone, Debug)]
pub struct GenesisConfig {
    /// Initial processes to create (typically just init)
    pub init_processes: Vec<InitProcess>,
    /// System-wide capabilities to create
    pub root_caps: Vec<RootCapability>,
}

/// An initial process created at genesis.
#[derive(Clone, Debug)]
pub struct InitProcess {
    pub pid: ProcessId,
    pub name: String,
    pub binary: BinaryRef,
}

/// A root capability created at genesis.
#[derive(Clone, Debug)]
pub struct RootCapability {
    pub holder: ProcessId,
    pub cap: Capability,
}
```

### Supporting Types

```rust
/// Memory permission flags.
#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryPermissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

/// Cryptographic signature.
#[derive(Clone, Debug)]
pub struct Signature {
    /// Signature algorithm identifier
    pub algorithm: SignatureAlgorithm,
    /// Signature bytes
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
pub enum SignatureAlgorithm {
    Ed25519,
    Secp256k1,
}
```

## CommitLog Structure

```rust
/// The CommitLog: append-only, hash-chained log of state mutations.
///
/// # Invariants
/// - Commits are append-only; never modified or removed
/// - Hash chain is maintained: each commit's `prev_commit` = previous commit's `id`
/// - Sequence numbers are contiguous: no gaps, no duplicates
/// - Commits with `caused_by` link back to valid SysEvent IDs
pub struct CommitLog {
    /// Log entries (append-only)
    commits: Vec<Commit>,
    /// Current sequence number
    next_seq: u64,
    /// Hash of the last commit
    last_hash: CommitId,
}

impl CommitLog {
    /// Create a new CommitLog with genesis commit.
    ///
    /// # Arguments
    /// - `config`: Genesis configuration
    /// - `timestamp`: Genesis timestamp
    pub fn new(config: GenesisConfig, timestamp: u64) -> Self {
        let genesis = Commit {
            id: ZERO_HASH,  // Computed below
            prev_commit: ZERO_HASH,
            seq: 0,
            timestamp,
            commit_type: CommitType::Genesis { config },
            caused_by: None,
        };
        
        let id = compute_commit_id(&genesis);
        let genesis = Commit { id, ..genesis };
        
        Self {
            commits: vec![genesis],
            next_seq: 1,
            last_hash: id,
        }
    }
    
    /// Append a commit to the log.
    ///
    /// # Arguments
    /// - `commit_type`: The state change
    /// - `caused_by`: The SysEvent that triggered this (if any)
    /// - `timestamp`: When this commit was created
    ///
    /// # Returns
    /// The CommitId of the appended commit
    ///
    /// # Invariants maintained
    /// - `prev_commit` is set to the previous commit's hash
    /// - `seq` is set to the next sequence number
    /// - Hash chain integrity is preserved
    pub fn append(
        &mut self,
        commit_type: CommitType,
        caused_by: Option<EventId>,
        timestamp: u64,
    ) -> CommitId {
        let commit = Commit {
            id: ZERO_HASH,
            prev_commit: self.last_hash,
            seq: self.next_seq,
            timestamp,
            commit_type,
            caused_by,
        };
        
        let id = compute_commit_id(&commit);
        let commit = Commit { id, ..commit };
        
        self.last_hash = id;
        self.next_seq += 1;
        self.commits.push(commit);
        
        id
    }
    
    /// Get the genesis commit.
    pub fn genesis(&self) -> &Commit {
        &self.commits[0]
    }
    
    /// Get commit by sequence number.
    pub fn get(&self, seq: u64) -> Option<&Commit> {
        self.commits.get(seq as usize)
    }
    
    /// Get commit by ID.
    pub fn get_by_id(&self, id: &CommitId) -> Option<&Commit> {
        self.commits.iter().find(|c| &c.id == id)
    }
    
    /// Get commits in a sequence range.
    pub fn get_range(&self, start_seq: u64, end_seq: u64) -> &[Commit] {
        let start = start_seq as usize;
        let end = (end_seq as usize).min(self.commits.len());
        &self.commits[start..end]
    }
    
    /// Get all commits since a given sequence number.
    pub fn since(&self, seq: u64) -> &[Commit] {
        let start = seq as usize;
        &self.commits[start..]
    }
    
    /// Get the current sequence number.
    pub fn current_seq(&self) -> u64 {
        self.next_seq - 1
    }
    
    /// Get the hash of the latest commit.
    pub fn head(&self) -> CommitId {
        self.last_hash
    }
}

/// Compute SHA-256 hash of a Commit (excluding the id field).
fn compute_commit_id(commit: &Commit) -> CommitId {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(&commit.prev_commit);
    hasher.update(&commit.seq.to_le_bytes());
    hasher.update(&commit.timestamp.to_le_bytes());
    hasher.update(&serialize_commit_type(&commit.commit_type));
    if let Some(ref event_id) = commit.caused_by {
        hasher.update(event_id);
    }
    hasher.finalize().into()
}
```

## Hash Chain Verification

The CommitLog is hash-chained for tamper-evidence:

```rust
impl CommitLog {
    /// Verify the integrity of the hash chain.
    ///
    /// # Returns
    /// - `Ok(())` if the chain is valid
    /// - `Err(seq)` with the sequence number of the first invalid commit
    ///
    /// # Complexity
    /// O(n) where n is the number of commits
    pub fn verify_integrity(&self) -> Result<(), u64> {
        let mut expected_prev = ZERO_HASH;
        
        for commit in &self.commits {
            // Check prev_commit matches
            if commit.prev_commit != expected_prev {
                return Err(commit.seq);
            }
            
            // Recompute and verify hash
            let computed_id = compute_commit_id(commit);
            if commit.id != computed_id {
                return Err(commit.seq);
            }
            
            expected_prev = commit.id;
        }
        
        Ok(())
    }
    
    /// Verify chain from a known-good checkpoint.
    ///
    /// # Arguments
    /// - `checkpoint_seq`: Sequence number of trusted checkpoint
    /// - `checkpoint_hash`: Expected hash at that sequence
    ///
    /// # Returns
    /// - `Ok(())` if chain from checkpoint to head is valid
    /// - `Err(seq)` with the sequence number of the first invalid commit
    pub fn verify_from_checkpoint(
        &self,
        checkpoint_seq: u64,
        checkpoint_hash: CommitId,
    ) -> Result<(), u64> {
        // Verify checkpoint matches
        let checkpoint = self.get(checkpoint_seq).ok_or(checkpoint_seq)?;
        if checkpoint.id != checkpoint_hash {
            return Err(checkpoint_seq);
        }
        
        // Verify chain from checkpoint
        let mut expected_prev = checkpoint_hash;
        for commit in self.since(checkpoint_seq + 1) {
            if commit.prev_commit != expected_prev {
                return Err(commit.seq);
            }
            
            let computed_id = compute_commit_id(commit);
            if commit.id != computed_id {
                return Err(commit.seq);
            }
            
            expected_prev = commit.id;
        }
        
        Ok(())
    }
}
```

## Checkpoints

Checkpoints are periodic commits with a signed state hash:

```rust
impl CommitLog {
    /// Create a checkpoint commit.
    ///
    /// # Arguments
    /// - `state`: Current kernel state to snapshot
    /// - `signer`: Key for signing the checkpoint
    /// - `timestamp`: Current timestamp
    ///
    /// # Returns
    /// The CommitId of the checkpoint commit
    pub fn checkpoint(
        &mut self,
        state: &KernelState,
        signer: &SigningKey,
        timestamp: u64,
    ) -> CommitId {
        let state_hash = state.compute_hash();
        let signature = signer.sign(&state_hash);
        
        self.append(
            CommitType::Checkpoint { state_hash, signature },
            None,
            timestamp,
        )
    }
    
    /// Find the most recent checkpoint.
    pub fn latest_checkpoint(&self) -> Option<(u64, &Commit)> {
        self.commits
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| matches!(c.commit_type, CommitType::Checkpoint { .. }))
            .map(|(i, c)| (i as u64, c))
    }
    
    /// Find all checkpoints.
    pub fn checkpoints(&self) -> impl Iterator<Item = (u64, &Commit)> {
        self.commits
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c.commit_type, CommitType::Checkpoint { .. }))
            .map(|(i, c)| (i as u64, c))
    }
}
```

## Commit as Pure Function

Each commit is a pure state transformation:

```rust
/// Apply a commit to kernel state.
///
/// # Properties
/// - **Pure**: No side effects. Output depends only on State + Commit
/// - **Deterministic**: Same (State, Commit) always produces same State'
/// - **Atomic**: All-or-nothing. No partial application
///
/// # Panics
/// Panics if the commit cannot be applied (indicates log corruption).
pub fn apply(state: &mut KernelState, commit: &Commit) {
    match &commit.commit_type {
        CommitType::Genesis { config } => {
            // Initialize from genesis config
            for proc in &config.init_processes {
                state.processes.insert(proc.pid, Process::new(proc));
                state.cspaces.insert(proc.pid, CapabilitySpace::new());
            }
            for cap in &config.root_caps {
                state.cspaces
                    .get_mut(&cap.holder)
                    .expect("holder must exist")
                    .insert(cap.cap.clone());
            }
        }
        
        CommitType::ProcessCreated { pid, parent, binary } => {
            state.processes.insert(*pid, Process {
                pid: *pid,
                parent: *parent,
                binary: binary.clone(),
                state: ProcessState::Ready,
            });
            state.cspaces.insert(*pid, CapabilitySpace::new());
        }
        
        CommitType::ProcessExited { pid, code } => {
            if let Some(proc) = state.processes.get_mut(pid) {
                proc.state = ProcessState::Exited(*code);
            }
            // Note: CSpace cleanup is handled by separate CapRemoved commits
        }
        
        CommitType::CapInserted { pid, slot, cap } => {
            if let Some(cspace) = state.cspaces.get_mut(pid) {
                cspace.insert_at(*slot, cap.clone());
            }
        }
        
        CommitType::CapRemoved { pid, slot } => {
            if let Some(cspace) = state.cspaces.get_mut(pid) {
                cspace.remove(*slot);
            }
        }
        
        CommitType::CapUpdated { pid, slot, new_perms } => {
            if let Some(cspace) = state.cspaces.get_mut(pid) {
                if let Some(cap) = cspace.get_mut(*slot) {
                    cap.permissions = *new_perms;
                }
            }
        }
        
        CommitType::EndpointCreated { id, owner } => {
            state.endpoints.insert(*id, Endpoint::new(*id, *owner));
        }
        
        CommitType::EndpointDestroyed { id } => {
            state.endpoints.remove(id);
        }
        
        CommitType::MemoryMapped { pid, vaddr, size, perms } => {
            if let Some(vmm) = state.vmm_states.get_mut(pid) {
                vmm.map(*vaddr, *size, *perms);
            }
        }
        
        CommitType::MemoryUnmapped { pid, vaddr, size } => {
            if let Some(vmm) = state.vmm_states.get_mut(pid) {
                vmm.unmap(*vaddr, *size);
            }
        }
        
        CommitType::Checkpoint { .. } => {
            // Checkpoints don't modify state; they just snapshot it
        }
    }
}
```

## Error Handling

The CommitLog NEVER contains failed operations:

| Scenario | SysLog | CommitLog |
|----------|--------|-----------|
| Successful CapGrant | Request + Ok response | CapInserted |
| Failed CapGrant (no permission) | Request + Err response | (nothing) |
| Successful Spawn | Request + Ok response | ProcessCreated, CapInserted... |
| Failed Spawn (invalid binary) | Request + Err response | (nothing) |

This ensures:
- CommitLog is always consistent
- Replay never encounters errors
- State reconstruction is deterministic

## WASM Persistence

On WASM, the CommitLog is persisted to IndexedDB:

```rust
#[cfg(target_arch = "wasm32")]
impl CommitLog {
    /// Persist commits to IndexedDB.
    ///
    /// # Arguments
    /// - `db`: IndexedDB database handle
    /// - `since_seq`: Only persist commits after this sequence number
    pub async fn persist(
        &self,
        db: &IndexedDb,
        since_seq: u64,
    ) -> Result<(), StorageError> {
        let tx = db.transaction("commit_log", TransactionMode::ReadWrite)?;
        let store = tx.object_store("commit_log")?;
        
        for commit in self.since(since_seq) {
            store.put(&commit.seq.to_le_bytes(), &serialize(commit))?;
        }
        
        tx.commit().await
    }
    
    /// Load CommitLog from IndexedDB.
    pub async fn load(db: &IndexedDb) -> Result<Self, StorageError> {
        let tx = db.transaction("commit_log", TransactionMode::ReadOnly)?;
        let store = tx.object_store("commit_log")?;
        
        let commits: Vec<Commit> = store
            .get_all()?
            .await?
            .into_iter()
            .map(|bytes| deserialize(&bytes))
            .collect::<Result<_, _>>()?;
        
        if commits.is_empty() {
            return Err(StorageError::NoGenesis);
        }
        
        let last = commits.last().unwrap();
        Ok(Self {
            next_seq: last.seq + 1,
            last_hash: last.id,
            commits,
        })
    }
}
```

## Commit Batching

For performance, multiple commits can be batched atomically:

```rust
impl CommitLog {
    /// Append multiple commits atomically.
    ///
    /// # Arguments
    /// - `commits`: Commit types to append
    /// - `caused_by`: The SysEvent that triggered these
    /// - `timestamp`: When these commits were created
    ///
    /// # Returns
    /// Vector of CommitIds for the appended commits
    pub fn append_batch(
        &mut self,
        commits: Vec<CommitType>,
        caused_by: Option<EventId>,
        timestamp: u64,
    ) -> Vec<CommitId> {
        commits
            .into_iter()
            .map(|ct| self.append(ct, caused_by, timestamp))
            .collect()
    }
}
```

## Querying the CommitLog

Common queries:

```rust
impl CommitLog {
    /// Get all commits affecting a process.
    pub fn commits_for_process(&self, pid: ProcessId) -> Vec<&Commit> {
        self.commits
            .iter()
            .filter(|c| commit_affects_process(&c.commit_type, pid))
            .collect()
    }
    
    /// Get all commits caused by a SysEvent.
    pub fn commits_for_event(&self, event_id: &EventId) -> Vec<&Commit> {
        self.commits
            .iter()
            .filter(|c| c.caused_by.as_ref() == Some(event_id))
            .collect()
    }
    
    /// Get commits in a time range.
    pub fn commits_in_range(&self, start: u64, end: u64) -> Vec<&Commit> {
        self.commits
            .iter()
            .filter(|c| c.timestamp >= start && c.timestamp <= end)
            .collect()
    }
}

fn commit_affects_process(ct: &CommitType, pid: ProcessId) -> bool {
    match ct {
        CommitType::ProcessCreated { pid: p, .. } => *p == pid,
        CommitType::ProcessExited { pid: p, .. } => *p == pid,
        CommitType::CapInserted { pid: p, .. } => *p == pid,
        CommitType::CapRemoved { pid: p, .. } => *p == pid,
        CommitType::CapUpdated { pid: p, .. } => *p == pid,
        CommitType::MemoryMapped { pid: p, .. } => *p == pid,
        CommitType::MemoryUnmapped { pid: p, .. } => *p == pid,
        CommitType::EndpointCreated { owner, .. } => *owner == pid,
        _ => false,
    }
}
```
