//! Mock HAL implementation for testing Orbital OS
//!
//! This provides a mock implementation of the HAL trait that can be used
//! for unit testing the kernel without requiring browser or hardware.

#![no_std]
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::sync::atomic::{AtomicU64, Ordering};
use orbital_hal::{HalError, NumericProcessHandle, HAL};

/// Mock HAL for unit testing
///
/// Provides simulated process spawning, time, memory, and message passing
/// for testing kernel logic without a real platform.
pub struct MockHal {
    /// Simulated time in nanoseconds
    time: AtomicU64,
    /// Captured debug messages
    debug_log: RefCell<Vec<String>>,
    /// Random seed for deterministic testing
    random_seed: AtomicU64,
    /// Next process ID to assign
    next_pid: AtomicU64,
    /// Simulated processes (pid -> (name, alive, memory_size, pending_messages))
    processes: RefCell<BTreeMap<u64, MockProcess>>,
    /// Messages from processes to supervisor
    incoming_messages: RefCell<Vec<(NumericProcessHandle, Vec<u8>)>>,
}

/// Simulated process state
struct MockProcess {
    name: String,
    alive: bool,
    memory_size: usize,
    /// Messages pending delivery to this process
    pending_messages: Vec<Vec<u8>>,
}

impl MockHal {
    /// Create a new mock HAL
    pub fn new() -> Self {
        Self {
            time: AtomicU64::new(0),
            debug_log: RefCell::new(Vec::new()),
            random_seed: AtomicU64::new(12345), // Deterministic seed
            next_pid: AtomicU64::new(1),
            processes: RefCell::new(BTreeMap::new()),
            incoming_messages: RefCell::new(Vec::new()),
        }
    }

    /// Create a mock HAL with a specific starting time
    pub fn with_time(nanos: u64) -> Self {
        Self {
            time: AtomicU64::new(nanos),
            debug_log: RefCell::new(Vec::new()),
            random_seed: AtomicU64::new(12345),
            next_pid: AtomicU64::new(1),
            processes: RefCell::new(BTreeMap::new()),
            incoming_messages: RefCell::new(Vec::new()),
        }
    }

    /// Advance the simulated time by the given duration
    pub fn advance_time(&self, nanos: u64) {
        self.time.fetch_add(nanos, Ordering::SeqCst);
    }

    /// Set the simulated time to a specific value
    pub fn set_time(&self, nanos: u64) {
        self.time.store(nanos, Ordering::SeqCst);
    }

    /// Get all captured debug messages
    pub fn get_debug_log(&self) -> Vec<String> {
        self.debug_log.borrow().clone()
    }

    /// Clear the debug log
    pub fn clear_debug_log(&self) {
        self.debug_log.borrow_mut().clear();
    }

    /// Check if a specific message was logged
    pub fn has_log_containing(&self, substr: &str) -> bool {
        self.debug_log
            .borrow()
            .iter()
            .any(|msg| msg.contains(substr))
    }

    /// Get the number of debug messages
    pub fn debug_log_count(&self) -> usize {
        self.debug_log.borrow().len()
    }

    /// Set the random seed for deterministic testing
    pub fn set_random_seed(&self, seed: u64) {
        self.random_seed.store(seed, Ordering::SeqCst);
    }

    /// Get the number of spawned processes
    pub fn process_count(&self) -> usize {
        self.processes.borrow().len()
    }

    /// Get the number of alive processes
    pub fn alive_process_count(&self) -> usize {
        self.processes.borrow().values().filter(|p| p.alive).count()
    }

    /// Simulate a message arriving from a process
    pub fn simulate_message_from_process(&self, pid: u64, msg: Vec<u8>) {
        self.incoming_messages
            .borrow_mut()
            .push((NumericProcessHandle::new(pid), msg));
    }

    /// Get pending messages for a process (for testing)
    pub fn get_pending_messages(&self, pid: u64) -> Vec<Vec<u8>> {
        self.processes
            .borrow()
            .get(&pid)
            .map(|p| p.pending_messages.clone())
            .unwrap_or_default()
    }

    /// Set memory size for a process (for testing)
    pub fn set_process_memory_size(&self, pid: u64, size: usize) {
        if let Some(proc) = self.processes.borrow_mut().get_mut(&pid) {
            proc.memory_size = size;
        }
    }
}

impl Default for MockHal {
    fn default() -> Self {
        Self::new()
    }
}

// MockHal is Send + Sync because it uses atomic operations and RefCell
// is only accessed in single-threaded test contexts
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
            memory_size: 65536, // Default 1 WASM page (64KB)
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
        // In mock, we use the global allocator
        let layout =
            core::alloc::Layout::from_size_align(size, 8).map_err(|_| HalError::InvalidArgument)?;
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        if ptr.is_null() {
            Err(HalError::OutOfMemory)
        } else {
            Ok(ptr)
        }
    }

    fn deallocate(&self, ptr: *mut u8, size: usize, _align: usize) {
        if !ptr.is_null() {
            let layout = core::alloc::Layout::from_size_align(size, 8).unwrap();
            unsafe { alloc::alloc::dealloc(ptr, layout) };
        }
    }

    fn now_nanos(&self) -> u64 {
        self.time.load(Ordering::SeqCst)
    }

    fn random_bytes(&self, buf: &mut [u8]) -> Result<(), HalError> {
        // Simple LCG for deterministic "random" bytes in tests
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
        let result = messages.drain(..).collect();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn test_mock_hal_time() {
        let hal = MockHal::new();
        assert_eq!(hal.now_nanos(), 0);

        hal.advance_time(1_000_000_000); // 1 second
        assert_eq!(hal.now_nanos(), 1_000_000_000);

        hal.advance_time(500_000_000); // 0.5 seconds
        assert_eq!(hal.now_nanos(), 1_500_000_000);
    }

    #[test]
    fn test_mock_hal_debug_log() {
        let hal = MockHal::new();

        hal.debug_write("Hello");
        hal.debug_write("World");

        let log = hal.get_debug_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0], "Hello");
        assert_eq!(log[1], "World");

        assert!(hal.has_log_containing("Hello"));
        assert!(!hal.has_log_containing("Foo"));
    }

    #[test]
    fn test_mock_hal_spawn_process() {
        let hal = MockHal::new();

        let handle1 = hal.spawn_process("test1", &[]).unwrap();
        let handle2 = hal.spawn_process("test2", &[]).unwrap();

        assert_eq!(handle1.id(), 1);
        assert_eq!(handle2.id(), 2);
        assert_eq!(hal.process_count(), 2);
        assert!(hal.is_process_alive(&handle1));
        assert!(hal.is_process_alive(&handle2));
    }

    #[test]
    fn test_mock_hal_kill_process() {
        let hal = MockHal::new();

        let handle = hal.spawn_process("test", &[]).unwrap();
        assert!(hal.is_process_alive(&handle));

        hal.kill_process(&handle).unwrap();
        assert!(!hal.is_process_alive(&handle));

        // Killing again should fail
        assert_eq!(hal.kill_process(&handle), Err(HalError::ProcessNotFound));
    }

    #[test]
    fn test_mock_hal_send_message() {
        let hal = MockHal::new();

        let handle = hal.spawn_process("test", &[]).unwrap();
        hal.send_to_process(&handle, b"hello").unwrap();
        hal.send_to_process(&handle, b"world").unwrap();

        let messages = hal.get_pending_messages(handle.id());
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0], b"hello");
        assert_eq!(messages[1], b"world");
    }

    #[test]
    fn test_mock_hal_send_to_dead_process() {
        let hal = MockHal::new();

        let handle = hal.spawn_process("test", &[]).unwrap();
        hal.kill_process(&handle).unwrap();

        assert_eq!(
            hal.send_to_process(&handle, b"hello"),
            Err(HalError::ProcessNotFound)
        );
    }

    #[test]
    fn test_mock_hal_memory_size() {
        let hal = MockHal::new();

        let handle = hal.spawn_process("test", &[]).unwrap();
        assert_eq!(hal.get_process_memory_size(&handle).unwrap(), 65536);

        hal.set_process_memory_size(handle.id(), 131072);
        assert_eq!(hal.get_process_memory_size(&handle).unwrap(), 131072);
    }

    #[test]
    fn test_mock_hal_random_bytes() {
        let hal = MockHal::new();
        hal.set_random_seed(42);

        let mut buf1 = [0u8; 8];
        let mut buf2 = [0u8; 8];

        hal.random_bytes(&mut buf1).unwrap();

        // Reset seed to get same sequence
        hal.set_random_seed(42);
        hal.random_bytes(&mut buf2).unwrap();

        assert_eq!(buf1, buf2); // Deterministic with same seed
    }

    #[test]
    fn test_mock_hal_poll_messages() {
        let hal = MockHal::new();

        hal.simulate_message_from_process(1, vec![1, 2, 3]);
        hal.simulate_message_from_process(2, vec![4, 5, 6]);

        let messages = hal.poll_messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].0.id(), 1);
        assert_eq!(messages[0].1, vec![1, 2, 3]);

        // Poll again should be empty
        let messages = hal.poll_messages();
        assert_eq!(messages.len(), 0);
    }
}
