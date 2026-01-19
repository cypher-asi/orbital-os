//! System Event Log (SysLog)
//!
//! Records all syscalls (request + response) for audit trail.
//! This is separate from CommitLog - SysLog is for auditing,
//! CommitLog is for deterministic replay.

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

use crate::types::{EventId, ProcessId};

/// A system event (syscall request or response).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SysEvent {
    /// Unique event ID (monotonic)
    pub id: EventId,
    /// Process that made the syscall
    pub sender: ProcessId,
    /// Timestamp (nanos since boot)
    pub timestamp: u64,
    /// Event type (request or response)
    pub event_type: SysEventType,
}

/// Type of system event.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SysEventType {
    /// Syscall request from a process
    Request {
        /// Syscall number
        syscall_num: u32,
        /// Syscall arguments (up to 4)
        args: [u32; 4],
    },
    /// Syscall response to a process
    Response {
        /// ID of the request this responds to
        request_id: EventId,
        /// Syscall result (negative = error)
        result: i64,
    },
}

/// Maximum number of events to keep in memory
const MAX_SYSLOG_EVENTS: usize = 10000;

/// System event log for auditing.
///
/// Records every syscall (request and response) for audit purposes.
/// Events are append-only with monotonic IDs.
pub struct SysLog {
    /// Event entries (append-only)
    events: Vec<SysEvent>,
    /// Next event ID to assign
    next_id: EventId,
}

impl SysLog {
    /// Create a new empty SysLog.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            next_id: 0,
        }
    }

    /// Log a syscall request.
    ///
    /// Returns the event ID for correlating with the response.
    pub fn log_request(
        &mut self,
        sender: ProcessId,
        syscall_num: u32,
        args: [u32; 4],
        timestamp: u64,
    ) -> EventId {
        let id = self.next_id;
        self.next_id += 1;

        self.events.push(SysEvent {
            id,
            sender,
            timestamp,
            event_type: SysEventType::Request { syscall_num, args },
        });

        self.trim_if_needed();
        id
    }

    /// Log a syscall response.
    pub fn log_response(
        &mut self,
        sender: ProcessId,
        request_id: EventId,
        result: i64,
        timestamp: u64,
    ) {
        let id = self.next_id;
        self.next_id += 1;

        self.events.push(SysEvent {
            id,
            sender,
            timestamp,
            event_type: SysEventType::Response { request_id, result },
        });

        self.trim_if_needed();
    }

    /// Get all events.
    pub fn events(&self) -> &[SysEvent] {
        &self.events
    }

    /// Get events in a sequence range.
    pub fn get_range(&self, start_id: EventId, end_id: EventId) -> Vec<&SysEvent> {
        self.events
            .iter()
            .filter(|e| e.id >= start_id && e.id < end_id)
            .collect()
    }

    /// Get the most recent N events.
    pub fn get_recent(&self, count: usize) -> Vec<&SysEvent> {
        self.events.iter().rev().take(count).collect()
    }

    /// Get the number of events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get the next event ID.
    pub fn next_id(&self) -> EventId {
        self.next_id
    }

    /// Trim old events if exceeding max capacity.
    fn trim_if_needed(&mut self) {
        if self.events.len() > MAX_SYSLOG_EVENTS {
            let drain_count = self.events.len() - MAX_SYSLOG_EVENTS;
            self.events.drain(0..drain_count);
        }
    }
}

impl Default for SysLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syslog_creation() {
        let log = SysLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert_eq!(log.next_id(), 0);
    }

    #[test]
    fn test_syslog_request_response() {
        let mut log = SysLog::new();

        // Log request
        let req_id = log.log_request(1, 0x01, [10, 20, 30, 40], 1000);
        assert_eq!(req_id, 0);

        // Log response
        log.log_response(1, req_id, 42, 1100);

        assert_eq!(log.len(), 2);

        let events = log.events();
        assert!(matches!(
            events[0].event_type,
            SysEventType::Request {
                syscall_num: 0x01,
                args: [10, 20, 30, 40]
            }
        ));
        assert!(matches!(
            events[1].event_type,
            SysEventType::Response {
                request_id: 0,
                result: 42
            }
        ));
    }

    #[test]
    fn test_syslog_get_recent() {
        let mut log = SysLog::new();

        for i in 0..10 {
            log.log_request(1, i, [0, 0, 0, 0], i as u64 * 100);
        }

        let recent = log.get_recent(3);
        assert_eq!(recent.len(), 3);
        // Most recent first
        assert_eq!(recent[0].id, 9);
        assert_eq!(recent[1].id, 8);
        assert_eq!(recent[2].id, 7);
    }

    #[test]
    fn test_syslog_get_range() {
        let mut log = SysLog::new();

        for i in 0..10 {
            log.log_request(1, i, [0, 0, 0, 0], i as u64 * 100);
        }

        let range = log.get_range(3, 7);
        assert_eq!(range.len(), 4);
        assert_eq!(range[0].id, 3);
        assert_eq!(range[3].id, 6);
    }
}
