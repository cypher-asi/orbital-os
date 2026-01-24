//! Identity Service IPC Client Library
//!
//! Provides a high-level API for processes to interact with the Identity Service
//! via capability-mediated IPC. This follows the microkernel principle that all
//! inter-process communication goes through proper capability channels.
//!
//! # Architecture
//!
//! The client uses two capability slots:
//! - IDENTITY_ENDPOINT_SLOT: EndpointCap to Identity Service (granted at spawn)
//! - INPUT_ENDPOINT_SLOT: Process's own input endpoint (for receiving replies)
//!
//! When making a request:
//! 1. Client sends request to Identity Service via IDENTITY_ENDPOINT_SLOT
//! 2. Client transfers its input endpoint capability with the request
//! 3. Identity Service sends response via the transferred capability
//! 4. Client receives response on its input endpoint
//!
//! This is proper capability-mediated IPC - no supervisor routing needed.
//!
//! # Example
//!
//! ```ignore
//! use zos_identity::client::IdentityClient;
//!
//! let client = IdentityClient::new();
//!
//! // Generate a Neural Key
//! let result = client.generate_neural_key(user_id)?;
//! let shards = result.shards; // Backup these!
//!
//! // Get stored identity key
//! if let Some(key_store) = client.get_identity_key(user_id)? {
//!     println!("Public key: {:?}", key_store.identity_signing_public_key);
//! }
//! ```

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::KeyError;
use crate::ipc::{
    CreateMachineKeyRequest, CreateMachineKeyResponse, GenerateNeuralKeyRequest,
    GenerateNeuralKeyResponse, GetIdentityKeyRequest, GetIdentityKeyResponse,
    ListMachineKeysRequest, ListMachineKeysResponse, NeuralKeyGenerated, NeuralShard,
    RecoverNeuralKeyRequest, RecoverNeuralKeyResponse, RevokeMachineKeyRequest,
    RevokeMachineKeyResponse, RotateMachineKeyRequest, RotateMachineKeyResponse,
};
use crate::keystore::{LocalKeyStore, MachineKeyCapabilities, MachineKeyRecord};
use crate::types::UserId;
use zos_process::{identity_key, identity_machine};

/// Default capability slot for Identity Service endpoint.
/// This is assigned by the supervisor when the process spawns.
pub const IDENTITY_ENDPOINT_SLOT: u32 = 4;

/// Process's own input endpoint slot (for receiving replies).
pub const INPUT_ENDPOINT_SLOT: u32 = 1;

/// Identity Service IPC client.
///
/// Provides methods to interact with the Identity Service via proper
/// capability-mediated IPC.
pub struct IdentityClient {
    /// Capability slot for Identity Service endpoint
    #[allow(dead_code)]
    identity_endpoint: u32,
    /// Capability slot for our own input endpoint (for replies)
    #[allow(dead_code)]
    input_endpoint: u32,
}

impl Default for IdentityClient {
    fn default() -> Self {
        Self::new()
    }
}

impl IdentityClient {
    /// Create a new Identity client with the default endpoint slots.
    pub fn new() -> Self {
        Self {
            identity_endpoint: IDENTITY_ENDPOINT_SLOT,
            input_endpoint: INPUT_ENDPOINT_SLOT,
        }
    }

    /// Create an Identity client with custom endpoint slots.
    pub fn with_endpoints(identity_endpoint: u32, input_endpoint: u32) -> Self {
        Self {
            identity_endpoint,
            input_endpoint,
        }
    }

    /// Generate a new Neural Key for a user.
    ///
    /// This triggers full key generation on the Identity Service:
    /// 1. Generate 32 bytes of secure entropy
    /// 2. Derive Ed25519/X25519 keypairs
    /// 3. Split entropy into 5 Shamir shards (3-of-5 threshold)
    /// 4. Store public keys to VFS
    /// 5. Return shards + public identifiers
    ///
    /// The returned shards should be shown to the user for backup.
    /// They are ephemeral and NOT stored anywhere.
    ///
    /// # Arguments
    /// - `user_id`: The user ID to generate keys for
    ///
    /// # Returns
    /// - `Ok(NeuralKeyGenerated)` with shards and public identifiers
    /// - `Err(KeyError)` on failure
    pub fn generate_neural_key(&self, user_id: UserId) -> Result<NeuralKeyGenerated, KeyError> {
        let request = GenerateNeuralKeyRequest { user_id };
        let response: GenerateNeuralKeyResponse = self.call(
            identity_key::MSG_GENERATE_NEURAL_KEY,
            identity_key::MSG_GENERATE_NEURAL_KEY_RESPONSE,
            &request,
        )?;
        response.result
    }

    /// Recover a Neural Key from Shamir shards.
    ///
    /// Requires at least 3 of 5 shards to reconstruct the original entropy.
    ///
    /// # Arguments
    /// - `user_id`: The user ID to recover keys for
    /// - `shards`: At least 3 Shamir shards
    ///
    /// # Returns
    /// - `Ok(NeuralKeyGenerated)` with new shards and public identifiers
    /// - `Err(KeyError)` on failure
    pub fn recover_neural_key(
        &self,
        user_id: UserId,
        shards: Vec<NeuralShard>,
    ) -> Result<NeuralKeyGenerated, KeyError> {
        let request = RecoverNeuralKeyRequest { user_id, shards };
        let response: RecoverNeuralKeyResponse = self.call(
            identity_key::MSG_RECOVER_NEURAL_KEY,
            identity_key::MSG_RECOVER_NEURAL_KEY_RESPONSE,
            &request,
        )?;
        response.result
    }

    /// Get the stored identity key for a user.
    ///
    /// Returns the public identifiers if a Neural Key exists.
    ///
    /// # Arguments
    /// - `user_id`: The user ID to get keys for
    ///
    /// # Returns
    /// - `Ok(Some(LocalKeyStore))` if keys exist
    /// - `Ok(None)` if no keys are stored for this user
    /// - `Err(KeyError)` on failure
    pub fn get_identity_key(&self, user_id: UserId) -> Result<Option<LocalKeyStore>, KeyError> {
        let request = GetIdentityKeyRequest { user_id };
        let response: GetIdentityKeyResponse = self.call(
            identity_key::MSG_GET_IDENTITY_KEY,
            identity_key::MSG_GET_IDENTITY_KEY_RESPONSE,
            &request,
        )?;
        response.result
    }

    // =========================================================================
    // Machine Key Operations
    // =========================================================================

    /// Create a new machine key for a user.
    ///
    /// Creates a new machine with the given name and capabilities.
    /// The identity key must exist before creating machine keys.
    ///
    /// # Arguments
    /// - `user_id`: The user ID to create machine key for
    /// - `machine_name`: Optional human-readable name for the machine
    /// - `capabilities`: Capabilities to grant to this machine
    ///
    /// # Returns
    /// - `Ok(MachineKeyRecord)` on success
    /// - `Err(KeyError)` on failure
    pub fn create_machine_key(
        &self,
        user_id: UserId,
        machine_name: Option<String>,
        capabilities: MachineKeyCapabilities,
    ) -> Result<MachineKeyRecord, KeyError> {
        // Keys are generated by the service from entropy - no key fields needed
        let request = CreateMachineKeyRequest {
            user_id,
            machine_name,
            capabilities,
        };
        let response: CreateMachineKeyResponse = self.call(
            identity_machine::MSG_CREATE_MACHINE_KEY,
            identity_machine::MSG_CREATE_MACHINE_KEY_RESPONSE,
            &request,
        )?;
        response.result
    }

    /// List all machine keys for a user.
    ///
    /// # Arguments
    /// - `user_id`: The user ID to list machine keys for
    ///
    /// # Returns
    /// - `Ok(Vec<MachineKeyRecord>)` with all machine records
    /// - Empty vector if no machines exist
    pub fn list_machine_keys(&self, user_id: UserId) -> Result<Vec<MachineKeyRecord>, KeyError> {
        let request = ListMachineKeysRequest { user_id };
        let response: ListMachineKeysResponse = self.call(
            identity_machine::MSG_LIST_MACHINE_KEYS,
            identity_machine::MSG_LIST_MACHINE_KEYS_RESPONSE,
            &request,
        )?;
        Ok(response.machines)
    }

    /// Revoke a machine key.
    ///
    /// Permanently deletes the machine key record.
    ///
    /// # Arguments
    /// - `user_id`: The user ID owning the machine
    /// - `machine_id`: The machine ID to revoke
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(KeyError::MachineKeyNotFound)` if machine doesn't exist
    pub fn revoke_machine_key(&self, user_id: UserId, machine_id: u128) -> Result<(), KeyError> {
        let request = RevokeMachineKeyRequest {
            user_id,
            machine_id,
        };
        let response: RevokeMachineKeyResponse = self.call(
            identity_machine::MSG_REVOKE_MACHINE_KEY,
            identity_machine::MSG_REVOKE_MACHINE_KEY_RESPONSE,
            &request,
        )?;
        response.result
    }

    /// Rotate the keys for a machine.
    ///
    /// Generates new signing and encryption keys for the machine.
    ///
    /// # Arguments
    /// - `user_id`: The user ID owning the machine
    /// - `machine_id`: The machine ID to rotate
    ///
    /// # Returns
    /// - `Ok(MachineKeyRecord)` with updated keys
    /// - `Err(KeyError::MachineKeyNotFound)` if machine doesn't exist
    pub fn rotate_machine_key(
        &self,
        user_id: UserId,
        machine_id: u128,
    ) -> Result<MachineKeyRecord, KeyError> {
        // New keys are generated by the service from entropy - no key fields needed
        let request = RotateMachineKeyRequest {
            user_id,
            machine_id,
        };
        let response: RotateMachineKeyResponse = self.call(
            identity_machine::MSG_ROTATE_MACHINE_KEY,
            identity_machine::MSG_ROTATE_MACHINE_KEY_RESPONSE,
            &request,
        )?;
        response.result
    }

    /// Internal: Send IPC request and receive response.
    ///
    /// Uses send_with_caps to transfer the reply endpoint capability,
    /// enabling proper capability-mediated responses.
    #[cfg(target_arch = "wasm32")]
    fn call<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        request_tag: u32,
        response_tag: u32,
        request: &Req,
    ) -> Result<Resp, KeyError> {
        use zos_process::{receive_blocking, send_with_caps};

        // Serialize request
        let data = serde_json::to_vec(request)
            .map_err(|e| KeyError::StorageError(alloc::format!("Serialize error: {}", e)))?;

        // Send request to Identity Service, transferring our input endpoint cap
        // so the service can respond directly to us
        send_with_caps(
            self.identity_endpoint,
            request_tag,
            &data,
            &[self.input_endpoint],
        )
        .map_err(|e| KeyError::StorageError(alloc::format!("Send error: {}", e)))?;

        // Wait for response on our input endpoint
        // The identity service will send the response using the transferred cap
        loop {
            let msg = receive_blocking(self.input_endpoint);
            
            // Check if this is the response we're waiting for
            if msg.tag == response_tag {
                // Deserialize response
                let resp: Resp = serde_json::from_slice(&msg.data)
                    .map_err(|e| KeyError::StorageError(alloc::format!("Deserialize error: {}", e)))?;
                return Ok(resp);
            }
            
            // Not our response, continue waiting
            // (In production, you'd want a timeout here)
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn call<Req: serde::Serialize, Resp: serde::de::DeserializeOwned>(
        &self,
        _request_tag: u32,
        _response_tag: u32,
        _request: &Req,
    ) -> Result<Resp, KeyError> {
        Err(KeyError::StorageError(String::from(
            "Identity IPC not available outside WASM",
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = IdentityClient::new();
        assert_eq!(client.identity_endpoint, IDENTITY_ENDPOINT_SLOT);
        assert_eq!(client.input_endpoint, INPUT_ENDPOINT_SLOT);

        let client2 = IdentityClient::with_endpoints(10, 5);
        assert_eq!(client2.identity_endpoint, 10);
        assert_eq!(client2.input_endpoint, 5);
    }
}
