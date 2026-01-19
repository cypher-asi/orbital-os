//! Orbital OS Axiom Layer
//!
//! The Axiom layer provides:
//! - **SysLog**: Audit trail of all syscalls (request + response)
//! - **CommitLog**: Deterministic state mutations for replay
//! - **AxiomGateway**: Entry point for all syscalls
//!
//! # Core Guarantee
//!
//! > Same CommitLog always produces same state.
//!
//! This is the foundation of Orbital OS's deterministic replay capability.

#![no_std]
extern crate alloc;

pub mod commitlog;
pub mod gateway;
pub mod replay;
pub mod syslog;
pub mod types;

// Re-export main types
pub use commitlog::{Commit, CommitLog, CommitType};
pub use gateway::AxiomGateway;
pub use replay::{
    apply_commit, replay, replay_and_verify, ReplayError, ReplayResult, Replayable, StateHasher,
};
pub use syslog::{SysEvent, SysEventType, SysLog};
pub use types::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_axiom_gateway_creation() {
        let gateway = AxiomGateway::new(0);
        assert_eq!(gateway.syslog().len(), 0);
        assert_eq!(gateway.commitlog().len(), 1); // Genesis commit
    }

    #[test]
    fn test_syslog_records_request_and_response() {
        let mut gateway = AxiomGateway::new(0);

        gateway.syscall(1, 0x01, [0, 0, 0, 0], 1000, |_, _| (0, alloc::vec![]));

        let events = gateway.syslog().events();
        assert_eq!(events.len(), 2); // Request + Response
    }

    #[test]
    fn test_commitlog_starts_with_genesis() {
        let gateway = AxiomGateway::new(0);

        assert_eq!(gateway.commitlog().current_seq(), 0);
        assert_eq!(gateway.commitlog().len(), 1);

        let commits = gateway.commitlog().commits();
        assert!(matches!(commits[0].commit_type, CommitType::Genesis));
    }

    #[test]
    fn test_commitlog_records_mutations() {
        let mut gateway = AxiomGateway::new(0);

        gateway.syscall(1, 0x35, [0, 0, 0, 0], 1000, |_, _| {
            (
                0,
                alloc::vec![CommitType::EndpointCreated { id: 1, owner: 1 }],
            )
        });

        assert_eq!(gateway.commitlog().current_seq(), 1);
        assert_eq!(gateway.commitlog().len(), 2); // Genesis + EndpointCreated
    }

    #[test]
    fn test_commitlog_hash_chain_integrity() {
        let mut gateway = AxiomGateway::new(0);

        // Add several commits
        for i in 1..=5 {
            gateway.syscall(1, 0x11, [0, 0, 0, 0], i * 1000, |_, _| {
                (
                    0,
                    alloc::vec![CommitType::EndpointCreated { id: i, owner: 1 }],
                )
            });
        }

        assert!(gateway.commitlog().verify_integrity());
    }

    #[test]
    fn test_syslog_event_ids_are_monotonic() {
        let mut gateway = AxiomGateway::new(0);

        for i in 0..5 {
            gateway.syscall(1, 0x01, [i, 0, 0, 0], i as u64 * 1000, |_, _| {
                (0, alloc::vec![])
            });
        }

        let events = gateway.syslog().events();
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.id, i as u64);
        }
    }
}
