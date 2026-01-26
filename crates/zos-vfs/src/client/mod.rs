//! VFS and Keystore IPC clients

pub mod async_ops;
pub mod keystore_async;
mod blocking;

pub use blocking::{VfsClient, VFS_ENDPOINT_SLOT, VFS_RESPONSE_SLOT};
pub use keystore_async::KEYSTORE_ENDPOINT_SLOT;
