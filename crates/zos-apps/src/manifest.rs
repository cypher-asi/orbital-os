//! Application Manifest
//!
//! Declares application identity and capability requirements.

/// Object types that capabilities can reference (matches zos-kernel)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectType {
    /// IPC endpoint
    Endpoint = 1,
    /// Another process
    Process = 2,
    /// Memory region
    Memory = 3,
    /// IRQ handler
    Irq = 4,
    /// I/O port range
    IoPort = 5,
    /// Console/debug output
    Console = 6,
    /// Persistent storage (namespaced per-app)
    Storage = 7,
    /// Network access
    Network = 8,
    /// Filesystem access (VFS)
    Filesystem = 9,
    /// User identity service
    Identity = 10,
}

/// Permission bits for capabilities
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Permissions {
    /// Can read from the object
    pub read: bool,
    /// Can write to the object
    pub write: bool,
    /// Can grant this capability to other processes
    pub grant: bool,
}

impl Permissions {
    /// Full permissions (read, write, grant)
    pub const fn full() -> Self {
        Self {
            read: true,
            write: true,
            grant: true,
        }
    }

    /// Read-write permissions (no grant)
    pub const fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            grant: false,
        }
    }

    /// Read-only permission
    pub const fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            grant: false,
        }
    }

    /// Write-only permission
    pub const fn write_only() -> Self {
        Self {
            read: false,
            write: true,
            grant: false,
        }
    }
}

/// A capability request with reason for user consent
#[derive(Clone, Debug)]
pub struct CapabilityRequest {
    /// Type of kernel object being requested
    pub object_type: ObjectType,
    /// Permissions needed on this object
    pub permissions: Permissions,
    /// Human-readable reason (shown to user in permission dialog)
    pub reason: &'static str,
    /// Whether the app can function without this capability
    pub required: bool,
}

/// Application manifest declaring identity and capabilities
#[derive(Clone, Debug)]
pub struct AppManifest {
    /// Unique identifier, reverse-domain format
    /// Example: "com.zero.clock"
    pub id: &'static str,

    /// Human-readable name
    /// Example: "Clock"
    pub name: &'static str,

    /// Semantic version
    /// Example: "1.0.0"
    pub version: &'static str,

    /// Brief description
    pub description: &'static str,

    /// Requested capabilities
    pub capabilities: &'static [CapabilityRequest],
}

impl AppManifest {
    /// Create a manifest for a minimal app (endpoint capability only)
    pub const fn minimal(
        id: &'static str,
        name: &'static str,
        version: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            id,
            name,
            version,
            description,
            capabilities: &[],
        }
    }

    /// Get the app's unique ID
    pub fn id(&self) -> &str {
        self.id
    }

    /// Get the app's display name
    pub fn name(&self) -> &str {
        self.name
    }

    /// Check if this is a factory (built-in) app
    pub fn is_factory_app(&self) -> bool {
        self.id.starts_with("com.zero.")
    }
}

// ============================================================================
// Factory App Manifests
// ============================================================================

/// Clock app manifest
pub static CLOCK_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.clock",
    name: "Clock",
    version: "1.0.0",
    description: "Displays current time and date",
    capabilities: &[CapabilityRequest {
        object_type: ObjectType::Endpoint,
        permissions: Permissions::read_write(),
        reason: "Send time updates to display",
        required: true,
    }],
};

/// Calculator app manifest
pub static CALCULATOR_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.calculator",
    name: "Calculator",
    version: "1.0.0",
    description: "Basic arithmetic calculator",
    capabilities: &[CapabilityRequest {
        object_type: ObjectType::Endpoint,
        permissions: Permissions::read_write(),
        reason: "Receive input and send results to display",
        required: true,
    }],
};

/// Terminal app manifest
pub static TERMINAL_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.terminal",
    name: "Terminal",
    version: "1.0.0",
    description: "Command-line interface for Zero OS",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Console,
            permissions: Permissions::read_write(),
            reason: "Display command output and read user input",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Process,
            permissions: Permissions::read_only(),
            reason: "List running processes (ps command)",
            required: false,
        },
    ],
};

/// PermissionManager service manifest (PID 2)
pub static PERMISSION_MANAGER_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.permission_manager",
    name: "Permission Manager",
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

/// Settings app manifest
pub static SETTINGS_MANIFEST: AppManifest = AppManifest {
    id: "com.zero.settings",
    name: "Settings",
    version: "1.0.0",
    description: "System settings and preferences management",
    capabilities: &[
        CapabilityRequest {
            object_type: ObjectType::Endpoint,
            permissions: Permissions::read_write(),
            reason: "Send settings updates to display",
            required: true,
        },
        CapabilityRequest {
            object_type: ObjectType::Storage,
            permissions: Permissions::read_write(),
            reason: "Persist user preferences and settings",
            required: false,
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
