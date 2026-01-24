//! Kernel error types
//!
//! This module contains error types used throughout the kernel.

use zos_hal::HalError;

/// Kernel errors
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelError {
    /// Process not found
    ProcessNotFound,
    /// Endpoint not found
    EndpointNotFound,
    /// Invalid capability (not found or wrong type)
    InvalidCapability,
    /// Permission denied
    PermissionDenied,
    /// No message available (would block)
    WouldBlock,
    /// HAL error
    Hal(HalError),
}

impl From<HalError> for KernelError {
    fn from(e: HalError) -> Self {
        KernelError::Hal(e)
    }
}
