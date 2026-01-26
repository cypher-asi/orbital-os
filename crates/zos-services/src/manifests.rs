//! Service Manifests
//!
//! Declares manifest identities and capability requirements for system services.
//!
//! These manifests define the core system services that run in Zero OS:
//! - PermissionService (PID 2): System capability authority
//! - IdentityService (PID 5): User identity and key management
//! - VfsService (PID 4): Virtual filesystem operations
//! - TimeService (PID 6): Time settings management
//! - KeystoreService (PID 7): Cryptographic key storage
//! - NetworkService (PID 8): HTTP request mediation

use zos_apps::{AppManifest, CapabilityRequest, ObjectType, Permissions};

/// Permission Service manifest (PID 2)
pub static PERMISSION_SERVICE_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.permission_service",
    name: "Permission Service",
    version: "1.0.0",
    description: "System capability authority service",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::full(),
            reason: "Receive capability requests and send responses",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Console,
            permissions: Permissions::full(),
            reason: "Root console capability for granting to apps",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Process,
            permissions: Permissions::full(),
            reason: "Root process capability for granting spawn rights",
            required: true,
        },
    ],
};

/// IdentityService manifest (PID 3)
pub static IDENTITY_SERVICE_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.identity_service",
    name: "Identity Service",
    version: "1.0.0",
    description: "User identity and cryptographic key management service",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::full(),
            reason: "Receive identity requests and send responses",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Filesystem,
            permissions: Permissions::read_write(),
            reason: "Read and write identity data to user home directories",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Identity,
            permissions: Permissions::full(),
            reason: "Manage cryptographic keys and identity operations",
            required: true,
        },
    ],
};

/// VFS Service manifest (PID 4)
pub static VFS_SERVICE_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.vfs_service",
    name: "VFS Service",
    version: "1.0.0",
    description: "Virtual filesystem service for Zero OS",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::full(),
            reason: "Receive VFS requests and send responses",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Storage,
            permissions: Permissions::full(),
            reason: "Access IndexedDB for persistent filesystem storage",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Filesystem,
            permissions: Permissions::full(),
            reason: "Provide filesystem operations to all processes",
            required: true,
        },
    ],
};

/// Time Service manifest (PID 5)
pub static TIME_SERVICE_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.time_service",
    name: "Time Service",
    version: "1.0.0",
    description: "Time settings management service for Zero OS",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::full(),
            reason: "Receive time settings requests and send responses",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Storage,
            permissions: Permissions::read_write(),
            reason: "Persist time settings to system storage",
            required: true,
        },
    ],
};

/// Network Service manifest (PID 8)
pub static NETWORK_SERVICE_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.network_service",
    name: "Network Service",
    version: "1.0.0",
    description: "HTTP request mediation service for Zero OS",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::full(),
            reason: "Receive network requests and send responses",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Network,
            permissions: Permissions::full(),
            reason: "Perform HTTP requests on behalf of other processes",
            required: true,
        },
    ],
};

/// Keystore Service manifest (PID 7)
pub static KEYSTORE_SERVICE_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.keystore_service",
    name: "Keystore Service",
    version: "1.0.0",
    description: "Cryptographic key storage service for Zero OS",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::full(),
            reason: "Receive keystore requests and send responses",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Keystore,
            permissions: Permissions::full(),
            reason: "Access zos-keystore IndexedDB for secure key storage",
            required: true,
        },
    ],
};
