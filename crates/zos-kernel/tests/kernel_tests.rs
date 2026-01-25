//! Kernel integration tests
//!
//! Tests extracted from kernel_impl.rs for better organization and to
//! reduce kernel LOC count (Invariant 5: Kernel ≤3000 LOC).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU64, Ordering};
use zos_hal::{HalError, NumericProcessHandle, HAL};
use zos_kernel::{
    axiom_check, AxiomError, Capability, CapabilitySpace, ObjectType, Permissions, ProcessId,
    ProcessState, System,
};

// ============================================================================
// Mock HAL for Testing
// ============================================================================

struct MockProcess {
    #[allow(dead_code)]
    name: String,
    alive: bool,
    memory_size: usize,
    pending_messages: Vec<Vec<u8>>,
}

pub struct MockHal {
    time: AtomicU64,
    wallclock: AtomicU64,
    debug_log: RefCell<Vec<String>>,
    random_seed: AtomicU64,
    next_pid: AtomicU64,
    processes: RefCell<BTreeMap<u64, MockProcess>>,
    incoming_messages: RefCell<Vec<(NumericProcessHandle, Vec<u8>)>>,
}

impl MockHal {
    pub fn new() -> Self {
        Self {
            time: AtomicU64::new(0),
            wallclock: AtomicU64::new(1737504000000),
            debug_log: RefCell::new(Vec::new()),
            random_seed: AtomicU64::new(12345),
            next_pid: AtomicU64::new(1),
            processes: RefCell::new(BTreeMap::new()),
            incoming_messages: RefCell::new(Vec::new()),
        }
    }

    #[allow(dead_code)]
    pub fn with_time(nanos: u64) -> Self {
        Self {
            time: AtomicU64::new(nanos),
            wallclock: AtomicU64::new(1737504000000),
            debug_log: RefCell::new(Vec::new()),
            random_seed: AtomicU64::new(12345),
            next_pid: AtomicU64::new(1),
            processes: RefCell::new(BTreeMap::new()),
            incoming_messages: RefCell::new(Vec::new()),
        }
    }
}

impl Default for MockHal {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for MockHal {}
unsafe impl Sync for MockHal {}

impl HAL for MockHal {
    type ProcessHandle = NumericProcessHandle;

    fn spawn_process(&self, name: &str, _binary: &[u8]) -> Result<Self::ProcessHandle, HalError> {
        let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
        let handle = NumericProcessHandle::new(pid);

        let process = MockProcess {
            name: String::from(name),
            alive: true,
            memory_size: 65536,
            pending_messages: Vec::new(),
        };

        self.processes.borrow_mut().insert(pid, process);
        self.debug_log.borrow_mut().push(alloc::format!(
            "[mock-hal] Spawned process '{}' with PID {}",
            name,
            pid
        ));

        Ok(handle)
    }

    fn kill_process(&self, handle: &Self::ProcessHandle) -> Result<(), HalError> {
        let mut processes = self.processes.borrow_mut();
        if let Some(proc) = processes.get_mut(&handle.id()) {
            if proc.alive {
                proc.alive = false;
                self.debug_log.borrow_mut().push(alloc::format!(
                    "[mock-hal] Killed process PID {}",
                    handle.id()
                ));
                Ok(())
            } else {
                Err(HalError::ProcessNotFound)
            }
        } else {
            Err(HalError::ProcessNotFound)
        }
    }

    fn send_to_process(&self, handle: &Self::ProcessHandle, msg: &[u8]) -> Result<(), HalError> {
        let mut processes = self.processes.borrow_mut();
        if let Some(proc) = processes.get_mut(&handle.id()) {
            if proc.alive {
                proc.pending_messages.push(msg.to_vec());
                Ok(())
            } else {
                Err(HalError::ProcessNotFound)
            }
        } else {
            Err(HalError::ProcessNotFound)
        }
    }

    fn is_process_alive(&self, handle: &Self::ProcessHandle) -> bool {
        self.processes
            .borrow()
            .get(&handle.id())
            .map(|p| p.alive)
            .unwrap_or(false)
    }

    fn get_process_memory_size(&self, handle: &Self::ProcessHandle) -> Result<usize, HalError> {
        self.processes
            .borrow()
            .get(&handle.id())
            .filter(|p| p.alive)
            .map(|p| p.memory_size)
            .ok_or(HalError::ProcessNotFound)
    }

    fn allocate(&self, size: usize, _align: usize) -> Result<*mut u8, HalError> {
        let layout =
            core::alloc::Layout::from_size_align(size, 8).map_err(|_| HalError::InvalidArgument)?;
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(HalError::OutOfMemory)
        } else {
            Ok(ptr)
        }
    }

    unsafe fn deallocate(&self, ptr: *mut u8, size: usize, _align: usize) {
        if !ptr.is_null() {
            let layout = core::alloc::Layout::from_size_align(size, 8).unwrap();
            alloc::alloc::dealloc(ptr, layout);
        }
    }

    fn now_nanos(&self) -> u64 {
        self.time.load(Ordering::SeqCst)
    }

    fn wallclock_ms(&self) -> u64 {
        self.wallclock.load(Ordering::SeqCst)
    }

    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError> {
        let mut seed = self.random_seed.load(Ordering::SeqCst);
        for byte in buf.iter_mut() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            *byte = (seed >> 33) as u8;
        }
        self.random_seed.store(seed, Ordering::SeqCst);
        Ok(())
    }

    fn debug_write(&self, msg: &str) {
        self.debug_log.borrow_mut().push(String::from(msg));
    }

    fn poll_messages(&self) -> Vec<(Self::ProcessHandle, Vec<u8>)> {
        let mut messages = self.incoming_messages.borrow_mut();
        messages.drain(..).collect()
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[test]
fn test_kernel_creation() {
    let hal = MockHal::new();
    let kernel = System::new(hal);

    assert_eq!(kernel.list_processes().len(), 0);
    assert_eq!(kernel.list_endpoints().len(), 0);
}

#[test]
fn test_process_registration() {
    let hal = MockHal::new();
    let mut kernel = System::new(hal);

    let pid1 = kernel.register_process("init");
    let pid2 = kernel.register_process("terminal");

    assert_eq!(pid1, ProcessId(1));
    assert_eq!(pid2, ProcessId(2));
    assert_eq!(kernel.list_processes().len(), 2);

    let proc = kernel.get_process(pid1).expect("process should exist");
    assert_eq!(proc.name, "init");
    assert_eq!(proc.state, ProcessState::Running);
}

#[test]
fn test_process_kill() {
    let hal = MockHal::new();
    let mut kernel = System::new(hal);

    let pid = kernel.register_process("test");
    assert!(kernel.get_process(pid).is_some());

    kernel.kill_process(pid);
    assert!(kernel.get_process(pid).is_none());
}

#[test]
fn test_endpoint_creation() {
    let hal = MockHal::new();
    let mut kernel = System::new(hal);

    let pid = kernel.register_process("test");
    let (eid, slot) = kernel
        .create_endpoint(pid)
        .expect("endpoint creation should succeed");

    assert_eq!(eid, zos_kernel::EndpointId(1));
    assert_eq!(slot, 0);

    let endpoints = kernel.list_endpoints();
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].owner, pid);
}

#[test]
fn test_capability_grant() {
    let hal = MockHal::new();
    let mut kernel = System::new(hal);

    let pid1 = kernel.register_process("owner");
    let pid2 = kernel.register_process("recipient");

    let (eid, owner_slot) = kernel.create_endpoint(pid1).unwrap();

    let recipient_slot = kernel
        .grant_capability(
            pid1,
            owner_slot,
            pid2,
            Permissions {
                read: true,
                write: true,
                grant: false,
            },
        )
        .expect("grant should succeed");

    let cap_space = kernel.get_cap_space(pid2).expect("cap space should exist");
    let cap = cap_space
        .get(recipient_slot)
        .expect("capability should exist");

    assert_eq!(cap.object_type, ObjectType::Endpoint);
    assert_eq!(cap.object_id, eid.0);
    assert!(cap.permissions.read);
    assert!(cap.permissions.write);
    assert!(!cap.permissions.grant);
}

#[test]
fn test_ipc_send_receive() {
    let hal = MockHal::new();
    let mut kernel = System::new(hal);

    let sender_pid = kernel.register_process("sender");
    let receiver_pid = kernel.register_process("receiver");

    let (_, receiver_slot) = kernel.create_endpoint(receiver_pid).unwrap();

    let sender_slot = kernel
        .grant_capability(
            receiver_pid,
            receiver_slot,
            sender_pid,
            Permissions {
                read: false,
                write: true,
                grant: false,
            },
        )
        .unwrap();

    let data = b"hello world".to_vec();
    kernel
        .ipc_send(sender_pid, sender_slot, 42, data.clone())
        .expect("send should succeed");

    let ep = kernel
        .get_endpoint(zos_kernel::EndpointId(1))
        .expect("endpoint should exist");
    assert_eq!(ep.pending_messages.len(), 1);

    let msg = kernel
        .ipc_receive(receiver_pid, receiver_slot)
        .expect("receive should succeed")
        .expect("message should be present");

    assert_eq!(msg.from, sender_pid);
    assert_eq!(msg.tag, 42);
    assert_eq!(msg.data, b"hello world");
}

#[test]
fn test_axiom_check_valid_capability() {
    let mut cspace = CapabilitySpace::new();
    let cap = Capability {
        id: 1,
        object_type: ObjectType::Endpoint,
        object_id: 42,
        permissions: Permissions::full(),
        generation: 0,
        expires_at: 0,
    };
    let slot = cspace.insert(cap);

    let result = axiom_check(
        &cspace,
        slot,
        &Permissions::read_only(),
        Some(ObjectType::Endpoint),
        0,
    );

    assert!(result.is_ok());
    let cap = result.unwrap();
    assert_eq!(cap.object_id, 42);
}

#[test]
fn test_axiom_check_invalid_slot() {
    let cspace = CapabilitySpace::new();

    let result = axiom_check(&cspace, 999, &Permissions::read_only(), None, 0);

    assert!(matches!(result, Err(AxiomError::InvalidSlot)));
}

#[test]
fn test_axiom_check_expired_capability() {
    let mut cspace = CapabilitySpace::new();
    let cap = Capability {
        id: 1,
        object_type: ObjectType::Endpoint,
        object_id: 42,
        permissions: Permissions::full(),
        generation: 0,
        expires_at: 1000,
    };
    let slot = cspace.insert(cap);

    let result = axiom_check(&cspace, slot, &Permissions::read_only(), None, 2000);

    assert!(matches!(result, Err(AxiomError::Expired)));
}

// ============================================================================
// Init-Driven Spawn Protocol Tests
// ============================================================================

/// Test that SYS_REGISTER_PROCESS (0x14) is Init-only.
///
/// This syscall should only succeed when called by Init (PID 1).
/// Other processes should receive an error.
#[test]
fn test_sys_register_process_init_only() {
    let hal = MockHal::new();
    let mut kernel = System::new(hal);

    // Create Init (PID 1)
    let init_pid = kernel.register_process_with_pid(ProcessId(1), "init");
    assert_eq!(init_pid, ProcessId(1));

    // Create a regular process (PID 2)
    let other_pid = kernel.register_process("other");
    assert_eq!(other_pid, ProcessId(2));

    // SYS_REGISTER_PROCESS = 0x14
    const SYS_REGISTER_PROCESS: u32 = 0x14;

    // Init (PID 1) should be able to register a new process
    let (result, _rich, _data) =
        kernel.process_syscall(init_pid, SYS_REGISTER_PROCESS, [0, 0, 0, 0], b"test_proc");
    assert!(
        result >= 0,
        "Init should be able to register processes, got {}",
        result
    );
    let new_pid = result as u64;
    assert!(new_pid > 0, "Should return a valid PID");

    // Verify the process was created
    let proc = kernel
        .get_process(ProcessId(new_pid))
        .expect("new process should exist");
    assert_eq!(proc.name, "test_proc");

    // Other processes should NOT be able to register processes
    let (result2, _rich2, _data2) =
        kernel.process_syscall(other_pid, SYS_REGISTER_PROCESS, [0, 0, 0, 0], b"malicious");
    assert_eq!(
        result2, -1,
        "Non-Init processes should not be able to register processes"
    );
}

/// Test that SYS_CREATE_ENDPOINT_FOR (0x15) is Init-only.
///
/// This syscall should only succeed when called by Init (PID 1).
/// Other processes should receive an error.
#[test]
fn test_sys_create_endpoint_for_init_only() {
    let hal = MockHal::new();
    let mut kernel = System::new(hal);

    // Create Init (PID 1)
    let init_pid = kernel.register_process_with_pid(ProcessId(1), "init");
    assert_eq!(init_pid, ProcessId(1));

    // Create a target process (PID 2)
    let target_pid = kernel.register_process("target");
    assert_eq!(target_pid, ProcessId(2));

    // Create another process (PID 3) that will try to abuse the syscall
    let attacker_pid = kernel.register_process("attacker");
    assert_eq!(attacker_pid, ProcessId(3));

    // SYS_CREATE_ENDPOINT_FOR = 0x15
    const SYS_CREATE_ENDPOINT_FOR: u32 = 0x15;

    // Init (PID 1) should be able to create an endpoint for another process
    let (result, _rich, _data) = kernel.process_syscall(
        init_pid,
        SYS_CREATE_ENDPOINT_FOR,
        [target_pid.0 as u32, 0, 0, 0],
        &[],
    );
    assert!(
        result >= 0,
        "Init should be able to create endpoints, got {}",
        result
    );

    // Verify the endpoint was created
    let endpoints = kernel.list_endpoints();
    assert_eq!(endpoints.len(), 1, "One endpoint should exist");
    assert_eq!(
        endpoints[0].owner, target_pid,
        "Endpoint should be owned by target"
    );

    // Other processes should NOT be able to create endpoints for others
    let (result2, _rich2, _data2) = kernel.process_syscall(
        attacker_pid,
        SYS_CREATE_ENDPOINT_FOR,
        [target_pid.0 as u32, 0, 0, 0],
        &[],
    );
    assert_eq!(
        result2, -1,
        "Non-Init processes should not be able to create endpoints for others"
    );
}
