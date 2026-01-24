//! IPC (Inter-Process Communication) types and structures
//!
//! This module contains types for IPC messaging:
//! - Messages and transferred capabilities
//! - Endpoints and their metrics
//! - IPC traffic monitoring

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use crate::capability::Capability;
use crate::types::{EndpointId, EndpointMetrics, ProcessId};
use zos_axiom::CapSlot;

/// Maximum capabilities per IPC message
pub const MAX_CAPS_PER_MESSAGE: usize = 8;

/// Maximum message payload size in bytes
pub const MAX_MESSAGE_SIZE: usize = 4096;

/// A capability being transferred via IPC.
///
/// When a capability is transferred, it is moved from the sender's CSpace
/// to the receiver's CSpace. The sender loses the capability.
#[derive(Clone, Debug)]
pub struct TransferredCap {
    /// The capability being transferred
    pub capability: Capability,
    /// Hint for receiver slot placement (None = kernel assigns)
    pub receiver_slot: Option<CapSlot>,
}

/// IPC message
#[derive(Clone, Debug)]
pub struct Message {
    /// Sender process
    pub from: ProcessId,
    /// Message tag (application-defined)
    pub tag: u32,
    /// Message payload
    pub data: Vec<u8>,
    /// Capabilities transferred with this message
    pub transferred_caps: Vec<TransferredCap>,
}

/// IPC endpoint
pub struct Endpoint {
    /// Endpoint ID
    pub id: EndpointId,
    /// Owning process
    pub owner: ProcessId,
    /// Queue of pending messages
    pub pending_messages: VecDeque<Message>,
    /// Endpoint metrics
    pub metrics: EndpointMetrics,
}

/// Detailed info about an endpoint
#[derive(Clone, Debug)]
pub struct EndpointDetail {
    pub id: EndpointId,
    pub owner: ProcessId,
    pub queue_depth: usize,
    pub metrics: EndpointMetrics,
    pub queued_messages: Vec<MessageSummary>,
}

/// Summary of a queued message
#[derive(Clone, Debug)]
pub struct MessageSummary {
    pub from: ProcessId,
    pub tag: u32,
    pub size: usize,
}

/// Summary info about an endpoint
#[derive(Clone, Debug)]
pub struct EndpointInfo {
    pub id: EndpointId,
    pub owner: ProcessId,
    pub queue_depth: usize,
}
