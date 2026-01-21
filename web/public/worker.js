/**
 * Orbital OS Process Worker - Minimal Bootstrap
 * 
 * This script is a thin shim that:
 * 1. Creates shared WASM memory (for syscall mailbox + process data)
 * 2. Provides syscall imports that use SharedArrayBuffer + Atomics
 * 3. Instantiates the WASM binary
 * 4. Reports memory to supervisor for syscall polling
 * 5. Calls _start() - WASM runs forever using native atomics
 * 
 * Mailbox Layout (at offset 0 in shared memory):
 * | Offset | Size | Field                              |
 * |--------|------|------------------------------------|
 * | 0      | 4    | status (0=idle, 1=pending, 2=ready)|
 * | 4      | 4    | syscall_num                        |
 * | 8      | 4    | arg0                               |
 * | 12     | 4    | arg1                               |
 * | 16     | 4    | arg2                               |
 * | 20     | 4    | result                             |
 * | 24     | 4    | data_len                           |
 * | 28     | 4068 | data buffer                        |
 * | 56     | 4    | pid (stored by supervisor)         |
 */

// Capture the worker's memory context ID from the browser
// performance.timeOrigin is the Unix timestamp (ms) when this worker context was created
const WORKER_MEMORY_ID = Math.floor(performance.timeOrigin);

// Mailbox constants
const STATUS_IDLE = 0;
const STATUS_PENDING = 1;
const STATUS_READY = 2;

// Mailbox field offsets (in i32 units)
const OFFSET_STATUS = 0;
const OFFSET_SYSCALL_NUM = 1;
const OFFSET_ARG0 = 2;
const OFFSET_ARG1 = 3;
const OFFSET_ARG2 = 4;
const OFFSET_RESULT = 5;
const OFFSET_DATA_LEN = 6;
const OFFSET_DATA = 7;  // Byte offset 28
const OFFSET_PID = 14;  // PID storage location

// Worker state (set after initialization)
let initialized = false;
let workerPid = 0;

self.onmessage = async (event) => {
    const data = event.data;
    
    // Check message type
    if (data.type === 'terminate') {
        // Supervisor is terminating this worker
        self.close();
        return;
    }
    
    if (data.type === 'ipc') {
        // IPC message delivery - with SharedArrayBuffer approach, we don't need this
        // The process polls for messages via syscalls
        // Just ignore these messages
        return;
    }
    
    // If already initialized, ignore unknown messages
    if (initialized) {
        console.log(`[worker:${WORKER_MEMORY_ID}] Ignoring message after init:`, data.type || 'unknown');
        return;
    }
    
    // Initial spawn message with WASM binary
    const { binary, pid } = data;
    
    if (!binary || !pid) {
        console.error(`[worker:${WORKER_MEMORY_ID}] Invalid init message - missing binary or pid`);
        return;
    }
    
    workerPid = pid;
    
    try {
        // Create shared WASM memory
        // First 4KB (1024 i32s) is reserved for syscall mailbox
        // Rest is available for process heap/stack
        const memory = new WebAssembly.Memory({
            initial: 256,   // 16MB initial
            maximum: 1024,  // 64MB max
            shared: true    // Enable SharedArrayBuffer backing
        });
        
        // Create views for mailbox access
        const mailboxView = new Int32Array(memory.buffer);
        const mailboxBytes = new Uint8Array(memory.buffer);
        
        // Store PID in mailbox for orbital_get_pid
        Atomics.store(mailboxView, OFFSET_PID, pid);
        
        /**
         * Make a syscall using SharedArrayBuffer + Atomics
         * 
         * This function:
         * 1. Writes syscall parameters to the shared mailbox
         * 2. Sets status to PENDING
         * 3. Blocks using Atomics.wait until the supervisor processes it
         * 4. Reads and returns the result
         */
        function orbital_syscall(syscall_num, arg0, arg1, arg2) {
            // Write syscall parameters
            Atomics.store(mailboxView, OFFSET_SYSCALL_NUM, syscall_num);
            Atomics.store(mailboxView, OFFSET_ARG0, arg0);
            Atomics.store(mailboxView, OFFSET_ARG1, arg1);
            Atomics.store(mailboxView, OFFSET_ARG2, arg2);
            
            // Set status to PENDING (signals the supervisor)
            Atomics.store(mailboxView, OFFSET_STATUS, STATUS_PENDING);
            
            // Wait for supervisor to process the syscall
            // Atomics.wait blocks until status changes from PENDING
            while (true) {
                const waitResult = Atomics.wait(mailboxView, OFFSET_STATUS, STATUS_PENDING, 1000);
                // waitResult: "ok" (notified), "not-equal" (value changed), "timed-out"
                
                const status = Atomics.load(mailboxView, OFFSET_STATUS);
                if (status !== STATUS_PENDING) {
                    break;
                }
                // Spurious wakeup or timeout, wait again
            }
            
            // Read the result
            const result = Atomics.load(mailboxView, OFFSET_RESULT);
            
            // Reset status to IDLE
            Atomics.store(mailboxView, OFFSET_STATUS, STATUS_IDLE);
            
            return result;
        }
        
        /**
         * Send bytes to the syscall data buffer
         * Must be called before orbital_syscall when the syscall needs data
         */
        function orbital_send_bytes(ptr, len) {
            const maxLen = 4068;
            const actualLen = Math.min(len, maxLen);
            
            if (actualLen > 0) {
                // Copy data from WASM memory to mailbox data buffer
                const srcBytes = new Uint8Array(memory.buffer, ptr, actualLen);
                mailboxBytes.set(srcBytes, 28); // Data starts at byte offset 28
            }
            
            // Store data length
            Atomics.store(mailboxView, OFFSET_DATA_LEN, actualLen);
            
            return actualLen;
        }
        
        /**
         * Receive bytes from the syscall result buffer
         * Called after orbital_syscall to retrieve result data
         */
        function orbital_recv_bytes(ptr, maxLen) {
            // Read data length from mailbox
            const dataLen = Atomics.load(mailboxView, OFFSET_DATA_LEN);
            const actualLen = Math.min(dataLen, maxLen);
            
            if (actualLen > 0) {
                // Copy data from mailbox to WASM memory
                const dstBytes = new Uint8Array(memory.buffer, ptr, actualLen);
                dstBytes.set(mailboxBytes.slice(28, 28 + actualLen));
            }
            
            return actualLen;
        }
        
        /**
         * Yield the current process's time slice
         * Does a short wait to allow other processes to run
         */
        function orbital_yield() {
            // Use Atomics.wait with 0 timeout to yield briefly
            // This allows the event loop to process other tasks
            Atomics.wait(mailboxView, OFFSET_STATUS, 0, 0);
        }
        
        /**
         * Get the process's assigned PID
         */
        function orbital_get_pid() {
            return Atomics.load(mailboxView, OFFSET_PID);
        }
        
        // Compile and instantiate WASM with syscall imports
        const module = await WebAssembly.compile(binary);
        const instance = await WebAssembly.instantiate(module, {
            env: {
                memory: memory,
                orbital_syscall: orbital_syscall,
                orbital_send_bytes: orbital_send_bytes,
                orbital_recv_bytes: orbital_recv_bytes,
                orbital_yield: orbital_yield,
                orbital_get_pid: orbital_get_pid,
            }
        });
        
        // Send shared memory to supervisor for syscall mailbox access
        // Include worker:memory_id - the browser-assigned context timestamp
        self.postMessage({
            type: 'memory',
            pid: pid,
            buffer: memory.buffer,  // SharedArrayBuffer
            workerId: WORKER_MEMORY_ID  // Browser-generated worker context ID
        });
        
        // Mark as initialized before running
        initialized = true;
        
        // Initialize runtime - mailbox is at offset 0
        // (This may be a no-op if the WASM uses our imported functions instead)
        if (instance.exports.__orbital_rt_init) {
            instance.exports.__orbital_rt_init(0);
        }
        
        // Run the process - blocks forever using atomics-based syscalls
        if (instance.exports._start) {
            instance.exports._start();
        }
        
    } catch (e) {
        console.error(`[worker:${WORKER_MEMORY_ID}] Error:`, e);
        self.postMessage({
            type: 'error',
            pid: workerPid,
            workerId: WORKER_MEMORY_ID,
            error: e.message
        });
    }
};
