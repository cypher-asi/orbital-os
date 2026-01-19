//! Commit Log for Deterministic Replay
//!
//! Records state mutations as commits for deterministic replay.
//! Each commit links to the previous via hash chain.
//!
//! # Core Invariant
//!
//! > `reduce(genesis, commits) -> state`
//!
//! Replaying the same CommitLog always produces the same state.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::types::{CapSlot, CommitId, EndpointId, EventId, Permissions, ProcessId};

/// A state mutation record.
///
/// Commits are append-only and form a hash chain for integrity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commit {
    /// Hash of this commit (computed from contents + prev_commit)
    pub id: CommitId,
    /// Hash of the previous commit (chain integrity)
    pub prev_commit: CommitId,
    /// Sequence number (monotonic)
    pub seq: u64,
    /// Timestamp (nanos since boot)
    pub timestamp: u64,
    /// The type of state mutation
    pub commit_type: CommitType,
    /// Optional: the syscall event that caused this commit
    pub caused_by: Option<EventId>,
}

/// Types of state mutations (for deterministic replay).
///
/// Each variant represents a discrete state change that can be
/// replayed to reconstruct kernel state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CommitType {
    /// Genesis commit (system boot)
    Genesis,

    // === Process Lifecycle ===
    /// Process created
    ProcessCreated {
        pid: ProcessId,
        parent: ProcessId,
        name: String,
    },
    /// Process exited
    ProcessExited { pid: ProcessId, code: i32 },

    // === Capability Mutations ===
    /// Capability inserted into a process's CSpace
    CapInserted {
        pid: ProcessId,
        slot: CapSlot,
        cap_id: u64,
        object_type: u8,
        object_id: u64,
        perms: u8,
    },
    /// Capability removed from a process's CSpace
    CapRemoved { pid: ProcessId, slot: CapSlot },
    /// Capability granted from one process to another
    CapGranted {
        from_pid: ProcessId,
        to_pid: ProcessId,
        from_slot: CapSlot,
        to_slot: CapSlot,
        new_cap_id: u64,
        perms: Permissions,
    },

    // === Endpoint Lifecycle ===
    /// Endpoint created
    EndpointCreated { id: EndpointId, owner: ProcessId },
    /// Endpoint destroyed
    EndpointDestroyed { id: EndpointId },
}

/// Maximum number of commits to keep in memory
const MAX_COMMITLOG_ENTRIES: usize = 100000;

/// Commit log for deterministic replay.
///
/// All state-changing operations are recorded as commits.
/// Replaying commits from genesis reconstructs the exact state.
pub struct CommitLog {
    /// Commit entries (append-only)
    commits: Vec<Commit>,
    /// Next sequence number
    next_seq: u64,
    /// Hash of the last commit
    last_hash: CommitId,
}

impl CommitLog {
    /// Create a new CommitLog with a genesis commit.
    pub fn new(timestamp: u64) -> Self {
        let genesis = Commit {
            id: [0u8; 32], // Will be computed
            prev_commit: [0u8; 32],
            seq: 0,
            timestamp,
            commit_type: CommitType::Genesis,
            caused_by: None,
        };
        let id = Self::compute_hash(&genesis);
        let genesis = Commit { id, ..genesis };

        Self {
            commits: vec![genesis],
            next_seq: 1,
            last_hash: id,
        }
    }

    /// Append a new commit to the log.
    ///
    /// Returns the commit ID (hash).
    pub fn append(
        &mut self,
        commit_type: CommitType,
        caused_by: Option<EventId>,
        timestamp: u64,
    ) -> CommitId {
        let commit = Commit {
            id: [0u8; 32], // Will be computed
            prev_commit: self.last_hash,
            seq: self.next_seq,
            timestamp,
            commit_type,
            caused_by,
        };
        let id = Self::compute_hash(&commit);
        let commit = Commit { id, ..commit };

        self.last_hash = id;
        self.next_seq += 1;
        self.commits.push(commit);

        self.trim_if_needed();
        id
    }

    /// Compute hash for a commit.
    ///
    /// Uses FNV-1a hash for no_std compatibility.
    /// In production, this could use SHA-256.
    fn compute_hash(commit: &Commit) -> CommitId {
        let mut hash = 0xcbf29ce484222325u64; // FNV offset basis
        const FNV_PRIME: u64 = 0x100000001b3;

        // Hash prev_commit
        for byte in commit.prev_commit {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        // Hash seq
        for byte in commit.seq.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        // Hash timestamp
        for byte in commit.timestamp.to_le_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }

        // Hash commit_type discriminant
        let type_byte = match &commit.commit_type {
            CommitType::Genesis => 0u8,
            CommitType::ProcessCreated { .. } => 1,
            CommitType::ProcessExited { .. } => 2,
            CommitType::CapInserted { .. } => 3,
            CommitType::CapRemoved { .. } => 4,
            CommitType::CapGranted { .. } => 5,
            CommitType::EndpointCreated { .. } => 6,
            CommitType::EndpointDestroyed { .. } => 7,
        };
        hash ^= type_byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);

        // Hash additional type-specific data
        match &commit.commit_type {
            CommitType::Genesis => {}
            CommitType::ProcessCreated { pid, parent, name } => {
                for byte in pid.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in parent.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in name.bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
            }
            CommitType::ProcessExited { pid, code } => {
                for byte in pid.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in code.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
            }
            CommitType::CapInserted {
                pid,
                slot,
                cap_id,
                object_type,
                object_id,
                perms,
            } => {
                for byte in pid.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in slot.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in cap_id.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                hash ^= *object_type as u64;
                hash = hash.wrapping_mul(FNV_PRIME);
                for byte in object_id.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                hash ^= *perms as u64;
                hash = hash.wrapping_mul(FNV_PRIME);
            }
            CommitType::CapRemoved { pid, slot } => {
                for byte in pid.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in slot.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
            }
            CommitType::CapGranted {
                from_pid,
                to_pid,
                from_slot,
                to_slot,
                new_cap_id,
                perms,
            } => {
                for byte in from_pid.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in to_pid.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in from_slot.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in to_slot.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in new_cap_id.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                hash ^= perms.to_byte() as u64;
                hash = hash.wrapping_mul(FNV_PRIME);
            }
            CommitType::EndpointCreated { id, owner } => {
                for byte in id.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
                for byte in owner.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
            }
            CommitType::EndpointDestroyed { id } => {
                for byte in id.to_le_bytes() {
                    hash ^= byte as u64;
                    hash = hash.wrapping_mul(FNV_PRIME);
                }
            }
        }

        // Expand to 32 bytes
        let mut result = [0u8; 32];
        let mut h = hash;
        for chunk in result.chunks_mut(8) {
            let bytes = h.to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
            h = h.wrapping_mul(FNV_PRIME);
        }
        result
    }

    /// Get all commits.
    pub fn commits(&self) -> &[Commit] {
        &self.commits
    }

    /// Get commits in a sequence range.
    pub fn get_range(&self, start_seq: u64, end_seq: u64) -> Vec<&Commit> {
        self.commits
            .iter()
            .filter(|c| c.seq >= start_seq && c.seq < end_seq)
            .collect()
    }

    /// Get the most recent N commits.
    pub fn get_recent(&self, count: usize) -> Vec<&Commit> {
        self.commits.iter().rev().take(count).collect()
    }

    /// Get the head commit ID (hash of the most recent commit).
    pub fn head(&self) -> CommitId {
        self.last_hash
    }

    /// Get the current sequence number (of the last commit).
    pub fn current_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }

    /// Get the number of commits.
    pub fn len(&self) -> usize {
        self.commits.len()
    }

    /// Check if the log is empty (should never be - always has genesis).
    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    /// Verify hash chain integrity.
    ///
    /// Returns true if the chain is intact.
    pub fn verify_integrity(&self) -> bool {
        if self.commits.is_empty() {
            return true;
        }

        let mut expected_prev = [0u8; 32]; // Genesis has zero prev

        for commit in &self.commits {
            if commit.prev_commit != expected_prev {
                return false;
            }
            let computed_hash = Self::compute_hash(commit);
            if computed_hash != commit.id {
                return false;
            }
            expected_prev = commit.id;
        }

        expected_prev == self.last_hash
    }

    /// Trim old commits if exceeding max capacity.
    fn trim_if_needed(&mut self) {
        if self.commits.len() > MAX_COMMITLOG_ENTRIES {
            let drain_count = self.commits.len() - MAX_COMMITLOG_ENTRIES;
            self.commits.drain(0..drain_count);
        }
    }
}

impl Default for CommitLog {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commitlog_creation() {
        let log = CommitLog::new(0);
        assert_eq!(log.len(), 1); // Genesis
        assert_eq!(log.current_seq(), 0);
        assert!(matches!(log.commits()[0].commit_type, CommitType::Genesis));
    }

    #[test]
    fn test_commitlog_append() {
        let mut log = CommitLog::new(0);

        let id1 = log.append(
            CommitType::ProcessCreated {
                pid: 1,
                parent: 0,
                name: String::from("init"),
            },
            None,
            1000,
        );

        assert_eq!(log.len(), 2);
        assert_eq!(log.current_seq(), 1);
        assert_ne!(id1, [0u8; 32]);

        let id2 = log.append(CommitType::EndpointCreated { id: 1, owner: 1 }, None, 2000);

        assert_eq!(log.len(), 3);
        assert_eq!(log.current_seq(), 2);
        assert_ne!(id2, id1);
    }

    #[test]
    fn test_commitlog_integrity() {
        let mut log = CommitLog::new(0);

        for i in 1..=10 {
            log.append(
                CommitType::ProcessCreated {
                    pid: i,
                    parent: 0,
                    name: String::from("test"),
                },
                None,
                i * 1000,
            );
        }

        assert!(log.verify_integrity());
    }

    #[test]
    fn test_commitlog_get_range() {
        let mut log = CommitLog::new(0);

        for i in 1..=5 {
            log.append(
                CommitType::EndpointCreated { id: i, owner: 1 },
                None,
                i * 1000,
            );
        }

        // Range [1, 4) should get commits 1, 2, 3
        let range = log.get_range(1, 4);
        assert_eq!(range.len(), 3);
        assert_eq!(range[0].seq, 1);
        assert_eq!(range[2].seq, 3);
    }

    #[test]
    fn test_commitlog_get_recent() {
        let mut log = CommitLog::new(0);

        for i in 1..=10 {
            log.append(
                CommitType::EndpointCreated { id: i, owner: 1 },
                None,
                i * 1000,
            );
        }

        let recent = log.get_recent(3);
        assert_eq!(recent.len(), 3);
        // Most recent first
        assert_eq!(recent[0].seq, 10);
        assert_eq!(recent[1].seq, 9);
        assert_eq!(recent[2].seq, 8);
    }

    #[test]
    fn test_commitlog_hash_determinism() {
        // Same commits should produce same hashes
        let mut log1 = CommitLog::new(1000);
        let mut log2 = CommitLog::new(1000);

        let commits = vec![
            CommitType::ProcessCreated {
                pid: 1,
                parent: 0,
                name: String::from("init"),
            },
            CommitType::EndpointCreated { id: 1, owner: 1 },
            CommitType::CapInserted {
                pid: 1,
                slot: 0,
                cap_id: 1,
                object_type: 1,
                object_id: 1,
                perms: 0x07,
            },
        ];

        for (i, ct) in commits.into_iter().enumerate() {
            let id1 = log1.append(ct.clone(), None, (i + 1) as u64 * 1000);
            let id2 = log2.append(ct, None, (i + 1) as u64 * 1000);
            assert_eq!(id1, id2);
        }

        assert_eq!(log1.head(), log2.head());
    }
}
