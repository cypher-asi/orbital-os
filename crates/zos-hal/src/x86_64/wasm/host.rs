//! Host functions for WASM processes
//!
//! These functions are imported by WASM processes to communicate with the kernel.
//! They match the interface defined in `zos-process/src/syscalls/mod.rs`.

use alloc::vec::Vec;
use wasmi::{Caller, Linker};

use super::serial;

/// Host state for a WASM process
///
/// Contains the process's syscall buffers and state.
pub struct HostState {
    /// Process ID
    pub pid: u64,
    /// Syscall input buffer (data sent by process)
    pub syscall_in_buffer: Vec<u8>,
    /// Syscall output buffer (result data for process)
    pub syscall_out_buffer: Vec<u8>,
    /// Last syscall result code (i64 to support packed 64-bit returns)
    pub syscall_result: i64,
    /// Whether a result is pending from kernel (syscall was processed)
    pub has_pending_result: bool,
    /// Process is waiting for a syscall result
    pub waiting_for_syscall: bool,
    /// Process has yielded
    pub yielded: bool,
    /// Pending syscall to dispatch
    pub pending_syscall: Option<PendingSyscallInfo>,
    /// True if the last trap was from zos_yield() which returns () not i32
    /// This affects how we resume - yield needs empty return, syscall needs i32
    pub trapped_from_yield: bool,
}

/// Information about a pending syscall
#[derive(Clone, Debug)]
pub struct PendingSyscallInfo {
    pub syscall_num: u32,
    pub args: [u32; 3],
    pub data: Vec<u8>,
}

impl HostState {
    /// Create new host state for a process
    pub fn new(pid: u64) -> Self {
        Self {
            pid,
            syscall_in_buffer: Vec::with_capacity(super::MAX_SYSCALL_BUFFER),
            syscall_out_buffer: Vec::new(),
            syscall_result: 0,
            has_pending_result: false,
            waiting_for_syscall: false,
            yielded: false,
            pending_syscall: None,
            trapped_from_yield: false,
        }
    }
    
    /// Set syscall result (called by kernel after processing syscall)
    pub fn set_syscall_result(&mut self, result: i64, data: &[u8]) {
        self.syscall_result = result;
        self.syscall_out_buffer.clear();
        self.syscall_out_buffer.extend_from_slice(data);
        self.has_pending_result = true;
        self.waiting_for_syscall = false;
    }
    
    /// Clear syscall buffers
    pub fn clear_syscall_buffers(&mut self) {
        self.syscall_in_buffer.clear();
        self.syscall_out_buffer.clear();
        self.pending_syscall = None;
        self.has_pending_result = false;
        self.waiting_for_syscall = false;
    }
}

// Syscall numbers (from zos-ipc)
const SYS_NOP: u32 = 0x00;
const SYS_DEBUG: u32 = 0x01;
const SYS_TIME: u32 = 0x02;
const SYS_GETPID: u32 = 0x03;
const SYS_YIELD: u32 = 0x04;
const SYS_RANDOM: u32 = 0x05;
const SYS_CONSOLE_WRITE: u32 = 0x07;

/// Register host functions with the linker
pub fn register_host_functions(linker: &mut Linker<HostState>) {
    // Register wasm-bindgen shims first (these are no-ops but required for linking)
    register_wasm_bindgen_shims(linker);
    // zos_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> i64
    // Returns i64 to support 64-bit return values (e.g., packed slot|endpoint_id)
    // Returns Result to allow triggering resumable pauses for syscalls that need kernel processing
    linker.func_wrap("env", "zos_syscall", |mut caller: Caller<'_, HostState>, syscall_num: u32, arg1: u32, arg2: u32, arg3: u32| -> Result<i64, wasmi::Error> {
        let host = caller.data_mut();
        let pid = host.pid;
        
        // Handle simple syscalls directly without going through kernel
        match syscall_num {
            SYS_NOP => return Ok(0),
            
            SYS_DEBUG | SYS_CONSOLE_WRITE => {
                // Print debug/console output directly to serial
                let data = core::mem::take(&mut host.syscall_in_buffer);
                if let Ok(text) = core::str::from_utf8(&data) {
                    serial::write_str(text);
                }
                return Ok(0);
            }
            
            SYS_GETPID => return Ok(pid as i64),
            
            SYS_RANDOM => {
                // Generate random bytes using RDRAND
                let requested = core::cmp::min(arg1 as usize, 256);
                let mut random_bytes = alloc::vec![0u8; requested];
                
                // Use the HAL's random_bytes function via RDRAND
                use crate::x86_64::random::fill_random_bytes;
                if fill_random_bytes(&mut random_bytes) {
                    // Store in output buffer for process to retrieve
                    host.syscall_out_buffer.clear();
                    host.syscall_out_buffer.extend_from_slice(&random_bytes);
                    return Ok(requested as i64);
                } else {
                    return Ok(-1); // Error: RDRAND not available
                }
            }
            
            SYS_YIELD => {
                // Mark as yielded and trigger a resumable pause
                host.yielded = true;
                host.trapped_from_yield = false; // zos_syscall returns i32, not ()
                // Return an error to trigger a resumable pause from the host
                // This allows wasmi to return Resumable instead of just continuing execution
                return Err(wasmi::Error::from(wasmi::core::TrapCode::OutOfFuel));
            }
            
            _ => {
                // Other syscalls need kernel processing
                // Don't log SYS_RECV (0x41) to avoid spamming console during idle loop
                if syscall_num != 0x41 {
                    // Verbose syscall logging disabled for cleaner output
                    // serial::write_str(&alloc::format!(
                    //     "[wasm-rt] PID {} syscall: num=0x{:x}, args=[{}, {}, {}]\n",
                    //     pid, syscall_num, arg1, arg2, arg3
                    // ));
                }
            }
        }
        
        // Check if we already have a result waiting (syscall was processed, we're resuming)
        if host.has_pending_result {
            // We have a result from kernel - return it and clear for next syscall
            let result = host.syscall_result;
            host.syscall_result = 0;
            host.has_pending_result = false;
            // Don't clear syscall_out_buffer - process may call zos_recv_bytes to get it
            return Ok(result);
        }
        
        // Store pending syscall for the kernel to process
        host.pending_syscall = Some(PendingSyscallInfo {
            syscall_num,
            args: [arg1, arg2, arg3],
            data: core::mem::take(&mut host.syscall_in_buffer),
        });
        
        // Mark that we need to wait for a syscall result
        host.waiting_for_syscall = true;
        host.trapped_from_yield = false; // zos_syscall returns i32, not ()
        
        // Trigger a resumable pause by returning an error from the host function
        // This allows wasmi to return Resumable (host trap) instead of Err (wasm trap)
        Err(wasmi::Error::from(wasmi::core::TrapCode::OutOfFuel))
    }).expect("Failed to register zos_syscall");
    
    // zos_send_bytes(ptr: u32, len: u32)
    linker.func_wrap("env", "zos_send_bytes", |mut caller: Caller<'_, HostState>, ptr: u32, len: u32| {
        let memory = match caller.get_export("memory") {
            Some(wasmi::Extern::Memory(mem)) => mem,
            _ => {
                serial::write_str("[wasm-rt] ERROR: No memory export\n");
                return;
            }
        };
        
        let start = ptr as usize;
        let end = start + len as usize;
        
        // Read bytes from WASM memory
        let bytes: Vec<u8> = {
            let data = memory.data(&caller);
            if end > data.len() {
                serial::write_str(&alloc::format!(
                    "[wasm-rt] ERROR: zos_send_bytes out of bounds: {}..{} > {}\n",
                    start, end, data.len()
                ));
                return;
            }
            data[start..end].to_vec()
        };
        
        // Now we can safely borrow host mutably
        let host = caller.data_mut();
        host.syscall_in_buffer.clear();
        host.syscall_in_buffer.extend_from_slice(&bytes);
    }).expect("Failed to register zos_send_bytes");
    
    // zos_recv_bytes(ptr: u32, max_len: u32) -> u32
    linker.func_wrap("env", "zos_recv_bytes", |mut caller: Caller<'_, HostState>, ptr: u32, max_len: u32| -> u32 {
        let out_data: Vec<u8>;
        {
            let host = caller.data();
            out_data = host.syscall_out_buffer.clone();
        }
        
        let copy_len = core::cmp::min(out_data.len(), max_len as usize);
        
        if copy_len == 0 {
            return 0;
        }
        
        let memory = match caller.get_export("memory") {
            Some(wasmi::Extern::Memory(mem)) => mem,
            _ => {
                serial::write_str("[wasm-rt] ERROR: No memory export\n");
                return 0;
            }
        };
        
        let data = memory.data_mut(&mut caller);
        let start = ptr as usize;
        let end = start + copy_len;
        
        if end > data.len() {
            serial::write_str(&alloc::format!(
                "[wasm-rt] ERROR: zos_recv_bytes out of bounds: {}..{} > {}\n",
                start, end, data.len()
            ));
            return 0;
        }
        
        data[start..end].copy_from_slice(&out_data[..copy_len]);
        copy_len as u32
    }).expect("Failed to register zos_recv_bytes");
    
    // zos_yield() - Must trap to actually yield control to scheduler
    // Returns () so when resuming we must provide empty slice, not i32
    linker.func_wrap("env", "zos_yield", |mut caller: Caller<'_, HostState>| -> Result<(), wasmi::Error> {
        let host = caller.data_mut();
        host.yielded = true;
        host.trapped_from_yield = true; // Signal that resume needs empty return value
        // Trigger a trap to return control to the scheduler
        // This matches SYS_YIELD behavior in zos_syscall
        Err(wasmi::Error::from(wasmi::core::TrapCode::OutOfFuel))
    }).expect("Failed to register zos_yield");
    
    // zos_get_pid() -> u32
    linker.func_wrap("env", "zos_get_pid", |caller: Caller<'_, HostState>| -> u32 {
        caller.data().pid as u32
    }).expect("Failed to register zos_get_pid");
}

/// Helper to fill WASM memory with random bytes using RDRAND
fn fill_random_into_wasm(caller: &mut Caller<'_, HostState>, ptr: i32, len: i32) {
    if len <= 0 {
        return;
    }
    
    let memory = match caller.get_export("memory") {
        Some(wasmi::Extern::Memory(mem)) => mem,
        _ => {
            serial::write_str("[wasm-rt] ERROR: fill_random_into_wasm - no memory export\n");
            return;
        }
    };
    
    let len = len as usize;
    let mut random_bytes = alloc::vec![0u8; len];
    
    use crate::x86_64::random::fill_random_bytes;
    if !fill_random_bytes(&mut random_bytes) {
        serial::write_str("[wasm-rt] ERROR: RDRAND failed in fill_random_into_wasm\n");
        return;
    }
    
    let data = memory.data_mut(caller);
    let start = ptr as usize;
    let end = start + len;
    
    if end > data.len() {
        serial::write_str("[wasm-rt] ERROR: fill_random_into_wasm out of bounds\n");
        return;
    }
    
    data[start..end].copy_from_slice(&random_bytes);
}

/// Register wasm-bindgen stub functions
///
/// These functions are required when the WASM module was compiled with wasm-bindgen
/// (e.g., for getrandom's "js" feature). In QEMU mode, we provide no-op stubs since
/// the actual random generation uses the SYS_RANDOM syscall via RDRAND.
fn register_wasm_bindgen_shims(linker: &mut Linker<HostState>) {
    // __wbindgen_placeholder__::__wbindgen_describe is used for type introspection
    // at link time. We provide a no-op since we don't need JS type info.
    linker.func_wrap("__wbindgen_placeholder__", "__wbindgen_describe", |_: i32| {
        // No-op: type description is only used by JS glue
    }).expect("Failed to register __wbindgen_describe");
    
    // __wbindgen_describe_cast - used for casting between JS types
    linker.func_wrap("__wbindgen_placeholder__", "__wbindgen_describe_cast", |_: i32, _: i32| -> i32 {
        1 // Return success
    }).ok();
    
    // __wbindgen_throw - register in __wbindgen_placeholder__ module
    // Newer wasm-bindgen versions look for this here instead of in wbg module
    linker.func_wrap("__wbindgen_placeholder__", "__wbindgen_throw", |_caller: Caller<'_, HostState>, _ptr: i32, _len: i32| {
        serial::write_str("[wasm-rt] __wbindgen_throw called (placeholder module)\n");
    }).ok();
    
    // Mangled variant of __wbindgen_throw used by some wasm-bindgen generated code
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_throw_be289d5034ed271b", |_caller: Caller<'_, HostState>, _ptr: i32, _len: i32| {
        serial::write_str("[wasm-rt] __wbindgen_throw called (mangled variant)\n");
    }).ok();
    
    // __wbindgen_object_drop_ref - drop a JS object reference (no-op in QEMU)
    // Some wasm-bindgen versions place this in __wbindgen_placeholder__ instead of wbg
    linker.func_wrap("__wbindgen_placeholder__", "__wbindgen_object_drop_ref", |_: i32| {
        // No-op: we don't manage JS object references in QEMU mode
    }).ok();
    
    // __wbindgen_object_clone_ref - clone a JS object reference (return same handle in QEMU)
    // Required by identity which uses wasm-bindgen bindings
    linker.func_wrap("__wbindgen_placeholder__", "__wbindgen_object_clone_ref", |handle: i32| -> i32 {
        // Return the same handle since we don't actually manage JS object refs in QEMU
        handle
    }).ok();
    
    // __wbg_crypto_* - return handle to crypto object (for getrandom "js" feature)
    // The hash suffix changes with wasm-bindgen version, so we register known variants
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_crypto_86f2631e91b51511", |_: i32| -> i32 {
        1 // Return handle to "crypto" object
    }).ok();
    
    // __wbg_msCrypto_* - fallback for older browsers (multiple hash variants)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_msCrypto_a61aeb35a24c1329", |_: i32| -> i32 {
        0 // Return null - we don't use msCrypto
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_msCrypto_d562bbe83e0d4b91", |_: i32| -> i32 {
        0 // Return null
    }).ok();
    
    // __wbg_getRandomValues_* - the actual random function (multiple hashes)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_getRandomValues_5f6dd458de83c4f5", |mut caller: Caller<'_, HostState>, _obj: i32, ptr: i32, len: i32| {
        fill_random_into_wasm(&mut caller, ptr, len);
    }).ok();
    // Variant with (i32, i32) -> () signature - ptr and len only
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_getRandomValues_b3f15fcbfabb0f8b", |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
        fill_random_into_wasm(&mut caller, ptr, len);
    }).ok();
    
    // __wbg_randomFillSync_* - Node.js style random (multiple signatures)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_randomFillSync_1b52c8482374c55b", |mut caller: Caller<'_, HostState>, _obj: i32, ptr: i32, len: i32| {
        fill_random_into_wasm(&mut caller, ptr, len);
    }).ok();
    // 2-param variant (ptr, len) -> ()
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_randomFillSync_f8c153b79f285817", |mut caller: Caller<'_, HostState>, ptr: i32, len: i32| {
        fill_random_into_wasm(&mut caller, ptr, len);
    }).ok();
    
    // __wbg_require_* - Node.js require function (multiple variants, different signatures)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_require_0993fe224bf8e202", |_: i32, _: i32| -> i32 {
        0 // Return null
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_require_b74f47fc2d022fd6", || -> i32 {
        0 // Return null - no-argument variant
    }).ok();
    
    // __wbg_newnoargs_* / __wbg_new_no_args_* - create new object with no args
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_newnoargs_19a249f4eceaaac3", |_: i32, _: i32| -> i32 {
        1 // Return a handle
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_new_no_args_1c7c842f08d00ebb", |_: i32, _: i32| -> i32 {
        1 // Return a handle
    }).ok();
    
    // __wbg_call_* - call a JS function (multiple signatures and hashes)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_call_3bfa248576352471", |_: i32, _: i32| -> i32 {
        1 // Return success handle
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_call_389efe28435a9388", |_: i32, _: i32| -> i32 {
        1 // Return success handle
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_call_4708e0c13bdc8e95", |_: i32, _: i32, _: i32| -> i32 {
        1 // Return success handle - 3 arg variant
    }).ok();
    
    // __wbg_self_* - get global self/window object
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_self_7eede1f4488bf346", || -> i32 {
        1 // Return handle to "self"
    }).ok();
    
    // __wbg_window_* - get window object
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_window_b1e7fec70c53b5f2", || -> i32 {
        1 // Return handle to "window"
    }).ok();
    
    // __wbg_globalThis_* - get globalThis object
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_globalThis_82f5c6f01e948e73", || -> i32 {
        1 // Return handle to globalThis
    }).ok();
    
    // __wbg_global_* - get global object
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_global_ef89f731612312c9", || -> i32 {
        1 // Return handle to global
    }).ok();
    
    // __wbg_static_accessor_* - access static properties
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_static_accessor_GLOBAL_88a902d13a557d07", || -> i32 {
        1
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_static_accessor_GLOBAL_12837167ad935116", || -> i32 {
        1
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_static_accessor_GLOBAL_THIS_56578be7e9f832b0", || -> i32 {
        1
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_static_accessor_GLOBAL_THIS_e628e89ab3b1c95f", || -> i32 {
        1
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_static_accessor_SELF_37c5d418e4bf5819", || -> i32 {
        1
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_static_accessor_SELF_a621d3dfbb60d0ce", || -> i32 {
        1
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_static_accessor_WINDOW_f8727f0cf888e0bd", || -> i32 {
        1
    }).ok();
    
    // Node.js environment shims (for getrandom's Node.js fallback path)
    // __wbg_process_* - Node.js process object
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_process_3975fd6c72f520aa", |_: i32| -> i32 {
        0 // Return null - we're not in Node.js
    }).ok();
    
    // __wbg_versions_* - process.versions (multiple hash variants)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_versions_4bc988c2d498fcd5", |_: i32| -> i32 {
        0 // Return null
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_versions_4e31226f5e8dc909", |_: i32| -> i32 {
        0 // Return null
    }).ok();
    
    // __wbg_node_* - check if running in Node.js (multiple hash variants)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_node_cc8c7e99b9fb0652", |_: i32| -> i32 {
        0 // Return null - not Node.js
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_node_e1f24f89a7336c2e", |_: i32| -> i32 {
        0 // Return null - not Node.js
    }).ok();
    
    // __wbg_instanceof_* - instanceof checks
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_instanceof_Uint8Array_17156bcf118086a9", |_: i32| -> i32 {
        1 // Always return true for Uint8Array checks
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_instanceof_ArrayBuffer_e7d53d51371448e2", |_: i32| -> i32 {
        0
    }).ok();
    
    // __wbg_newwithbyteoffsetandlength_* - create typed array view  
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_newwithbyteoffsetandlength_7a23ee1793aa2b5c", |_: i32, _: i32, _: i32| -> i32 {
        1 // Return a handle
    }).ok();
    
    // __wbg_buffer_* - get array buffer
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_buffer_344d9b41efe96da7", |_: i32| -> i32 {
        1
    }).ok();
    
    // __wbg_set_* - set array element
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_set_ec2fcf81bc573fd9", |_: i32, _: i32, _: i32| {
        // No-op
    }).ok();
    
    // __wbg_length_* - get array length (multiple hashes)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_length_a5587d6cd79ab197", |_: i32| -> i32 {
        0
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_length_32ed9a279acd054c", |_: i32| -> i32 {
        0
    }).ok();
    
    // __wbg_new_* - create new object/array (multiple variants)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_new_fec2611eb9180f95", |_: i32| -> i32 {
        1
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_new_with_length_a2c39cbe88fd8ff1", |_: i32| -> i32 {
        1
    }).ok();
    
    // __wbg_subarray_* - get subarray view (multiple hashes)
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_subarray_7f7a652672800851", |_: i32, _: i32, _: i32| -> i32 {
        1
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_subarray_a96e1fef17ed23cb", |_: i32, _: i32, _: i32| -> i32 {
        1
    }).ok();
    
    // Additional wasm-bindgen type checking and utility functions
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_is_string_cd444516edc5b180", |_: i32| -> i32 {
        0 // Not a string
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_string_get_cbb1fadb6e830ce3", |_: i32, _: i32| {
        // No-op for string get
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_is_object_d8a3f80aff05fe8a", |_: i32| -> i32 {
        0
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_is_object_5ae8e5880f2c1fbd", |_: i32| -> i32 {
        0
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_is_undefined_0a3d7be3bab94283", |_: i32| -> i32 {
        1 // Everything is "undefined" in QEMU
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_is_undefined_9e4d92534c42d778", |_: i32| -> i32 {
        1 // Everything is "undefined" in QEMU
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_is_function_31e96c78af4f5ea6", |_: i32| -> i32 {
        0
    }).ok();
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_is_function_0095a73b8b156f76", |_: i32| -> i32 {
        0
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_object_drop_ref_5c5e3f9e45d70ce1", |_: i32| {
        // No-op
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_number_get_b17d8926fb1b2bd5", |_: i32, _: i32| {
        // No-op
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_boolean_get_53ff1fa5e1b9dbfe", |_: i32| -> i32 {
        0
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_memory_6ce83f1eba5b49cb", || -> i32 {
        0
    }).ok();
    
    // Prototype/set/call utilities
    linker.func_wrap("__wbindgen_placeholder__", "__wbg_prototypesetcall_bdcdcc5842e4d77d", |_: i32, _: i32, _: i32| {
        // No-op
    }).ok();
    
    // Error/debug functions
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_debug_string_f96fb2b87c7a0e4c", |_: i32, _: i32| {
        // No-op
    }).ok();
    
    linker.func_wrap("__wbindgen_placeholder__", "__wbg___wbindgen_throw_d481d04d1a3c1c61", |_: i32, _: i32| {
        serial::write_str("[wasm-rt] __wbindgen_throw called\n");
    }).ok();
    
    // __wbindgen_externref_xform__::__wbindgen_externref_table_grow is for externref tables
    linker.func_wrap("__wbindgen_externref_xform__", "__wbindgen_externref_table_grow", |_: i32| -> i32 {
        0 // Return 0 (no growth needed)
    }).ok(); // Optional - may not be present in all modules
    
    // __wbindgen_externref_xform__::__wbindgen_externref_table_set_null
    linker.func_wrap("__wbindgen_externref_xform__", "__wbindgen_externref_table_set_null", |_: i32| {
        // No-op
    }).ok();
    
    // wbg namespace functions for crypto.getRandomValues
    // These are called by getrandom with the "js" feature
    linker.func_wrap("wbg", "__wbg_crypto_1d1f22824a6a080c", |_: i32| -> i32 {
        // Return a "handle" to crypto object - we'll use 1 as a sentinel
        1
    }).ok();
    
    linker.func_wrap("wbg", "__wbg_getRandomValues_37fa2ca9e4e07fab", |mut caller: Caller<'_, HostState>, _obj: i32, ptr: i32, len: i32| {
        // Fill the buffer with random values using RDRAND
        if len <= 0 {
            return;
        }
        
        let memory = match caller.get_export("memory") {
            Some(wasmi::Extern::Memory(mem)) => mem,
            _ => {
                serial::write_str("[wasm-rt] ERROR: __wbg_getRandomValues - no memory export\n");
                return;
            }
        };
        
        // Generate random bytes
        let len = len as usize;
        let mut random_bytes = alloc::vec![0u8; len];
        
        use crate::x86_64::random::fill_random_bytes;
        if !fill_random_bytes(&mut random_bytes) {
            serial::write_str("[wasm-rt] ERROR: RDRAND failed in __wbg_getRandomValues\n");
            return;
        }
        
        // Write to WASM memory
        let data = memory.data_mut(&mut caller);
        let start = ptr as usize;
        let end = start + len;
        
        if end > data.len() {
            serial::write_str("[wasm-rt] ERROR: __wbg_getRandomValues out of bounds\n");
            return;
        }
        
        data[start..end].copy_from_slice(&random_bytes);
    }).ok();
    
    // Common wasm-bindgen exports that might be imported
    linker.func_wrap("wbg", "__wbindgen_object_drop_ref", |_: i32| {
        // No-op: we don't manage JS object references
    }).ok();
    
    linker.func_wrap("wbg", "__wbindgen_throw", |_caller: Caller<'_, HostState>, _ptr: i32, _len: i32| {
        serial::write_str("[wasm-rt] __wbindgen_throw called - JS exception\n");
    }).ok();
    
    linker.func_wrap("wbg", "__wbindgen_is_undefined", |_: i32| -> i32 {
        1 // Everything is "undefined" in QEMU mode
    }).ok();
    
    linker.func_wrap("wbg", "__wbindgen_is_null", |_: i32| -> i32 {
        0
    }).ok();
    
    linker.func_wrap("wbg", "__wbindgen_is_object", |_: i32| -> i32 {
        0
    }).ok();
    
    linker.func_wrap("wbg", "__wbindgen_is_function", |_: i32| -> i32 {
        0
    }).ok();
}
