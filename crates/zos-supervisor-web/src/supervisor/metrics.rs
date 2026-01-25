//! Supervisor metrics and JSON APIs
//!
//! Provides JSON-serialized metrics and process data for the dashboard.

use wasm_bindgen::prelude::*;
use zos_hal::HAL;
use zos_kernel::ProcessId;

use super::Supervisor;
use crate::axiom;

#[wasm_bindgen]
impl Supervisor {
    /// Get system uptime in milliseconds
    #[wasm_bindgen]
    pub fn get_uptime_ms(&self) -> f64 {
        self.system.uptime_nanos() as f64 / 1_000_000.0
    }

    /// Get wall-clock time in milliseconds since Unix epoch
    ///
    /// This is real time-of-day (not monotonic) from the HAL.
    /// Used by the DateTime component in the taskbar.
    #[wasm_bindgen]
    pub fn get_wallclock_ms(&self) -> f64 {
        self.system.hal().wallclock_ms() as f64
    }

    /// Get process count (including supervisor)
    #[wasm_bindgen]
    pub fn get_process_count(&self) -> usize {
        self.system.list_processes().len()
    }

    /// Get total memory usage in bytes
    #[wasm_bindgen]
    pub fn get_total_memory(&self) -> usize {
        self.system.total_memory()
    }

    /// Get endpoint count
    #[wasm_bindgen]
    pub fn get_endpoint_count(&self) -> usize {
        self.system.list_endpoints().len()
    }

    /// Get total pending IPC messages
    #[wasm_bindgen]
    pub fn get_pending_messages(&self) -> usize {
        self.system.total_pending_messages()
    }

    /// Get total IPC message count since boot
    #[wasm_bindgen]
    pub fn get_total_ipc_messages(&self) -> f64 {
        self.system.get_system_metrics().total_ipc_messages as f64
    }

    /// Get Axiom statistics for dashboard
    #[wasm_bindgen]
    pub fn get_axiom_stats_json(&self) -> String {
        let commitlog = self.system.commitlog();
        let syslog = self.system.syslog();
        let commits_in_memory = commitlog.len();
        let commit_seq = commitlog.current_seq();
        let events_in_memory = syslog.len();
        let persisted = self.last_persisted_axiom_seq;
        let pending = (commit_seq + 1).saturating_sub(persisted);

        format!(
            r#"{{"commits":{},"events":{},"commit_seq":{},"persisted":{},"pending":{},"storage_ready":{}}}"#,
            commits_in_memory,
            events_in_memory,
            commit_seq,
            persisted,
            pending,
            self.axiom_storage_ready
        )
    }

    /// Get recent CommitLog entries as JSON for display
    #[wasm_bindgen]
    pub fn get_commitlog_json(&self, count: usize) -> String {
        let commitlog = self.system.commitlog();
        let commits = commitlog.get_recent(count);

        let mut json = String::from("[");
        for (i, commit) in commits.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }

            let commit_type = axiom::commit_type_short(&commit.commit_type);
            let details = axiom::commit_type_to_string(&commit.commit_type);

            json.push_str(&format!(
                r#"{{"seq":{},"timestamp":{},"type":"{}","details":"{}"}}"#,
                commit.seq,
                commit.timestamp,
                commit_type,
                details.replace('"', "'")
            ));
        }
        json.push(']');
        json
    }

    /// Get recent SysLog entries as JSON for display
    #[wasm_bindgen]
    pub fn get_syslog_json(&self, count: usize) -> String {
        let syslog = self.system.syslog();
        let events = syslog.get_recent(count);

        let mut json = String::from("[");
        for (i, event) in events.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }

            let (event_type, details) = match &event.event_type {
                zos_kernel::SysEventType::Request { syscall_num, args } => (
                    "Request",
                    format!(
                        "syscall={:#x} args=[{},{},{},{}]",
                        syscall_num, args[0], args[1], args[2], args[3]
                    ),
                ),
                zos_kernel::SysEventType::Response { request_id, result } => {
                    ("Response", format!("req={} result={}", request_id, result))
                }
            };

            json.push_str(&format!(
                r#"{{"id":{},"sender":{},"timestamp":{},"type":"{}","details":"{}"}}"#,
                event.id, event.sender, event.timestamp, event_type, details
            ));
        }
        json.push(']');
        json
    }

    /// Get process list as JSON for dashboard
    ///
    /// Includes all processes including PID 0 (supervisor), which runs on the
    /// main thread and manages kernel operations.
    #[wasm_bindgen]
    pub fn get_process_list_json(&self) -> String {
        let processes: Vec<_> = self
            .system
            .list_processes()
            .iter()
            .map(|(pid, proc)| {
                let state = match proc.state {
                    zos_kernel::ProcessState::Running => "Running",
                    zos_kernel::ProcessState::Blocked => "Blocked",
                    zos_kernel::ProcessState::Zombie => "Zombie",
                };
                let worker_id = self.system.hal().get_worker_id(pid.0);
                serde_json::json!({
                    "pid": pid.0,
                    "name": proc.name,
                    "state": state,
                    "memory": proc.metrics.memory_size,
                    "ipc_sent": proc.metrics.ipc_sent,
                    "ipc_received": proc.metrics.ipc_received,
                    "syscalls": proc.metrics.syscall_count,
                    "worker_id": worker_id
                })
            })
            .collect();
        serde_json::to_string(&processes).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get capabilities for a specific process as JSON
    #[wasm_bindgen]
    pub fn get_process_capabilities_json(&self, pid: u64) -> String {
        let process_id = ProcessId(pid);
        if let Some(cap_space) = self.system.get_cap_space(process_id) {
            let caps: Vec<_> = cap_space
                .list()
                .iter()
                .map(|(slot, cap)| {
                    let type_str = match cap.object_type {
                        zos_kernel::ObjectType::Endpoint => "Endpoint",
                        zos_kernel::ObjectType::Process => "Process",
                        zos_kernel::ObjectType::Memory => "Memory",
                        zos_kernel::ObjectType::Irq => "IRQ",
                        zos_kernel::ObjectType::IoPort => "IoPort",
                        zos_kernel::ObjectType::Console => "Console",
                    };
                    serde_json::json!({
                        "slot": slot,
                        "objectType": type_str,
                        "permissions": {
                            "read": cap.permissions.read,
                            "write": cap.permissions.write,
                            "grant": cap.permissions.grant
                        },
                        "objectId": cap.object_id
                    })
                })
                .collect();
            serde_json::to_string(&caps).unwrap_or_else(|_| "[]".to_string())
        } else {
            "[]".to_string()
        }
    }

    /// Get all processes with their capabilities as JSON (including supervisor)
    #[wasm_bindgen]
    pub fn get_processes_with_capabilities_json(&self) -> String {
        let processes: Vec<_> = self
            .system
            .list_processes()
            .iter()
            .map(|(pid, proc)| {
                let state = match proc.state {
                    zos_kernel::ProcessState::Running => "Running",
                    zos_kernel::ProcessState::Blocked => "Blocked",
                    zos_kernel::ProcessState::Zombie => "Zombie",
                };

                let caps: Vec<serde_json::Value> =
                    if let Some(cap_space) = self.system.get_cap_space(*pid) {
                        cap_space
                            .list()
                            .iter()
                            .map(|(slot, cap)| {
                                let type_str = match cap.object_type {
                                    zos_kernel::ObjectType::Endpoint => "Endpoint",
                                    zos_kernel::ObjectType::Process => "Process",
                                    zos_kernel::ObjectType::Memory => "Memory",
                                    zos_kernel::ObjectType::Irq => "IRQ",
                                    zos_kernel::ObjectType::IoPort => "IoPort",
                                    zos_kernel::ObjectType::Console => "Console",
                                };
                                serde_json::json!({
                                    "slot": slot,
                                    "objectType": type_str,
                                    "permissions": {
                                        "read": cap.permissions.read,
                                        "write": cap.permissions.write,
                                        "grant": cap.permissions.grant
                                    }
                                })
                            })
                            .collect()
                    } else {
                        vec![]
                    };

                serde_json::json!({
                    "pid": pid.0,
                    "name": proc.name,
                    "state": state,
                    "capabilities": caps
                })
            })
            .collect();
        serde_json::to_string(&processes).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get endpoint list as JSON for dashboard
    #[wasm_bindgen]
    pub fn get_endpoint_list_json(&self) -> String {
        let endpoints: Vec<_> = self
            .system
            .list_endpoints()
            .iter()
            .map(|ep| {
                let detail = self.system.get_endpoint_detail(ep.id);
                serde_json::json!({
                    "id": ep.id.0,
                    "owner": ep.owner.0,
                    "queue": ep.queue_depth,
                    "total_msgs": detail.as_ref().map(|d| d.metrics.total_messages).unwrap_or(0),
                    "total_bytes": detail.as_ref().map(|d| d.metrics.total_bytes).unwrap_or(0)
                })
            })
            .collect();
        serde_json::to_string(&endpoints).unwrap_or_else(|_| "[]".to_string())
    }

    /// Get recent IPC traffic as JSON for dashboard
    ///
    /// NOTE: IPC traffic logging has been moved out of the kernel as part of
    /// the invariant compliance refactor. Traffic can be reconstructed from
    /// the SysLog (kernel.syslog()) which records all syscalls including IPC.
    /// A userspace Monitor service should be used for detailed traffic analysis.
    #[wasm_bindgen]
    pub fn get_ipc_traffic_json(&self, _count: usize) -> String {
        // Return empty array - traffic monitoring moved to userspace/SysLog
        "[]".to_string()
    }

    /// Get system metrics as JSON for dashboard
    #[wasm_bindgen]
    pub fn get_system_metrics_json(&self) -> String {
        let m = self.system.get_system_metrics();
        serde_json::to_string(&serde_json::json!({
            "process_count": m.process_count,
            "total_memory": m.total_memory,
            "endpoint_count": m.endpoint_count,
            "total_pending_messages": m.total_pending_messages,
            "total_ipc_messages": m.total_ipc_messages,
            "uptime_ns": m.uptime_ns
        }))
        .unwrap_or_else(|_| "{}".to_string())
    }
}
