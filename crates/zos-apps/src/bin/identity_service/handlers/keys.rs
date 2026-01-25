//! Neural key and machine key operations
//!
//! Handlers for:
//! - Neural key generation and recovery
//! - Machine key CRUD operations (create, list, get, revoke, rotate)

use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use zos_apps::identity::crypto::{derive_public_key, generate_random_bytes, shamir_reconstruct, shamir_split};
use zos_apps::identity::pending::PendingStorageOp;
use zos_apps::identity::response;
use zos_apps::syscall;
use zos_apps::{AppError, Message};
use zos_identity::ipc::{CreateMachineKeyRequest, GenerateNeuralKeyRequest, GetIdentityKeyRequest, GetMachineKeyRequest, ListMachineKeysRequest, NeuralKeyGenerated, PublicIdentifiers, RecoverNeuralKeyRequest, RevokeMachineKeyRequest, RotateMachineKeyRequest};
use zos_identity::keystore::{KeyScheme, LocalKeyStore, MachineKeyRecord};
use zos_identity::KeyError;

use crate::service::IdentityService;

// =============================================================================
// Neural Key Operations
// =============================================================================

pub fn handle_generate_neural_key(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    syscall::debug("IdentityService: Handling generate neural key request");

    let request: GenerateNeuralKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_neural_key_error(msg.from_pid, &msg.cap_slots, KeyError::DerivationFailed);
        }
    };

    let user_id = request.user_id;
    syscall::debug(&format!("IdentityService: Generating Neural Key for user {:032x}", user_id));

    let key_path = LocalKeyStore::storage_path(user_id);
    service.start_storage_exists(
        &format!("inode:{}", key_path),
        PendingStorageOp::CheckKeyExists {
            client_pid: msg.from_pid,
            user_id,
            cap_slots: msg.cap_slots.clone(),
        },
    )
}

pub fn continue_generate_after_exists_check(service: &mut IdentityService, client_pid: u32, user_id: u128, exists: bool, cap_slots: Vec<u32>) -> Result<(), AppError> {
    use zos_apps::identity::crypto::bytes_to_hex;

    if exists {
        syscall::debug("IdentityService: Neural Key already exists");
        return response::send_neural_key_error(client_pid, &cap_slots, KeyError::IdentityKeyAlreadyExists);
    }

    let entropy = generate_random_bytes(32);
    let identity_signing = derive_public_key(&entropy, "identity-signing");
    let machine_signing = derive_public_key(&entropy, "machine-signing");
    let machine_encryption = derive_public_key(&entropy, "machine-encryption");

    let shards = shamir_split(&entropy, 3, 5);
    let public_identifiers = PublicIdentifiers {
        identity_signing_pub_key: format!("0x{}", bytes_to_hex(&identity_signing)),
        machine_signing_pub_key: format!("0x{}", bytes_to_hex(&machine_signing)),
        machine_encryption_pub_key: format!("0x{}", bytes_to_hex(&machine_encryption)),
    };

    let created_at = syscall::get_wallclock();
    let key_store = LocalKeyStore::new(user_id, identity_signing, machine_signing, machine_encryption, created_at);
    let result = NeuralKeyGenerated { public_identifiers, shards, created_at };

    let key_path = LocalKeyStore::storage_path(user_id);
    match serde_json::to_vec(&key_store) {
        Ok(json_bytes) => service.start_storage_write(
            &format!("content:{}", key_path),
            &json_bytes.clone(),
            PendingStorageOp::WriteKeyStoreContent { client_pid, user_id, result, json_bytes, cap_slots },
        ),
        Err(e) => response::send_neural_key_error(client_pid, &cap_slots, KeyError::StorageError(format!("Serialization failed: {}", e))),
    }
}

pub fn handle_recover_neural_key(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    use zos_apps::identity::crypto::bytes_to_hex;

    syscall::debug("IdentityService: Handling recover neural key request");

    let request: RecoverNeuralKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_recover_key_error(msg.from_pid, &msg.cap_slots, KeyError::DerivationFailed);
        }
    };

    if request.shards.len() < 3 {
        return response::send_recover_key_error(msg.from_pid, &msg.cap_slots, KeyError::InsufficientShards);
    }

    let entropy = match shamir_reconstruct(&request.shards) {
        Ok(e) => e,
        Err(e) => return response::send_recover_key_error(msg.from_pid, &msg.cap_slots, e),
    };

    let identity_signing = derive_public_key(&entropy, "identity-signing");
    let machine_signing = derive_public_key(&entropy, "machine-signing");
    let machine_encryption = derive_public_key(&entropy, "machine-encryption");

    let public_identifiers = PublicIdentifiers {
        identity_signing_pub_key: format!("0x{}", bytes_to_hex(&identity_signing)),
        machine_signing_pub_key: format!("0x{}", bytes_to_hex(&machine_signing)),
        machine_encryption_pub_key: format!("0x{}", bytes_to_hex(&machine_encryption)),
    };

    let created_at = syscall::get_wallclock();
    let key_store = LocalKeyStore::new(request.user_id, identity_signing, machine_signing, machine_encryption, created_at);
    let new_shards = shamir_split(&entropy, 3, 5);
    let result = NeuralKeyGenerated { public_identifiers, shards: new_shards, created_at };

    let key_path = LocalKeyStore::storage_path(request.user_id);
    match serde_json::to_vec(&key_store) {
        Ok(json_bytes) => service.start_storage_write(
            &format!("content:{}", key_path),
            &json_bytes.clone(),
            PendingStorageOp::WriteRecoveredKeyStoreContent {
                client_pid: msg.from_pid,
                user_id: request.user_id,
                result,
                json_bytes,
                cap_slots: msg.cap_slots.clone(),
            },
        ),
        Err(e) => response::send_recover_key_error(msg.from_pid, &msg.cap_slots, KeyError::StorageError(format!("Serialization failed: {}", e))),
    }
}

pub fn handle_get_identity_key(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    let request: GetIdentityKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_get_identity_key_error(msg.from_pid, &msg.cap_slots, KeyError::DerivationFailed);
        }
    };

    let key_path = LocalKeyStore::storage_path(request.user_id);
    service.start_storage_read(&format!("content:{}", key_path), PendingStorageOp::GetIdentityKey {
        client_pid: msg.from_pid,
        cap_slots: msg.cap_slots.clone(),
    })
}

// =============================================================================
// Machine Key Operations
// =============================================================================

pub fn handle_create_machine_key(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    let request: CreateMachineKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_create_machine_key_error(msg.from_pid, &msg.cap_slots, KeyError::DerivationFailed);
        }
    };

    let key_path = LocalKeyStore::storage_path(request.user_id);
    service.start_storage_exists(&format!("inode:{}", key_path), PendingStorageOp::CheckIdentityForMachine {
        client_pid: msg.from_pid,
        request,
        cap_slots: msg.cap_slots.clone(),
    })
}

pub fn continue_create_machine_after_identity_check(service: &mut IdentityService, client_pid: u32, request: CreateMachineKeyRequest, exists: bool, cap_slots: Vec<u32>) -> Result<(), AppError> {
    if !exists {
        return response::send_create_machine_key_error(client_pid, &cap_slots, KeyError::IdentityKeyRequired);
    }

    let machine_entropy = generate_random_bytes(32);
    let machine_id_bytes = generate_random_bytes(16);
    let machine_id = u128::from_le_bytes(machine_id_bytes[..16].try_into().unwrap_or([0u8; 16]));

    // Always generate classical keys (Ed25519/X25519)
    let signing_key = derive_public_key(&machine_entropy, "machine-signing");
    let encryption_key = derive_public_key(&machine_entropy, "machine-encryption");
    let now = syscall::get_wallclock();

    // Generate PQ keys if PqHybrid scheme is requested
    let (pq_signing_public_key, pq_encryption_public_key) = if request.key_scheme == KeyScheme::PqHybrid {
        // For WASM target, we generate deterministic placeholder keys using HKDF
        // Real PQ key generation would use ML-DSA-65 (1952 bytes) and ML-KEM-768 (1184 bytes)
        // TODO: Integrate actual PQ crypto library when WASM-compatible version is available
        let pq_sign_seed = derive_public_key(&machine_entropy, "cypher:shared:machine:pq-sign:v1");
        let pq_kem_seed = derive_public_key(&machine_entropy, "cypher:shared:machine:pq-kem:v1");
        
        // Create placeholder public keys with correct sizes
        // ML-DSA-65 public key: 1952 bytes
        // ML-KEM-768 public key: 1184 bytes
        let mut pq_sign_pk = vec![0u8; 1952];
        pq_sign_pk[..32].copy_from_slice(&pq_sign_seed);
        
        let mut pq_kem_pk = vec![0u8; 1184];
        pq_kem_pk[..32].copy_from_slice(&pq_kem_seed);
        
        syscall::debug(&format!(
            "IdentityService: Generated PQ-Hybrid keys (placeholder) for machine {:032x}",
            machine_id
        ));
        
        (Some(pq_sign_pk), Some(pq_kem_pk))
    } else {
        (None, None)
    };

    let record = MachineKeyRecord {
        machine_id,
        signing_public_key: signing_key,
        encryption_public_key: encryption_key,
        authorized_at: now,
        authorized_by: request.user_id,
        capabilities: request.capabilities,
        machine_name: request.machine_name,
        last_seen_at: now,
        epoch: 1,
        key_scheme: request.key_scheme,
        pq_signing_public_key,
        pq_encryption_public_key,
    };

    let machine_path = MachineKeyRecord::storage_path(request.user_id, machine_id);
    match serde_json::to_vec(&record) {
        Ok(json_bytes) => service.start_storage_write(
            &format!("content:{}", machine_path),
            &json_bytes.clone(),
            PendingStorageOp::WriteMachineKeyContent {
                client_pid,
                user_id: request.user_id,
                record,
                json_bytes,
                cap_slots,
            },
        ),
        Err(e) => response::send_create_machine_key_error(client_pid, &cap_slots, KeyError::StorageError(format!("Serialization failed: {}", e))),
    }
}

pub fn handle_list_machine_keys(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    let request: ListMachineKeysRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(_) => return response::send_list_machine_keys(msg.from_pid, &msg.cap_slots, vec![]),
    };

    let machine_dir = format!("/home/{:032x}/.zos/identity/machine", request.user_id);
    service.start_storage_list(&format!("inode:{}", machine_dir), PendingStorageOp::ListMachineKeys {
        client_pid: msg.from_pid,
        user_id: request.user_id,
        cap_slots: msg.cap_slots.clone(),
    })
}

pub fn handle_revoke_machine_key(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    let request: RevokeMachineKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_revoke_machine_key_error(msg.from_pid, &msg.cap_slots, KeyError::DerivationFailed);
        }
    };

    let machine_path = MachineKeyRecord::storage_path(request.user_id, request.machine_id);
    service.start_storage_delete(&format!("content:{}", machine_path), PendingStorageOp::DeleteMachineKey {
        client_pid: msg.from_pid,
        user_id: request.user_id,
        machine_id: request.machine_id,
        cap_slots: msg.cap_slots.clone(),
    })
}

pub fn handle_rotate_machine_key(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    let request: RotateMachineKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(e) => {
            syscall::debug(&format!("IdentityService: Failed to parse request: {}", e));
            return response::send_rotate_machine_key_error(msg.from_pid, &msg.cap_slots, KeyError::DerivationFailed);
        }
    };

    let machine_path = MachineKeyRecord::storage_path(request.user_id, request.machine_id);
    service.start_storage_read(&format!("content:{}", machine_path), PendingStorageOp::ReadMachineForRotate {
        client_pid: msg.from_pid,
        user_id: request.user_id,
        machine_id: request.machine_id,
        cap_slots: msg.cap_slots.clone(),
    })
}

pub fn continue_rotate_after_read(service: &mut IdentityService, client_pid: u32, user_id: u128, machine_id: u128, data: &[u8], cap_slots: Vec<u32>) -> Result<(), AppError> {
    let mut record: MachineKeyRecord = match serde_json::from_slice(data) {
        Ok(r) => r,
        Err(e) => return response::send_rotate_machine_key_error(client_pid, &cap_slots, KeyError::StorageError(format!("Parse failed: {}", e))),
    };

    let new_entropy = generate_random_bytes(32);
    record.signing_public_key = derive_public_key(&new_entropy, "machine-signing");
    record.encryption_public_key = derive_public_key(&new_entropy, "machine-encryption");
    record.epoch += 1;
    record.last_seen_at = syscall::get_wallclock();

    // Regenerate PQ keys if PqHybrid scheme
    if record.key_scheme == KeyScheme::PqHybrid {
        let pq_sign_seed = derive_public_key(&new_entropy, "cypher:shared:machine:pq-sign:v1");
        let pq_kem_seed = derive_public_key(&new_entropy, "cypher:shared:machine:pq-kem:v1");
        
        let mut pq_sign_pk = vec![0u8; 1952];
        pq_sign_pk[..32].copy_from_slice(&pq_sign_seed);
        
        let mut pq_kem_pk = vec![0u8; 1184];
        pq_kem_pk[..32].copy_from_slice(&pq_kem_seed);
        
        record.pq_signing_public_key = Some(pq_sign_pk);
        record.pq_encryption_public_key = Some(pq_kem_pk);
        
        syscall::debug(&format!(
            "IdentityService: Rotated PQ-Hybrid keys for machine {:032x} (epoch {})",
            machine_id, record.epoch
        ));
    }

    let machine_path = MachineKeyRecord::storage_path(user_id, machine_id);
    match serde_json::to_vec(&record) {
        Ok(json_bytes) => service.start_storage_write(
            &format!("content:{}", machine_path),
            &json_bytes.clone(),
            PendingStorageOp::WriteRotatedMachineKeyContent { client_pid, user_id, record, json_bytes, cap_slots },
        ),
        Err(e) => response::send_rotate_machine_key_error(client_pid, &cap_slots, KeyError::StorageError(format!("Serialization failed: {}", e))),
    }
}

pub fn handle_get_machine_key(service: &mut IdentityService, msg: &Message) -> Result<(), AppError> {
    let request: GetMachineKeyRequest = match serde_json::from_slice(&msg.data) {
        Ok(r) => r,
        Err(_e) => return response::send_get_machine_key_error(msg.from_pid, &msg.cap_slots, KeyError::DerivationFailed),
    };

    let machine_path = MachineKeyRecord::storage_path(request.user_id, request.machine_id);
    service.start_storage_read(&format!("content:{}", machine_path), PendingStorageOp::ReadSingleMachineKey {
        client_pid: msg.from_pid,
        cap_slots: msg.cap_slots.clone(),
    })
}
