//! ZeroApp Trait and Core Types
//!
//! Defines the interface that all Zero applications implement.

use crate::error::AppError;
use crate::manifest::AppManifest;
use alloc::string::String;
use alloc::vec::Vec;

/// User ID type (128-bit UUID).
pub type UserId = u128;

/// Session ID type (128-bit UUID).
pub type SessionId = u128;

/// User identity context for apps.
#[derive(Clone, Debug, Default)]
pub struct UserContext {
    /// User ID of the user who launched this app (None = system process)
    pub user_id: Option<UserId>,

    /// Session ID this app is running under (None = system process)
    pub session_id: Option<SessionId>,

    /// Display name of the user (for UI)
    pub display_name: Option<String>,
}

impl UserContext {
    /// Create a system context (no user).
    pub fn system() -> Self {
        Self::default()
    }

    /// Create a user context.
    pub fn user(user_id: UserId, session_id: SessionId, display_name: String) -> Self {
        Self {
            user_id: Some(user_id),
            session_id: Some(session_id),
            display_name: Some(display_name),
        }
    }

    /// Check if this is a system context (no user).
    pub fn is_system(&self) -> bool {
        self.user_id.is_none()
    }

    /// Check if this is a user context.
    pub fn is_user(&self) -> bool {
        self.user_id.is_some()
    }
}

/// Execution context provided to app methods
#[derive(Clone, Debug)]
pub struct AppContext {
    /// This process's ID
    pub pid: u32,

    /// Monotonic uptime in nanoseconds (via SYS_GET_TIME)
    /// Suitable for measuring durations, scheduling
    pub uptime_ns: u64,

    /// Wall-clock time in milliseconds since Unix epoch (via SYS_GET_WALLCLOCK)
    /// Suitable for displaying time-of-day to users
    pub wallclock_ms: u64,

    /// Capability slot for communicating with UI (if connected)
    pub ui_endpoint: Option<u32>,

    /// Capability slot for receiving input
    pub input_endpoint: Option<u32>,

    /// User identity context (who launched this app)
    pub user: UserContext,

    /// App ID (from manifest)
    pub app_id: String,
}

impl AppContext {
    /// Create a new context with the given values
    pub fn new(
        pid: u32,
        uptime_ns: u64,
        wallclock_ms: u64,
        ui_endpoint: Option<u32>,
        input_endpoint: Option<u32>,
    ) -> Self {
        Self {
            pid,
            uptime_ns,
            wallclock_ms,
            ui_endpoint,
            input_endpoint,
            user: UserContext::system(),
            app_id: String::new(),
        }
    }

    /// Create a context with user information.
    pub fn with_user(mut self, user: UserContext) -> Self {
        self.user = user;
        self
    }

    /// Set the app ID.
    pub fn with_app_id(mut self, app_id: String) -> Self {
        self.app_id = app_id;
        self
    }

    /// Get the app's data directory path.
    ///
    /// For user apps: `/home/{user_id}/Apps/{app_id}/data`
    /// For system apps: `/system/apps/{app_id}/data`
    pub fn data_dir(&self) -> String {
        if let Some(user_id) = self.user.user_id {
            alloc::format!("/home/{:032x}/Apps/{}/data", user_id, self.app_id)
        } else {
            alloc::format!("/system/apps/{}/data", self.app_id)
        }
    }

    /// Get the app's config directory path.
    ///
    /// For user apps: `/home/{user_id}/Apps/{app_id}/config`
    /// For system apps: `/system/apps/{app_id}/config`
    pub fn config_dir(&self) -> String {
        if let Some(user_id) = self.user.user_id {
            alloc::format!("/home/{:032x}/Apps/{}/config", user_id, self.app_id)
        } else {
            alloc::format!("/system/apps/{}/config", self.app_id)
        }
    }

    /// Get the app's cache directory path.
    ///
    /// For user apps: `/home/{user_id}/Apps/{app_id}/cache`
    /// For system apps: `/tmp/apps/{app_id}/cache`
    pub fn cache_dir(&self) -> String {
        if let Some(user_id) = self.user.user_id {
            alloc::format!("/home/{:032x}/Apps/{}/cache", user_id, self.app_id)
        } else {
            alloc::format!("/tmp/apps/{}/cache", self.app_id)
        }
    }

    /// Get the user's home directory path (if user context).
    pub fn home_dir(&self) -> Option<String> {
        self.user.user_id.map(|id| alloc::format!("/home/{:032x}", id))
    }

    /// Check if this app is running as a system process.
    pub fn is_system_app(&self) -> bool {
        self.user.is_system()
    }
}

/// Control flow returned by update()
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlFlow {
    /// Continue to next update cycle
    Continue,

    /// Exit with the given code
    Exit(i32),

    /// Yield CPU, wait for next scheduling quantum
    Yield,
}

/// An IPC message received by the app
#[derive(Clone, Debug)]
pub struct Message {
    /// Message tag (identifies message type)
    pub tag: u32,

    /// Sender's process ID
    pub from_pid: u32,

    /// Message payload data
    pub data: Vec<u8>,
}

impl Message {
    /// Create a new message
    pub fn new(tag: u32, from_pid: u32, data: Vec<u8>) -> Self {
        Self { tag, from_pid, data }
    }
}

/// The Program Interface that all Zero apps implement.
///
/// # Lifecycle
///
/// 1. **init()**: Called once when the app starts. Initialize state, set up IPC endpoints.
/// 2. **update()**: Called repeatedly in the event loop. Perform periodic work, update state.
/// 3. **on_message()**: Called when a message is received via IPC.
/// 4. **shutdown()**: Called before the app exits. Clean up resources.
///
/// # Example
///
/// ```ignore
/// use zos_apps::*;
///
/// #[derive(Default)]
/// struct MyApp {
///     counter: u32,
/// }
///
/// impl ZeroApp for MyApp {
///     fn manifest() -> &'static AppManifest {
///         &MY_MANIFEST
///     }
///
///     fn init(&mut self, _ctx: &AppContext) -> Result<(), AppError> {
///         Ok(())
///     }
///
///     fn update(&mut self, ctx: &AppContext) -> ControlFlow {
///         self.counter += 1;
///         ControlFlow::Yield
///     }
///
///     fn on_message(&mut self, _ctx: &AppContext, msg: Message) -> Result<(), AppError> {
///         // Handle incoming messages
///         Ok(())
///     }
///
///     fn shutdown(&mut self, _ctx: &AppContext) {
///         // Cleanup
///     }
/// }
/// ```
pub trait ZeroApp {
    /// Returns the static application manifest.
    fn manifest() -> &'static AppManifest
    where
        Self: Sized;

    /// Called once when the app starts.
    /// Initialize state, set up IPC endpoints.
    fn init(&mut self, ctx: &AppContext) -> Result<(), AppError>;

    /// Called repeatedly in the event loop.
    /// Perform periodic work, update state.
    fn update(&mut self, ctx: &AppContext) -> ControlFlow;

    /// Called when a message is received via IPC.
    fn on_message(&mut self, ctx: &AppContext, msg: Message) -> Result<(), AppError>;

    /// Called before the app exits.
    /// Clean up resources.
    fn shutdown(&mut self, ctx: &AppContext);
}
