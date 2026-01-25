//! Zero OS Identity Layer
//!
//! The Identity layer provides user-facing identity management for Zero OS:
//!
//! - **Users**: User primitive backed by Zero-ID identities
//! - **Sessions**: Local and remote session management
//! - **KeyStore**: Cryptographic key storage and operations
//! - **Service**: User and session service traits and implementations
//! - **IPC**: Inter-process communication protocol for identity operations
//!
//! # Design Principles
//!
//! 1. **Offline-first**: Local authentication works without network
//! 2. **File-based**: All identity material stored as files in user home directory
//! 3. **Zero-ID integrated**: Cryptographic identity backed by Zero-ID protocol
//! 4. **Capability-aware**: Permission policies control capability grants
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                            Identity Layer                                     │
//! │                                                                              │
//! │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────────┐ │
//! │  │   User Service  │  │ Session Service │  │   Permission Service        │ │
//! │  └────────┬────────┘  └────────┬────────┘  └─────────────┬───────────────┘ │
//! │           │                    │                          │                  │
//! │           └────────────────────┼──────────────────────────┘                  │
//! │                                │                                             │
//! │                     ┌──────────▼──────────┐                                 │
//! │                     │   Zero-ID Store     │                                 │
//! │                     └──────────┬──────────┘                                 │
//! └────────────────────────────────┼─────────────────────────────────────────────┘
//!                                  │
//!                                  ▼
//!                     ┌────────────────────────┐
//!                     │   VFS (06-filesystem)  │
//!                     └────────────────────────┘
//! ```

#![no_std]
extern crate alloc;

pub mod client;
pub mod crypto;
pub mod error;
pub mod ipc;
pub mod keystore;
pub mod service;
pub mod session;
pub mod types;

// Re-export main types
pub use error::{IdentityError, KeyError, SessionError, UserError};
pub use keystore::{
    EncryptedPrivateKeys, KeyDerivation, KeyScheme, LocalKeyStore, MachineKeyCapabilities,
    MachineKeyRecord,
};
pub use service::{SessionService, UserService};
pub use session::{LocalSession, RemoteAuthState, SessionMetadata};
pub use types::{User, UserPreferences, UserRegistry, UserRegistryEntry, UserStatus};

// IPC message constants - re-export from zos-ipc (single source of truth)
pub use zos_process::{
    identity_cred, identity_key, identity_machine, identity_perm, identity_query, identity_remote,
    identity_session, identity_user,
};

// Key management IPC types
pub use ipc::{
    CreateMachineKeyRequest, CreateMachineKeyResponse, GetIdentityKeyRequest,
    GetIdentityKeyResponse, GetMachineKeyRequest, GetMachineKeyResponse, ListMachineKeysRequest,
    ListMachineKeysResponse, RegisterIdentityKeyRequest, RegisterIdentityKeyResponse,
    RevokeMachineKeyRequest, RevokeMachineKeyResponse, RotateMachineKeyRequest,
    RotateMachineKeyResponse,
};

// Neural Key IPC types
pub use ipc::{
    GenerateNeuralKeyRequest, GenerateNeuralKeyResponse, NeuralKeyGenerated, NeuralShard,
    PublicIdentifiers, RecoverNeuralKeyRequest, RecoverNeuralKeyResponse,
};

// IPC Client
pub use client::{IdentityClient, IDENTITY_ENDPOINT_SLOT, INPUT_ENDPOINT_SLOT};
