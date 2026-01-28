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
    /// Last syscall result code
    pub syscall_result: u32,
    /// Process has yielded
    pub yielded: bool,
    /// Pending syscall to dispatch
    pub pending_syscall: Option<PendingSyscallInfo>,
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
            yielded: false,
            pending_syscall: None,
        }
    }
    
    /// Set syscall result (called by kernel after processing syscall)
    pub fn set_syscall_result(&mut self, result: u32, data: &[u8]) {
        self.syscall_result = result;
        self.syscall_out_buffer.clear();
        self.syscall_out_buffer.extend_from_slice(data);
    }
    
    /// Clear syscall buffers
    pub fn clear_syscall_buffers(&mut self) {
        self.syscall_in_buffer.clear();
        self.syscall_out_buffer.clear();
        self.pending_syscall = None;
    }
}

// Syscall numbers (from zos-ipc)
const SYS_NOP: u32 = 0x00;
const SYS_DEBUG: u32 = 0x01;
const SYS_TIME: u32 = 0x02;
const SYS_GETPID: u32 = 0x03;
const SYS_YIELD: u32 = 0x04;
const SYS_CONSOLE_WRITE: u32 = 0x07;

/// Register host functions with the linker
pub fn register_host_functions(linker: &mut Linker<HostState>) {
    // zos_syscall(syscall_num: u32, arg1: u32, arg2: u32, arg3: u32) -> u32
    linker.func_wrap("env", "zos_syscall", |mut caller: Caller<'_, HostState>, syscall_num: u32, arg1: u32, arg2: u32, arg3: u32| -> u32 {
        let host = caller.data_mut();
        let pid = host.pid;
        
        // Handle simple syscalls directly without going through kernel
        match syscall_num {
            SYS_NOP => return 0,
            
            SYS_DEBUG | SYS_CONSOLE_WRITE => {
                // Print debug/console output directly to serial
                let data = core::mem::take(&mut host.syscall_in_buffer);
                if let Ok(text) = core::str::from_utf8(&data) {
                    serial::write_str(text);
                }
                return 0;
            }
            
            SYS_GETPID => return pid as u32,
            
            SYS_YIELD => {
                // Mark as yielded - caller will check this
                host.yielded = true;
                return 0;
            }
            
            _ => {
                // Other syscalls need kernel processing
                // Don't log SYS_RECV (0x41) to avoid spamming console during idle loop
                if syscall_num != 0x41 {
                    serial::write_str(&alloc::format!(
                        "[wasm-rt] PID {} syscall: num=0x{:x}, args=[{}, {}, {}]\n",
                        pid, syscall_num, arg1, arg2, arg3
                    ));
                }
            }
        }
        
        // Store pending syscall for the kernel to process
        host.pending_syscall = Some(PendingSyscallInfo {
            syscall_num,
            args: [arg1, arg2, arg3],
            data: core::mem::take(&mut host.syscall_in_buffer),
        });
        
        // Return the stored result (kernel should have set this from previous syscall)
        host.syscall_result
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
    
    // zos_yield()
    linker.func_wrap("env", "zos_yield", |mut caller: Caller<'_, HostState>| {
        let host = caller.data_mut();
        host.yielded = true;
        // In a full implementation, this would trap to return control to scheduler
    }).expect("Failed to register zos_yield");
    
    // zos_get_pid() -> u32
    linker.func_wrap("env", "zos_get_pid", |caller: Caller<'_, HostState>| -> u32 {
        caller.data().pid as u32
    }).expect("Failed to register zos_get_pid");
}
