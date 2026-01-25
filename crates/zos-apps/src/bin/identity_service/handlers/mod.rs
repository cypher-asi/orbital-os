//! Identity Service message handlers
//!
//! Organized by functional domain:
//! - `keys`: Neural key and machine key operations
//! - `session`: ZID login/enrollment flows
//! - `credentials`: Credential management
//! - `preferences`: Identity preferences (default key scheme, etc.)

pub mod credentials;
pub mod keys;
pub mod preferences;
pub mod session;
