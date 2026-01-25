//! Zero OS Application Framework
//!
//! This crate provides the platform-agnostic application model for Zero OS:
//!
//! - **ZeroApp trait**: The interface all apps implement
//! - **AppManifest**: Declarative capability requirements
//! - **AppRuntime**: Event loop that drives apps inside WASM processes
//! - **App Protocol**: Binary wire format for Backend ↔ UI communication
//!
//! # Example
//!
//! ```ignore
//! use zos_apps::{app_main, ZeroApp, AppContext, ControlFlow, AppError, Message};
//!
//! #[derive(Default)]
//! struct MyApp { /* state */ }
//!
//! impl ZeroApp for MyApp {
//!     fn manifest() -> &'static AppManifest { &MY_APP_MANIFEST }
//!     fn init(&mut self, _ctx: &AppContext) -> Result<(), AppError> { Ok(()) }
//!     fn update(&mut self, _ctx: &AppContext) -> ControlFlow { ControlFlow::Yield }
//!     fn on_message(&mut self, _ctx: &AppContext, _msg: Message) -> Result<(), AppError> { Ok(()) }
//!     fn shutdown(&mut self, _ctx: &AppContext) {}
//! }
//!
//! app_main!(MyApp);
//! ```

// Note: Library uses std, but the app_main! macro sets up no_std for binaries
extern crate alloc;

pub mod app;
pub mod app_protocol;
pub mod error;
pub mod identity;
pub mod manifest;
pub mod runtime;
pub mod vfs_async;

// Re-export core types at crate root
pub use app::{AppContext, ControlFlow, Message, ZeroApp};
pub use app_protocol::tags;
pub use error::{AppError, ProtocolError};
pub use manifest::{AppManifest, CapabilityRequest};
pub use runtime::AppRuntime;

// Re-export syscall interface from zos-process
pub use zos_process as syscall;

// Re-export IPC protocol modules from zos-process (which re-exports from zos-ipc)
// This allows apps to use consistent message constants.
pub use zos_process::{init, kernel, permission, pm, storage, supervisor};

/// Generate the entry point and runtime setup for a Zero app.
///
/// This macro eliminates boilerplate by generating:
/// - The `_start` entry point
/// - Panic handler
/// - Global allocator
///
/// # Usage
///
/// ```ignore
/// use zos_apps::{app_main, ZeroApp};
///
/// #[derive(Default)]
/// struct MyApp;
///
/// impl ZeroApp for MyApp {
///     // ... trait implementation
/// }
///
/// app_main!(MyApp);
/// ```
#[macro_export]
macro_rules! app_main {
    ($app_type:ty) => {
        /// Entry point called by the WASM runtime
        #[no_mangle]
        pub extern "C" fn _start() {
            // #region agent log
            $crate::syscall::debug("[_start] Entry point called");
            // #endregion
            
            // Create app instance
            let app = <$app_type>::default();
            
            // #region agent log
            $crate::syscall::debug("[_start] App instance created");
            // #endregion

            // Create and run runtime
            let mut runtime = $crate::AppRuntime::new();
            
            // #region agent log
            use alloc::format;
            $crate::syscall::debug(&format!(
                "[_start] Runtime created, PID={}",
                runtime.pid()
            ));
            // #endregion

            // Set app ID from manifest
            runtime.set_app_id(<$app_type as $crate::ZeroApp>::manifest().id);

            // Setup endpoints from capability slots
            // Slot 0 is typically the UI output endpoint
            // Slot 1 is typically the input endpoint
            runtime.set_ui_endpoint(0);
            runtime.set_input_endpoint(1);
            
            // #region agent log
            $crate::syscall::debug(&format!(
                "[_start] Endpoints configured, starting run loop for {}",
                <$app_type as $crate::ZeroApp>::manifest().id
            ));
            // #endregion

            // Run forever (exits via syscall::exit)
            runtime.run(app);
        }
    };
}
