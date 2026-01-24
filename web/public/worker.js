/**
 * Zero OS Process Worker - Minimal Bootstrap
 * 
 * This script is a thin shim that:
 * 1. Instantiates the WASM binary with syscall imports
 * 2. Uses the WASM module's memory for the syscall mailbox
 * 3. Reports memory to supervisor for syscall polling
 * 4. Calls _start() - WASM runs forever using atomics-based syscalls
 * 
 * Mailbox Layout (at offset 0 in WASM linear memory):
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

// Note: VFS operations now go through syscalls to the supervisor (main thread),
// which handles IndexedDB access. Workers no longer need direct VFS bindings.

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
        // Compile the WASM module to inspect its imports/exports
        const module = await WebAssembly.compile(binary);
        
        // Check what the module needs
        const imports = WebAssembly.Module.imports(module);
        const exports = WebAssembly.Module.exports(module);
        const importsMemory = imports.some(imp => imp.module === 'env' && imp.name === 'memory' && imp.kind === 'memory');
        const exportsMemory = exports.some(exp => exp.name === 'memory' && exp.kind === 'memory');
        
        console.log(`[worker:${WORKER_MEMORY_ID}] Module imports memory: ${importsMemory}, exports memory: ${exportsMemory}`);
        
        // The memory we'll use for the mailbox - determined after instantiation
        let wasmMemory = null;
        let mailboxView = null;
        let mailboxBytes = null;
        
        // Helper to refresh views if memory buffer changes (e.g., after memory.grow())
        function refreshViews() {
            if (wasmMemory && mailboxView.buffer !== wasmMemory.buffer) {
                mailboxView = new Int32Array(wasmMemory.buffer);
                mailboxBytes = new Uint8Array(wasmMemory.buffer);
            }
        }
        
        /**
         * Make a syscall using SharedArrayBuffer + Atomics
         */
        function zos_syscall(syscall_num, arg0, arg1, arg2) {
            refreshViews();
            
            // Write syscall parameters
            Atomics.store(mailboxView, OFFSET_SYSCALL_NUM, syscall_num);
            Atomics.store(mailboxView, OFFSET_ARG0, arg0);
            Atomics.store(mailboxView, OFFSET_ARG1, arg1);
            Atomics.store(mailboxView, OFFSET_ARG2, arg2);
            
            // Set status to PENDING (signals the supervisor)
            Atomics.store(mailboxView, OFFSET_STATUS, STATUS_PENDING);
            
            // Wait for supervisor to process the syscall
            while (true) {
                const waitResult = Atomics.wait(mailboxView, OFFSET_STATUS, STATUS_PENDING, 1000);
                const status = Atomics.load(mailboxView, OFFSET_STATUS);
                if (status !== STATUS_PENDING) {
                    break;
                }
            }
            
            // Read the result
            const result = Atomics.load(mailboxView, OFFSET_RESULT);
            
            // Reset status to IDLE
            Atomics.store(mailboxView, OFFSET_STATUS, STATUS_IDLE);
            
            return result;
        }
        
        /**
         * Send bytes to the syscall data buffer
         * Must be called before zos_syscall when the syscall needs data
         */
        function zos_send_bytes(ptr, len) {
            refreshViews();
            
            const maxLen = 4068;
            const actualLen = Math.min(len, maxLen);
            
            if (actualLen > 0) {
                // Copy data from WASM linear memory (at ptr) to mailbox data buffer (at offset 28)
                // Both are in the same wasmMemory.buffer
                const srcBytes = new Uint8Array(wasmMemory.buffer, ptr, actualLen);
                mailboxBytes.set(srcBytes, 28);
            }
            
            // Store data length
            Atomics.store(mailboxView, OFFSET_DATA_LEN, actualLen);
            
            return actualLen;
        }
        
        /**
         * Receive bytes from the syscall result buffer
         */
        function zos_recv_bytes(ptr, maxLen) {
            refreshViews();
            
            const dataLen = Atomics.load(mailboxView, OFFSET_DATA_LEN);
            const actualLen = Math.min(dataLen, maxLen);
            
            if (actualLen > 0) {
                const dstBytes = new Uint8Array(wasmMemory.buffer, ptr, actualLen);
                dstBytes.set(mailboxBytes.slice(28, 28 + actualLen));
            }
            
            return actualLen;
        }
        
        /**
         * Yield the current process's time slice
         */
        function zos_yield() {
            refreshViews();
            Atomics.wait(mailboxView, OFFSET_STATUS, 0, 0);
        }
        
        /**
         * Get the process's assigned PID
         */
        function zos_get_pid() {
            refreshViews();
            return Atomics.load(mailboxView, OFFSET_PID);
        }
        
        // Build import object
        // If module imports memory, we must provide shared memory for atomics to work
        let sharedMemory = null;
        const importObject = {
            env: {
                zos_syscall: zos_syscall,
                zos_send_bytes: zos_send_bytes,
                zos_recv_bytes: zos_recv_bytes,
                zos_yield: zos_yield,
                zos_get_pid: zos_get_pid,
                // Note: VFS operations now go through storage syscalls (0x70-0x74)
                // which are handled by the supervisor in the main thread.
            }
        };
        
        if (importsMemory) {
            // Module imports memory - provide shared memory
            // Memory sizes must match what WASM module declares (set via linker args)
            // Current config: 4MB initial (64 pages), 16MB max (256 pages)
            sharedMemory = new WebAssembly.Memory({
                initial: 64,    // 4MB initial (64 * 64KB)
                maximum: 256,   // 16MB max (256 * 64KB)
                shared: true
            });
            importObject.env.memory = sharedMemory;
        }
        
        // Instantiate the module
        const instance = await WebAssembly.instantiate(module, importObject);
        
        // Determine which memory to use
        if (importsMemory && sharedMemory) {
            // Module imported our shared memory - use it
            wasmMemory = sharedMemory;
            console.log(`[worker:${WORKER_MEMORY_ID}] Using imported shared memory`);
        } else if (exportsMemory && instance.exports.memory) {
            // Module has its own memory - use the exported memory
            wasmMemory = instance.exports.memory;
            console.log(`[worker:${WORKER_MEMORY_ID}] Using module's exported memory`);
            
            // Check if it's a SharedArrayBuffer (required for atomics)
            if (!(wasmMemory.buffer instanceof SharedArrayBuffer)) {
                console.warn(`[worker:${WORKER_MEMORY_ID}] Module memory is not shared - atomics may not work correctly`);
            }
        } else {
            throw new Error('WASM module has no accessible memory');
        }
        
        // Create typed array views for the mailbox
        mailboxView = new Int32Array(wasmMemory.buffer);
        mailboxBytes = new Uint8Array(wasmMemory.buffer);
        
        // Store PID in mailbox
        Atomics.store(mailboxView, OFFSET_PID, pid);
        
        // Send the memory buffer to supervisor for syscall mailbox access
        // The supervisor will use this same buffer to read/write syscall data
        self.postMessage({
            type: 'memory',
            pid: pid,
            buffer: wasmMemory.buffer,
            workerId: WORKER_MEMORY_ID
        });
        
        // Mark as initialized before running
        initialized = true;
        
        // Initialize runtime if the module exports it
        if (instance.exports.__zero_rt_init) {
            instance.exports.__zero_rt_init(0);
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
