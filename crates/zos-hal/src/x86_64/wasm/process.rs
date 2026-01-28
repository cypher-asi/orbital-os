//! WASM process state management
//!
//! Defines the state structure for each WASM process instance.

use alloc::string::String;
use wasmi::{Instance, Store, TypedFunc};

use super::host::HostState;

/// State of a WASM process
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    /// Process is ready to run
    Ready,
    /// Process is currently executing
    Running,
    /// Process is blocked waiting for a syscall result
    Blocked,
    /// Process has terminated
    Terminated,
}

/// A running WASM process
pub struct WasmProcess {
    /// Process ID
    pub pid: u64,
    /// Human-readable name
    pub name: String,
    /// Current state
    pub state: ProcessState,
    /// Wasmi store containing the process's host state
    pub store: Store<HostState>,
    /// Wasmi instance
    pub instance: Instance,
    /// The _start function (None if already called)
    pub start_func: Option<TypedFunc<(), ()>>,
    /// Memory size in bytes
    pub memory_size: usize,
}

impl WasmProcess {
    /// Get the process's memory size
    pub fn update_memory_size(&mut self) {
        if let Some(wasmi::Extern::Memory(memory)) = self.instance.get_export(&self.store, "memory") {
            // wasmi memory size - use data_size() which returns bytes
            self.memory_size = memory.data_size(&self.store);
        }
    }
    
    /// Get access to the host state
    pub fn host_state(&self) -> &HostState {
        self.store.data()
    }
    
    /// Get mutable access to the host state
    pub fn host_state_mut(&mut self) -> &mut HostState {
        self.store.data_mut()
    }
}

// SAFETY: WasmProcess is only accessed from a single-threaded kernel context
unsafe impl Send for WasmProcess {}
unsafe impl Sync for WasmProcess {}
