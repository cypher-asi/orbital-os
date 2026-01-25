//! Legacy Worker Message Handling
//!
//! This module handles the legacy postMessage-based worker communication path.
//! New processes use SharedArrayBuffer-based syscalls (polled via poll_syscalls).

use wasm_bindgen::prelude::*;
use zos_kernel::ProcessId;

use crate::syscall;
use crate::worker::{WorkerMessage, WorkerMessageType};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

impl super::Supervisor {
    /// Internal handler for processing worker messages.
    pub(super) fn process_worker_messages_internal(&mut self) -> usize {
        const MAX_MESSAGES_PER_BATCH: usize = 5000;

        let incoming = self.system.hal().incoming_messages();
        let messages: Vec<WorkerMessage> = {
            if let Ok(mut queue) = incoming.lock() {
                let take_count = queue.len().min(MAX_MESSAGES_PER_BATCH);
                queue.drain(..take_count).collect()
            } else {
                return 0;
            }
        };

        let count = messages.len();

        for msg in messages {
            match msg.msg_type {
                WorkerMessageType::Ready { memory_size } => {
                    self.system
                        .hal()
                        .update_process_memory(msg.pid, memory_size);
                    log(&format!(
                        "[supervisor] Worker {} ready, memory: {} bytes",
                        msg.pid, memory_size
                    ));
                }
                WorkerMessageType::Syscall { syscall_num, args } => {
                    self.handle_worker_syscall(msg.pid, syscall_num, args, &msg.data);
                }
                WorkerMessageType::Error { ref message } => {
                    log(&format!(
                        "[supervisor] Worker {} error: {}",
                        msg.pid, message
                    ));
                }
                WorkerMessageType::Terminated => {
                    log(&format!("[supervisor] Worker {} terminated", msg.pid));
                    let pid = ProcessId(msg.pid);
                    if self.system.get_process(pid).is_some() {
                        // Route through Init for proper auditing
                        self.cleanup_process_state(msg.pid);
                        if msg.pid == 1 {
                            self.kill_process_direct(pid);
                        } else {
                            self.kill_process_via_init(pid);
                        }
                    }
                }
                WorkerMessageType::MemoryUpdate { memory_size } => {
                    self.system
                        .hal()
                        .update_process_memory(msg.pid, memory_size);
                    let pid = ProcessId(msg.pid);
                    self.system.update_process_memory(pid, memory_size);
                }
                WorkerMessageType::Yield => {
                    // Worker yielded - nothing to do
                }
            }
        }

        count
    }

    /// Handle a syscall from a Worker process (legacy postMessage path).
    fn handle_worker_syscall(&mut self, pid: u64, syscall_num: u32, args: [u32; 3], data: &[u8]) {
        let process_id = ProcessId(pid);

        if self.system.get_process(process_id).is_none() {
            log(&format!(
                "[supervisor] Syscall from unknown process {}",
                pid
            ));
            return;
        }

        let result = self.dispatch_legacy_syscall(process_id, syscall_num, args, data);
        syscall::send_syscall_result(self.system.hal(), pid, result);
    }

    fn dispatch_legacy_syscall(
        &mut self,
        process_id: ProcessId,
        syscall_num: u32,
        args: [u32; 3],
        data: &[u8],
    ) -> zos_kernel::SyscallResult {
        use zos_kernel::{Syscall, SyscallResult};

        match syscall_num {
            0 => SyscallResult::Ok(0), // NOP
            1 => self.handle_legacy_sys_debug(process_id, data),
            2 => self
                .system
                .handle_syscall(process_id, Syscall::CreateEndpoint),
            3 => self.handle_legacy_send(process_id, args, data),
            4 => self.handle_legacy_receive(process_id, args),
            5 => self.system.handle_syscall(process_id, Syscall::ListCaps),
            6 => self
                .system
                .handle_syscall(process_id, Syscall::ListProcesses),
            7 => self.handle_legacy_exit(process_id, args[0]),
            8 => self.system.handle_syscall(process_id, Syscall::GetTime),
            9 => SyscallResult::Ok(0), // SYS_YIELD
            _ => {
                log(&format!(
                    "[supervisor] Unknown syscall {} from process {}",
                    syscall_num, process_id.0
                ));
                SyscallResult::Err(zos_kernel::KernelError::PermissionDenied)
            }
        }
    }

    fn handle_legacy_sys_debug(
        &mut self,
        pid: ProcessId,
        data: &[u8],
    ) -> zos_kernel::SyscallResult {
        let args4 = [0u32, 0, 0, 0];
        let (_result, rich_result, _data) = self.system.process_syscall(pid, 0x01, args4, data);

        if let Ok(s) = std::str::from_utf8(data) {
            if let Some(service_name) = s.strip_prefix("INIT:SPAWN:") {
                log(&format!(
                    "[supervisor] Init requesting spawn of '{}'",
                    service_name
                ));
                self.request_spawn(service_name, service_name);
            } else if s.starts_with("INIT:GRANT:") {
                syscall::handle_init_grant(&mut self.system, s);
            } else if s.starts_with("INIT:REVOKE:") {
                syscall::handle_init_revoke(&mut self.system, s);
            } else if let Some(init_msg) = s.strip_prefix("INIT:") {
                log(&format!("[init] {}", init_msg));
            } else {
                log(&format!("[process {}] {}", pid.0, s));
            }
        }

        rich_result
    }

    fn handle_legacy_send(
        &mut self,
        process_id: ProcessId,
        args: [u32; 3],
        data: &[u8],
    ) -> zos_kernel::SyscallResult {
        let slot = args[0];
        let tag = args[1];
        let syscall = zos_kernel::Syscall::Send {
            endpoint_slot: slot,
            tag,
            data: data.to_vec(),
        };
        self.system.handle_syscall(process_id, syscall)
    }

    fn handle_legacy_receive(
        &mut self,
        process_id: ProcessId,
        args: [u32; 3],
    ) -> zos_kernel::SyscallResult {
        let slot = args[0];
        let syscall = zos_kernel::Syscall::Receive {
            endpoint_slot: slot,
        };
        self.system.handle_syscall(process_id, syscall)
    }

    fn handle_legacy_exit(
        &mut self,
        process_id: ProcessId,
        exit_code: u32,
    ) -> zos_kernel::SyscallResult {
        let result = self.handle_sys_exit(process_id, exit_code);
        zos_kernel::SyscallResult::Ok(result as u64)
    }
}
