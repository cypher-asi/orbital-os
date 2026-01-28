//! Core type definitions for the desktop compositor
//!
//! This module centralizes type aliases used throughout the crate
//! for consistency and discoverability.

/// Unique window identifier
///
/// Windows are identified by a monotonically increasing 64-bit integer.
/// Window IDs are globally unique within a `DesktopEngine` instance.
pub type WindowId = u64;

/// Unique desktop identifier
///
/// Desktops are identified by a monotonically increasing 32-bit integer.
/// Desktop IDs are globally unique within a `DesktopEngine` instance.
pub type DesktopId = u32;
