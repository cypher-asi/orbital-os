//! Zero OS System Services
//!
//! This crate provides system services for Zero OS:
//!
//! - **Identity Service**: User identity management, neural keys, machine keys
//! - **VFS Service**: Virtual filesystem operations
//! - **Time Service**: System time and timezone management
//! - **Network Service**: Network connectivity and operations
//! - **Permission Service**: Permission management for apps
//!
//! These services run as background processes in Zero OS and provide
//! core functionality that apps depend on.
//!
//! # Architecture
//!
//! Each service is implemented in the `services` module with:
//! - Service implementation (`ZeroApp` trait impl)
//! - Handlers, state types, and response helpers
//!
//! Binary entry points are thin wrappers in `src/bin/` that just invoke
//! the `app_main!` macro with the service type.

extern crate alloc;

pub mod manifests;
pub mod services;

#[cfg(test)]
pub mod test_utils;

// Re-export common dependencies for service implementations
pub use zos_apps::{app_main, AppContext, AppError, ControlFlow, Message, ZeroApp};
pub use zos_apps::{AppManifest, CapabilityRequest};
pub use zos_apps::syscall;
pub use zos_apps::{init, kernel, permission, pm, storage, supervisor};

// Re-export service manifests for convenience
pub use manifests::{
    IDENTITY_MANIFEST, NETWORK_MANIFEST, PERMISSION_MANIFEST,
    TIME_MANIFEST, VFS_MANIFEST, KEYSTORE_MANIFEST,
};

// Re-export service types for convenience
pub use services::{
    IdentityService, NetworkService, PermissionService, TimeService, VfsService,
};
