//! VFS IPC Client Library
//!
//! Provides a high-level API for processes to interact with the VFS service
//! via IPC messages. This replaces direct VFS syscalls (which are now deprecated).
//!
//! # Example
//!
//! ```ignore
//! use zos_vfs::client::VfsClient;
//!
//! // Connect to VFS service
//! let vfs = VfsClient::connect()?;
//!
//! // Create a directory
//! vfs.mkdir("/home/user/Documents")?;
//!
//! // Write a file
//! vfs.write_file("/home/user/file.txt", b"Hello, World!")?;
//!
//! // Read a file
//! let content = vfs.read_file("/home/user/file.txt")?;
//! ```

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::VfsError;
use crate::ipc::{
    vfs_msg, ExistsRequest, ExistsResponse, MkdirRequest, MkdirResponse, ReadFileRequest,
    ReadFileResponse, ReaddirRequest, ReaddirResponse, RmdirRequest, RmdirResponse, StatRequest,
    StatResponse, UnlinkRequest, UnlinkResponse, WriteFileRequest, WriteFileResponse,
};
use crate::types::{DirEntry, Inode};

/// Default capability slot for VFS service endpoint
/// This is assigned by init when the process starts
pub const VFS_ENDPOINT_SLOT: u32 = 3;

/// Dedicated slot for receiving VFS responses
/// This is a separate endpoint to prevent race conditions where the VFS client's
/// blocking receive could consume other IPC messages on the general input endpoint.
/// The supervisor routes VFS responses to this slot via Init.
pub const VFS_RESPONSE_SLOT: u32 = 4;

/// VFS client for sending IPC messages to VFS Service
pub struct VfsClient {
    /// Capability slot for VFS service endpoint
    #[allow(dead_code)]  // Used in WASM target
    vfs_endpoint: u32,
}

impl Default for VfsClient {
    fn default() -> Self {
        Self::new()
    }
}

impl VfsClient {
    /// Create a new VFS client with the default endpoint slot.
    pub fn new() -> Self {
        Self {
            vfs_endpoint: VFS_ENDPOINT_SLOT,
        }
    }

    /// Create a VFS client with a custom endpoint slot.
    pub fn with_endpoint(endpoint_slot: u32) -> Self {
        Self {
            vfs_endpoint: endpoint_slot,
        }
    }

    /// Discover VFS service endpoint from init.
    ///
    /// This sends a lookup request to init and waits for a response
    /// with the VFS service endpoint capability.
    #[cfg(target_arch = "wasm32")]
    pub fn connect() -> Result<Self, VfsError> {
        use zos_process::{
            receive_blocking, send, INIT_ENDPOINT_SLOT, MSG_LOOKUP_RESPONSE, MSG_LOOKUP_SERVICE,
        };

        // Send lookup request to init
        let service_name = "vfs";
        let name_bytes = service_name.as_bytes();
        let mut data = Vec::with_capacity(1 + name_bytes.len());
        data.push(name_bytes.len() as u8);
        data.extend_from_slice(name_bytes);

        send(INIT_ENDPOINT_SLOT, MSG_LOOKUP_SERVICE, &data)
            .map_err(|e| VfsError::StorageError(alloc::format!("Lookup send failed: {}", e)))?;

        // Wait for response
        let response = receive_blocking(INIT_ENDPOINT_SLOT);
        if response.tag != MSG_LOOKUP_RESPONSE {
            return Err(VfsError::StorageError(String::from(
                "Unexpected response tag",
            )));
        }

        // Parse response: [found: u8, endpoint_id_low: u32, endpoint_id_high: u32]
        if response.data.is_empty() || response.data[0] == 0 {
            return Err(VfsError::StorageError(String::from(
                "VFS service not found",
            )));
        }

        // For now, use the default slot since init grants it at spawn
        Ok(Self::new())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn connect() -> Result<Self, VfsError> {
        Ok(Self::new())
    }

    /// Create a directory.
    ///
    /// # Arguments
    /// - `path`: Path to the directory to create
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(VfsError)` on failure
    pub fn mkdir(&self, path: &str) -> Result<(), VfsError> {
        self.mkdir_with_options(path, false)
    }

    /// Create a directory with options.
    ///
    /// # Arguments
    /// - `path`: Path to the directory to create
    /// - `create_parents`: If true, create parent directories as needed
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(VfsError)` on failure
    pub fn mkdir_with_options(&self, path: &str, create_parents: bool) -> Result<(), VfsError> {
        let request = MkdirRequest {
            path: path.to_string(),
            create_parents,
        };
        let response: MkdirResponse = self.call(vfs_msg::MSG_VFS_MKDIR, &request)?;
        response.result
    }

    /// Create all directories in a path (like `mkdir -p`).
    ///
    /// # Arguments
    /// - `path`: Path to create, including all parent directories
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(VfsError)` on failure
    pub fn mkdir_all(&self, path: &str) -> Result<(), VfsError> {
        self.mkdir_with_options(path, true)
    }

    /// Remove a directory.
    ///
    /// # Arguments
    /// - `path`: Path to the directory to remove
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(VfsError)` on failure (e.g., not empty)
    pub fn rmdir(&self, path: &str) -> Result<(), VfsError> {
        self.rmdir_with_options(path, false)
    }

    /// Remove a directory with options.
    ///
    /// # Arguments
    /// - `path`: Path to the directory to remove
    /// - `recursive`: If true, remove contents recursively
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(VfsError)` on failure
    pub fn rmdir_with_options(&self, path: &str, recursive: bool) -> Result<(), VfsError> {
        let request = RmdirRequest {
            path: path.to_string(),
            recursive,
        };
        let response: RmdirResponse = self.call(vfs_msg::MSG_VFS_RMDIR, &request)?;
        response.result
    }

    /// Remove a directory and all its contents.
    ///
    /// # Arguments
    /// - `path`: Path to remove recursively
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(VfsError)` on failure
    pub fn rmdir_all(&self, path: &str) -> Result<(), VfsError> {
        self.rmdir_with_options(path, true)
    }

    /// Read directory contents.
    ///
    /// # Arguments
    /// - `path`: Path to the directory
    ///
    /// # Returns
    /// - `Ok(Vec<DirEntry>)` on success
    /// - `Err(VfsError)` on failure
    pub fn readdir(&self, path: &str) -> Result<Vec<DirEntry>, VfsError> {
        let request = ReaddirRequest {
            path: path.to_string(),
        };
        let response: ReaddirResponse = self.call(vfs_msg::MSG_VFS_READDIR, &request)?;
        response.result
    }

    /// Write a file.
    ///
    /// # Arguments
    /// - `path`: Path to the file
    /// - `content`: File content to write
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(VfsError)` on failure
    pub fn write_file(&self, path: &str, content: &[u8]) -> Result<(), VfsError> {
        self.write_file_with_options(path, content, false)
    }

    /// Write a file with options.
    ///
    /// # Arguments
    /// - `path`: Path to the file
    /// - `content`: File content to write
    /// - `encrypt`: If true, encrypt the file content
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(VfsError)` on failure
    pub fn write_file_with_options(
        &self,
        path: &str,
        content: &[u8],
        encrypt: bool,
    ) -> Result<(), VfsError> {
        let request = WriteFileRequest {
            path: path.to_string(),
            content: content.to_vec(),
            encrypt,
        };
        let response: WriteFileResponse = self.call(vfs_msg::MSG_VFS_WRITE, &request)?;
        response.result
    }

    /// Read a file.
    ///
    /// # Arguments
    /// - `path`: Path to the file
    ///
    /// # Returns
    /// - `Ok(Vec<u8>)` with file contents on success
    /// - `Err(VfsError)` on failure
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        self.read_file_with_options(path, None, None)
    }

    /// Read a file with options.
    ///
    /// # Arguments
    /// - `path`: Path to the file
    /// - `offset`: Byte offset to start reading from
    /// - `length`: Number of bytes to read
    ///
    /// # Returns
    /// - `Ok(Vec<u8>)` with file contents on success
    /// - `Err(VfsError)` on failure
    pub fn read_file_with_options(
        &self,
        path: &str,
        offset: Option<u64>,
        length: Option<u64>,
    ) -> Result<Vec<u8>, VfsError> {
        let request = ReadFileRequest {
            path: path.to_string(),
            offset,
            length,
        };
        let response: ReadFileResponse = self.call(vfs_msg::MSG_VFS_READ, &request)?;
        response.result
    }

    /// Delete a file.
    ///
    /// # Arguments
    /// - `path`: Path to the file to delete
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(VfsError)` on failure
    pub fn unlink(&self, path: &str) -> Result<(), VfsError> {
        let request = UnlinkRequest {
            path: path.to_string(),
        };
        let response: UnlinkResponse = self.call(vfs_msg::MSG_VFS_UNLINK, &request)?;
        response.result
    }

    /// Alias for unlink - delete a file.
    pub fn delete(&self, path: &str) -> Result<(), VfsError> {
        self.unlink(path)
    }

    /// Get file/directory metadata.
    ///
    /// # Arguments
    /// - `path`: Path to stat
    ///
    /// # Returns
    /// - `Ok(Inode)` with metadata on success
    /// - `Err(VfsError)` on failure
    pub fn stat(&self, path: &str) -> Result<Inode, VfsError> {
        let request = StatRequest {
            path: path.to_string(),
        };
        let response: StatResponse = self.call(vfs_msg::MSG_VFS_STAT, &request)?;
        response.result
    }

    /// Check if a path exists.
    ///
    /// # Arguments
    /// - `path`: Path to check
    ///
    /// # Returns
    /// - `Ok(true)` if path exists
    /// - `Ok(false)` if path does not exist
    /// - `Err(VfsError)` on error
    pub fn exists(&self, path: &str) -> Result<bool, VfsError> {
        let request = ExistsRequest {
            path: path.to_string(),
        };
        let response: ExistsResponse = self.call(vfs_msg::MSG_VFS_EXISTS, &request)?;
        Ok(response.exists)
    }

    /// Check if path is a directory.
    pub fn is_directory(&self, path: &str) -> Result<bool, VfsError> {
        match self.stat(path) {
            Ok(inode) => Ok(inode.is_directory()),
            Err(VfsError::NotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Check if path is a file.
    pub fn is_file(&self, path: &str) -> Result<bool, VfsError> {
        match self.stat(path) {
            Ok(inode) => Ok(inode.is_file()),
            Err(VfsError::NotFound) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// Internal: Send IPC request and receive response.
    #[cfg(target_arch = "wasm32")]
    fn call<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        tag: u32,
        request: &Req,
    ) -> Result<Resp, VfsError> {
        use zos_process::{debug, receive_blocking, send};

        // VFS protocol: response tag = request tag + 1
        let expected_response_tag = tag + 1;

        // Serialize request
        let data = serde_json::to_vec(request)
            .map_err(|e| VfsError::StorageError(alloc::format!("Serialize error: {}", e)))?;

        // Send request to VFS service via our capability slot
        send(self.vfs_endpoint, tag, &data)
            .map_err(|e| VfsError::StorageError(alloc::format!("Send error: {}", e)))?;

        // Wait for response on dedicated VFS response endpoint (slot 4)
        // This uses a separate endpoint from the general input slot (slot 1) to prevent
        // race conditions where blocking here could consume other IPC messages.
        // The supervisor routes VFS responses to this slot via Init.
        loop {
            let response = receive_blocking(VFS_RESPONSE_SLOT);

            if response.tag == expected_response_tag {
                // This is our VFS response - deserialize and return
                let resp: Resp = serde_json::from_slice(&response.data)
                    .map_err(|e| VfsError::StorageError(alloc::format!("Deserialize error: {}", e)))?;
                return Ok(resp);
            }

            // Non-matching message on VFS response slot - this shouldn't happen
            // with a dedicated slot, but log it just in case
            debug(&alloc::format!(
                "[VFS] Unexpected message on VFS response slot (tag=0x{:04X}, expected=0x{:04X}, from_pid={})",
                response.tag,
                expected_response_tag,
                response.from_pid
            ));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn call<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        _tag: u32,
        _request: &Req,
    ) -> Result<Resp, VfsError> {
        Err(VfsError::StorageError(String::from(
            "VFS IPC not available outside WASM",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = VfsClient::new();
        assert_eq!(client.vfs_endpoint, VFS_ENDPOINT_SLOT);

        let client2 = VfsClient::with_endpoint(10);
        assert_eq!(client2.vfs_endpoint, 10);
    }
}
