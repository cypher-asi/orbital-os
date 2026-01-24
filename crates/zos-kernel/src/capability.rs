//! Capability-based access control
//!
//! This module re-exports capability types from zos-axiom.
//!
//! Per Invariant 10, all capability verification flows through Axiom's
//! `axiom_check` function. This module exists only to maintain backwards
//! compatibility for kernel code.

// Re-export all capability types from zos-axiom
pub use zos_axiom::{axiom_check, AxiomError, Capability, CapabilitySpace, Permissions};
