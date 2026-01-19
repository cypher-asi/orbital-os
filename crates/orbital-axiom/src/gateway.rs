//! Axiom Gateway
//!
//! Entry point for all syscalls. The gateway:
//! 1. Logs the syscall request to SysLog
//! 2. Executes the kernel operation
//! 3. Appends any resulting commits to CommitLog
//! 4. Logs the syscall response to SysLog
//!
//! This ensures all syscalls are audited and all state mutations
//! are recorded for deterministic replay.

use alloc::vec::Vec;

use crate::commitlog::{CommitLog, CommitType};
use crate::syslog::SysLog;
use crate::types::{CommitId, ProcessId};

/// Axiom gateway: Entry point for all syscalls.
///
/// All syscalls flow through the gateway, which:
/// - Records requests and responses to SysLog (audit)
/// - Records state mutations to CommitLog (replay)
pub struct AxiomGateway {
    /// Syscall audit log
    syslog: SysLog,
    /// State mutation log
    commitlog: CommitLog,
}

impl AxiomGateway {
    /// Create a new Axiom gateway.
    ///
    /// # Arguments
    /// - `timestamp`: Boot timestamp (nanos)
    pub fn new(timestamp: u64) -> Self {
        Self {
            syslog: SysLog::new(),
            commitlog: CommitLog::new(timestamp),
        }
    }

    /// Process a syscall through Axiom.
    ///
    /// This is the main entry point for syscall processing:
    /// 1. Log syscall request to SysLog
    /// 2. Execute kernel function (provided by caller)
    /// 3. Append commits to CommitLog
    /// 4. Log syscall response to SysLog
    ///
    /// # Arguments
    /// - `sender`: Process ID making the syscall
    /// - `syscall_num`: Syscall number
    /// - `args`: Syscall arguments (up to 4)
    /// - `timestamp`: Current timestamp (nanos since boot)
    /// - `kernel_fn`: Function that executes the kernel operation
    ///
    /// # Returns
    /// Tuple of (result, commit_ids) where:
    /// - `result`: The syscall result (negative = error)
    /// - `commit_ids`: IDs of any commits created
    ///
    /// # Type Parameters
    /// - `F`: Kernel function type that takes (syscall_num, args) and returns
    ///   (result, Vec<CommitType>)
    pub fn syscall<F>(
        &mut self,
        sender: ProcessId,
        syscall_num: u32,
        args: [u32; 4],
        timestamp: u64,
        mut kernel_fn: F,
    ) -> (i64, Vec<CommitId>)
    where
        F: FnMut(u32, [u32; 4]) -> (i64, Vec<CommitType>),
    {
        // 1. Log syscall request
        let request_id = self
            .syslog
            .log_request(sender, syscall_num, args, timestamp);

        // 2. Execute kernel operation
        let (result, commit_types) = kernel_fn(syscall_num, args);

        // 3. Append commits to CommitLog
        let commit_ids: Vec<CommitId> = commit_types
            .into_iter()
            .map(|ct| self.commitlog.append(ct, Some(request_id), timestamp))
            .collect();

        // 4. Log syscall response
        self.syslog
            .log_response(sender, request_id, result, timestamp);

        (result, commit_ids)
    }

    /// Get the SysLog (for inspection/auditing).
    pub fn syslog(&self) -> &SysLog {
        &self.syslog
    }

    /// Get mutable reference to SysLog (for logging syscalls).
    pub fn syslog_mut(&mut self) -> &mut SysLog {
        &mut self.syslog
    }

    /// Get the CommitLog (for replay/inspection).
    pub fn commitlog(&self) -> &CommitLog {
        &self.commitlog
    }

    /// Get mutable reference to CommitLog.
    ///
    /// Use with care - direct mutations bypass syscall logging.
    pub fn commitlog_mut(&mut self) -> &mut CommitLog {
        &mut self.commitlog
    }

    /// Append a commit directly (bypassing SysLog).
    ///
    /// Use for internal kernel operations that don't originate
    /// from a syscall (e.g., timer-driven cleanup).
    pub fn append_internal_commit(&mut self, commit_type: CommitType, timestamp: u64) -> CommitId {
        self.commitlog.append(commit_type, None, timestamp)
    }

    /// Verify integrity of both logs.
    pub fn verify_integrity(&self) -> bool {
        self.commitlog.verify_integrity()
    }

    /// Get current state for serialization.
    pub fn state_summary(&self) -> GatewayState {
        GatewayState {
            syslog_len: self.syslog.len(),
            syslog_next_id: self.syslog.next_id(),
            commitlog_len: self.commitlog.len(),
            commitlog_seq: self.commitlog.current_seq(),
            commitlog_head: self.commitlog.head(),
        }
    }
}

/// Summary of gateway state (for debugging/monitoring).
#[derive(Clone, Debug)]
pub struct GatewayState {
    /// Number of events in SysLog
    pub syslog_len: usize,
    /// Next event ID in SysLog
    pub syslog_next_id: u64,
    /// Number of commits in CommitLog
    pub commitlog_len: usize,
    /// Current sequence number in CommitLog
    pub commitlog_seq: u64,
    /// Head commit hash
    pub commitlog_head: CommitId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    #[test]
    fn test_gateway_creation() {
        let gateway = AxiomGateway::new(0);
        assert_eq!(gateway.syslog().len(), 0);
        assert_eq!(gateway.commitlog().len(), 1); // Genesis
    }

    #[test]
    fn test_gateway_syscall_no_commits() {
        let mut gateway = AxiomGateway::new(0);

        let (result, commits) =
            gateway.syscall(1, 0x01, [0, 0, 0, 0], 1000, |_, _| (42, Vec::new()));

        assert_eq!(result, 42);
        assert!(commits.is_empty());
        assert_eq!(gateway.syslog().len(), 2); // Request + Response
        assert_eq!(gateway.commitlog().len(), 1); // Still just Genesis
    }

    #[test]
    fn test_gateway_syscall_with_commits() {
        let mut gateway = AxiomGateway::new(0);

        let (result, commits) = gateway.syscall(1, 0x11, [0, 0, 0, 0], 1000, |_, _| {
            (
                0,
                alloc::vec![
                    CommitType::ProcessCreated {
                        pid: 1,
                        parent: 0,
                        name: String::from("init"),
                    },
                    CommitType::EndpointCreated { id: 1, owner: 1 },
                ],
            )
        });

        assert_eq!(result, 0);
        assert_eq!(commits.len(), 2);
        assert_eq!(gateway.syslog().len(), 2);
        assert_eq!(gateway.commitlog().len(), 3); // Genesis + 2
    }

    #[test]
    fn test_gateway_multiple_syscalls() {
        let mut gateway = AxiomGateway::new(0);

        for i in 1..=5 {
            gateway.syscall(1, 0x11, [i, 0, 0, 0], i as u64 * 1000, |_, _| {
                (
                    0,
                    alloc::vec![CommitType::EndpointCreated {
                        id: i as u64,
                        owner: 1
                    }],
                )
            });
        }

        assert_eq!(gateway.syslog().len(), 10); // 5 requests + 5 responses
        assert_eq!(gateway.commitlog().len(), 6); // Genesis + 5 endpoints
        assert!(gateway.verify_integrity());
    }

    #[test]
    fn test_gateway_state_summary() {
        let mut gateway = AxiomGateway::new(1000);

        gateway.syscall(1, 0x01, [0, 0, 0, 0], 2000, |_, _| {
            (
                0,
                alloc::vec![CommitType::EndpointCreated { id: 1, owner: 1 }],
            )
        });

        let state = gateway.state_summary();
        assert_eq!(state.syslog_len, 2);
        assert_eq!(state.syslog_next_id, 2);
        assert_eq!(state.commitlog_len, 2);
        assert_eq!(state.commitlog_seq, 1);
    }

    #[test]
    fn test_gateway_internal_commit() {
        let mut gateway = AxiomGateway::new(0);

        let commit_id =
            gateway.append_internal_commit(CommitType::ProcessExited { pid: 1, code: 0 }, 1000);

        assert_ne!(commit_id, [0u8; 32]);
        assert_eq!(gateway.syslog().len(), 0); // No syscall logged
        assert_eq!(gateway.commitlog().len(), 2); // Genesis + exit
    }
}
