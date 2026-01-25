/**
 * Zero OS Process Worker - Minimal Bootstrap
 * VERSION: 2025-01-25-crypto-prototypesetcall-fix
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
 * | 28     | 16356| data buffer (16KB total buffer)    |
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
const OFFSET_DATA = 7; // Byte offset 28
const OFFSET_PID = 14; // PID storage location

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
    console.log(
      `[worker:${WORKER_MEMORY_ID}] Ignoring message after init:`,
      data.type || 'unknown'
    );
    return;
  }

  // Initial spawn message with WASM binary
  const { binary, pid } = data;

  if (!binary || !pid) {
    console.error(`[worker:${WORKER_MEMORY_ID}] Invalid init message - missing binary or pid`);
    return;
  }

  workerPid = pid;

  // #region agent log - hypothesis A
  fetch('http://127.0.0.1:7245/ingest/2b6230bb-353d-4596-b36e-727501c94847',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({location:'worker.js:init',message:'worker_init',data:{pid:pid,binarySize:binary.byteLength,timestamp:Date.now()},timestamp:Date.now(),sessionId:'debug-session',hypothesisId:'A'})}).catch(()=>{});
  // #endregion

  try {
    // Compile the WASM module to inspect its imports/exports
    const module = await WebAssembly.compile(binary);

    // Check what the module needs
    const imports = WebAssembly.Module.imports(module);
    const exports = WebAssembly.Module.exports(module);
    const importsMemory = imports.some(
      (imp) => imp.module === 'env' && imp.name === 'memory' && imp.kind === 'memory'
    );
    const exportsMemory = exports.some((exp) => exp.name === 'memory' && exp.kind === 'memory');

    console.log(
      `[worker:${WORKER_MEMORY_ID}] Module imports memory: ${importsMemory}, exports memory: ${exportsMemory}`
    );

    // The memory we'll use for the mailbox - determined after instantiation
    let wasmMemory = null;
    let mailboxView = null;
    let mailboxBytes = null;

    // Helper to refresh views if memory buffer changes (e.g., after memory.grow())
    function refreshViews() {
      if (wasmMemory && mailboxView.buffer !== wasmMemory.buffer) {
        console.log(
          `[worker:${WORKER_MEMORY_ID}:${workerPid}] MEMORY GREW! Old buffer detached, creating new views. New size: ${wasmMemory.buffer.byteLength} bytes`
        );
        
        // CRITICAL: Save mailbox state from old buffer before it's detached
        // The supervisor may have written data that we haven't read yet
        const oldMailboxView = mailboxView;
        const oldMailboxBytes = mailboxBytes;
        const savedStatus = Atomics.load(oldMailboxView, OFFSET_STATUS);
        const savedDataLen = Atomics.load(oldMailboxView, OFFSET_DATA_LEN);
        const savedResult = Atomics.load(oldMailboxView, OFFSET_RESULT);
        
        // Copy data buffer if there's pending data
        let savedData = null;
        if (savedDataLen > 0) {
          savedData = new Uint8Array(savedDataLen);
          savedData.set(oldMailboxBytes.slice(28, 28 + savedDataLen));
          console.log(
            `[worker:${WORKER_MEMORY_ID}:${workerPid}] Preserved ${savedDataLen} bytes of mailbox data across memory growth`
          );
        }
        
        // Update local views to new buffer
        mailboxView = new Int32Array(wasmMemory.buffer);
        mailboxBytes = new Uint8Array(wasmMemory.buffer);
        
        // Restore mailbox state to new buffer
        Atomics.store(mailboxView, OFFSET_STATUS, savedStatus);
        Atomics.store(mailboxView, OFFSET_DATA_LEN, savedDataLen);
        Atomics.store(mailboxView, OFFSET_RESULT, savedResult);
        Atomics.store(mailboxView, OFFSET_PID, workerPid);
        
        // Restore data buffer
        if (savedData) {
          mailboxBytes.set(savedData, 28);
        }
        
        // CRITICAL: Re-send the new buffer to the supervisor!
        // The supervisor must update its reference or it will write to the old (detached) buffer
        self.postMessage({
          type: 'memory',
          pid: workerPid,
          buffer: wasmMemory.buffer,
          workerId: WORKER_MEMORY_ID,
        });
        
        console.log(
          `[worker:${WORKER_MEMORY_ID}:${workerPid}] Sent new SharedArrayBuffer to supervisor after memory growth`
        );
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

      const maxLen = 16356;
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

      // DEBUG: Log for Init when receiving 0 bytes but expecting data
      if (workerPid === 1 && dataLen === 0 && maxLen > 0) {
        // Check the buffer marker
        const markerView = new Uint32Array(wasmMemory.buffer);
        const marker = markerView[4096];
        console.error(
          `[worker:${WORKER_MEMORY_ID}:${workerPid}] RECV DEBUG: dataLen=0 but maxLen=${maxLen}. ` +
          `Buffer size: ${wasmMemory.buffer.byteLength}, ` +
          `Buffer is SharedArrayBuffer: ${wasmMemory.buffer instanceof SharedArrayBuffer}, ` +
          `Marker at offset 16384: 0x${marker.toString(16).toUpperCase().padStart(8, '0')}`
        );
      }

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
      // Wait up to 1ms before returning - prevents busy-loop when no messages
      Atomics.wait(mailboxView, OFFSET_STATUS, STATUS_IDLE, 1);
    }

    /**
     * Get the process's assigned PID
     */
    function zos_get_pid() {
      refreshViews();
      return Atomics.load(mailboxView, OFFSET_PID);
    }

    /**
     * Helper to decode string from WASM memory (handles SharedArrayBuffer)
     * TextDecoder.decode() doesn't support SharedArrayBuffer views, so we copy first
     */
    function decodeWasmString(ptr, len) {
      if (!wasmMemory) return '';
      const sharedView = new Uint8Array(wasmMemory.buffer, ptr, len);
      // Copy to non-shared buffer for TextDecoder compatibility
      const copied = new Uint8Array(len);
      copied.set(sharedView);
      return new TextDecoder().decode(copied);
    }

    /**
     * wasm-bindgen shims - required when dependencies like getrandom or uuid
     * are built with "js" features enabled. These add wasm-bindgen imports even though
     * we're not using wasm-pack. We provide minimal shims to satisfy the imports.
     */
    function __wbindgen_throw(ptr, len) {
      if (!wasmMemory) {
        throw new Error('wasm-bindgen error: memory not initialized');
      }
      const msg = decodeWasmString(ptr, len);
      throw new Error(msg);
    }

    function __wbindgen_rethrow(arg) {
      throw arg;
    }

    function __wbindgen_memory() {
      return wasmMemory;
    }

    function __wbindgen_is_undefined(idx) {
      const val = getObject(idx);
      return val === undefined;
    }

    function __wbindgen_is_null(idx) {
      const val = getObject(idx);
      return val === null;
    }

    function __wbindgen_object_drop_ref(idx) {
      dropObject(idx);
    }

    function __wbindgen_string_new(ptr, len) {
      const str = decodeWasmString(ptr, len);
      return addHeapObject(str);
    }

    function __wbindgen_number_get(val) {
      return typeof val === 'number' ? val : undefined;
    }

    function __wbindgen_boolean_get(val) {
      return typeof val === 'boolean' ? val : undefined;
    }

    function __wbindgen_jsval_eq(a, b) {
      return a === b;
    }

    function __wbindgen_describe(val) {
      // Type description function - no-op for our use case
      // This is used by wasm-bindgen for type introspection during initialization
    }

    function __wbindgen_object_clone_ref(idx) {
      // Clone an object reference - add the same object to heap again
      const obj = getObject(idx);
      return addHeapObject(obj);
    }

    function __wbg___wbindgen_throw_be289d5034ed271b(ptr, len) {
      // Mangled version of __wbindgen_throw with hash suffix
      return __wbindgen_throw(ptr, len);
    }

    function __wbindgen_string_get(val, retptr) {
      // Get a string value - store pointer and length at retptr
      if (!wasmMemory || typeof val !== 'string') {
        const view = new DataView(wasmMemory.buffer);
        view.setUint32(retptr, 0, true);
        view.setUint32(retptr + 4, 0, true);
        return;
      }
      const encoded = new TextEncoder().encode(val);
      const view = new DataView(wasmMemory.buffer);
      view.setUint32(retptr, encoded.byteOffset, true);
      view.setUint32(retptr + 4, encoded.byteLength, true);
    }

    function __wbindgen_error_new(ptr, len) {
      // Create a JS Error from WASM string
      const msg = decodeWasmString(ptr, len) || 'Unknown error';
      const err = new Error(msg);
      return addHeapObject(err);
    }

    function __wbindgen_cb_drop(idx) {
      // Drop a callback - no-op for our use case
      return true;
    }

    function __wbindgen_describe_cast() {
      // Type cast description - no-op for runtime, used by wasm-bindgen for type introspection
    }

    // JS Object heap for wasm-bindgen compatibility
    // wasm-bindgen stores JS objects in a heap and passes indices to WASM
    const heap = new Array(128).fill(undefined);
    // Pre-populate with common values at known indices
    heap.push(undefined, null, true, false);
    let heapNext = heap.length;
    
    function addHeapObject(obj) {
      if (heapNext === heap.length) heap.push(heap.length + 1);
      const idx = heapNext;
      heapNext = heap[idx];
      heap[idx] = obj;
      return idx;
    }
    
    function getObject(idx) {
      return heap[idx];
    }
    
    function dropObject(idx) {
      if (idx < 132) return; // Don't drop pre-populated values
      heap[idx] = heapNext;
      heapNext = idx;
    }
    
    function takeObject(idx) {
      const ret = getObject(idx);
      dropObject(idx);
      return ret;
    }

    // External reference table for managing JS object references
    const externrefTable = new WebAssembly.Table({
      initial: 128,
      maximum: 128,
      element: 'externref',
    });

    function __wbindgen_externref_table_grow(delta) {
      // Grow the externref table - just return current size
      return externrefTable.length;
    }

    function __wbindgen_externref_table_set_null(idx) {
      // Set table entry to null
      if (idx < externrefTable.length) {
        externrefTable.set(idx, null);
      }
    }

    // Crypto API shim for getrandom - provides access to browser's crypto.getRandomValues
    // NOTE: crypto.getRandomValues does NOT support SharedArrayBuffer!
    // When WASM memory is shared (for atomics), we must copy via a non-shared buffer.
    function __wbindgen_get_random_values(ptr, len) {
      console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] __wbindgen_get_random_values called: ptr=${ptr}, len=${len}`);
      if (!wasmMemory) {
        console.error(`[worker:${WORKER_MEMORY_ID}:${workerPid}] CRYPTO ERROR: Memory not initialized`);
        throw new Error('Memory not initialized for getRandomValues');
      }
      const buf = new Uint8Array(wasmMemory.buffer, ptr, len);
      
      // Check if backed by SharedArrayBuffer - getRandomValues doesn't support it
      if (wasmMemory.buffer instanceof SharedArrayBuffer) {
        // Create a non-shared copy, fill it, then copy back
        const copy = new Uint8Array(len);
        self.crypto.getRandomValues(copy);
        buf.set(copy);
        // Verify we got non-zero random bytes
        const allZeros = copy.every(b => b === 0);
        if (allZeros && len > 0) {
          console.error(`[worker:${WORKER_MEMORY_ID}:${workerPid}] CRYPTO WARNING: getRandomValues returned all zeros!`);
        }
      } else {
        self.crypto.getRandomValues(buf);
      }
      // Log first few bytes for debugging (only for small buffers to avoid spam)
      if (len <= 32) {
        const preview = Array.from(buf.slice(0, Math.min(8, len))).map(b => b.toString(16).padStart(2, '0')).join('');
        console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] Random bytes preview: ${preview}...`);
      }
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
      },
      // External reference table functions for wasm-bindgen
      __wbindgen_externref_xform__: new Proxy(
        {
          __wbindgen_externref_table_grow: __wbindgen_externref_table_grow,
          __wbindgen_externref_table_set_null: __wbindgen_externref_table_set_null,
        },
        {
          get(target, prop) {
            if (prop in target) {
              return target[prop];
            }
            // Return no-op for unknown externref functions
            console.log(`[worker:${WORKER_MEMORY_ID}] Shimming unknown externref import: ${prop}`);
            return function () {
              return undefined;
            };
          },
        }
      ),
      // wasm-bindgen placeholder module with Proxy to handle unknown imports
      __wbindgen_placeholder__: new Proxy(
        {
          __wbindgen_throw: __wbindgen_throw,
          __wbindgen_rethrow: __wbindgen_rethrow,
          __wbindgen_memory: __wbindgen_memory,
          __wbindgen_is_undefined: __wbindgen_is_undefined,
          __wbindgen_is_null: __wbindgen_is_null,
          __wbindgen_object_drop_ref: __wbindgen_object_drop_ref,
          __wbindgen_object_clone_ref: __wbindgen_object_clone_ref,
          __wbindgen_string_new: __wbindgen_string_new,
          __wbindgen_string_get: __wbindgen_string_get,
          __wbindgen_number_get: __wbindgen_number_get,
          __wbindgen_boolean_get: __wbindgen_boolean_get,
          __wbindgen_jsval_eq: __wbindgen_jsval_eq,
          __wbindgen_describe: __wbindgen_describe,
          __wbindgen_error_new: __wbindgen_error_new,
          __wbindgen_cb_drop: __wbindgen_cb_drop,
          __wbindgen_describe_cast: __wbindgen_describe_cast,
          __wbg___wbindgen_throw_be289d5034ed271b: __wbg___wbindgen_throw_be289d5034ed271b,
          // Crypto API for getrandom
          __wbindgen_get_random_values: __wbindgen_get_random_values,
        },
        {
          get(target, prop) {
            // Return existing function if available
            if (prop in target) {
              return target[prop];
            }
            
            // Handle Web Crypto API imports for getrandom "js" feature
            // getrandom uses wasm-bindgen which generates __wbg_* imports
            if (prop.includes('crypto') && !prop.includes('msCrypto')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: crypto getter shim matched for import: ${prop}`);
              return function(selfIdx) {
                const selfObj = getObject(selfIdx);
                const crypto = selfObj?.crypto || self.crypto;
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY crypto getter: selfIdx=${selfIdx}, selfObj type=${typeof selfObj}, found crypto=${!!crypto}`);
                if (!crypto) {
                  console.error(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY crypto getter: FAILED - no crypto object found!`);
                }
                return addHeapObject(crypto);
              };
            }
            if (prop.includes('getRandomValues')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: getRandomValues shim matched for import: ${prop}`);
              return function(cryptoIdx, arrayIdx) {
                const cryptoObj = getObject(cryptoIdx) || self.crypto;
                const array = getObject(arrayIdx);
                
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY getRandomValues: cryptoIdx=${cryptoIdx}, arrayIdx=${arrayIdx}, arrayLen=${array?.length}`);
                
                if (array instanceof Uint8Array) {
                  // Check if backed by SharedArrayBuffer - getRandomValues doesn't support it
                  if (array.buffer instanceof SharedArrayBuffer) {
                    // Create a non-shared copy, fill it, then copy back
                    const copy = new Uint8Array(array.length);
                    cryptoObj.getRandomValues(copy);
                    array.set(copy);
                    // Log preview
                    const preview = Array.from(copy.slice(0, 8)).map(b => b.toString(16).padStart(2, '0')).join('');
                    console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY random bytes (SharedAB): ${preview}...`);
                  } else {
                    cryptoObj.getRandomValues(array);
                    // Log preview
                    const preview = Array.from(array.slice(0, 8)).map(b => b.toString(16).padStart(2, '0')).join('');
                    console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY random bytes: ${preview}...`);
                  }
                } else {
                  console.error(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY getRandomValues: expected Uint8Array, got ${typeof array}, value:`, array);
                  throw new Error(`getRandomValues: expected Uint8Array, got ${typeof array}`);
                }
              };
            }
            if (prop.includes('randomFillSync')) {
              // Node.js fallback - use getRandomValues in browser
              // NOTE: crypto.getRandomValues does NOT support SharedArrayBuffer!
              return function(cryptoObj, ptr, len) {
                if (!wasmMemory) {
                  throw new Error('Memory not initialized for randomFillSync');
                }
                const buf = new Uint8Array(wasmMemory.buffer, ptr, len);
                
                // Check if backed by SharedArrayBuffer - getRandomValues doesn't support it
                if (wasmMemory.buffer instanceof SharedArrayBuffer) {
                  const copy = new Uint8Array(len);
                  self.crypto.getRandomValues(copy);
                  buf.set(copy);
                } else {
                  self.crypto.getRandomValues(buf);
                }
              };
            }
            if (prop.includes('static_accessor_SELF') || prop.includes('_self_')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: SELF accessor shim matched for import: ${prop}`);
              return function() {
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY SELF(): returning self (Web Worker global)`);
                return addHeapObject(self);
              };
            }
            if (prop.includes('static_accessor_WINDOW') || prop.includes('_window_')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: WINDOW accessor shim matched for import: ${prop}`);
              return function() {
                const obj = typeof window !== 'undefined' ? window : undefined;
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY WINDOW(): window exists=${typeof window !== 'undefined'}, returning ${obj === undefined ? '0 (undefined)' : 'window object'}`);
                if (obj === undefined) return 0;
                return addHeapObject(obj);
              };
            }
            if (prop.includes('static_accessor_GLOBAL_THIS') || prop.includes('_globalThis_')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: GLOBAL_THIS accessor shim matched for import: ${prop}`);
              return function() {
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY GLOBAL_THIS(): returning globalThis (has crypto=${!!globalThis.crypto})`);
                return addHeapObject(globalThis);
              };
            }
            if (prop.includes('static_accessor_GLOBAL') || prop.includes('_global_')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: GLOBAL accessor shim matched for import: ${prop}`);
              return function() {
                // In Web Workers, there's no 'global' (that's Node.js), so fall back to 'self'
                // getrandom's js.rs calls js_sys::global() which checks globalThis/self/window/global
                const obj = typeof global !== 'undefined' ? global : self;
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY GLOBAL(): global exists=${typeof global !== 'undefined'}, returning ${typeof global !== 'undefined' ? 'global' : 'self (fallback)'} (has crypto=${!!obj.crypto})`);
                return addHeapObject(obj);
              };
            }
            if (prop.includes('new_no_args') || prop.includes('newnoargs')) {
              return function(ptr, len) {
                const str = decodeWasmString(ptr, len);
                const fn = new Function(str);
                return addHeapObject(fn);
              };
            }
            if (prop.includes('is_object')) {
              return function(idx) {
                const val = getObject(idx);
                return typeof val === 'object' && val !== null;
              };
            }
            if (prop.includes('is_undefined')) {
              return function(idx) {
                const val = getObject(idx);
                return val === undefined;
              };
            }
            if (prop.includes('is_string')) {
              return function(idx) {
                const val = getObject(idx);
                return typeof val === 'string';
              };
            }
            if (prop.includes('is_function')) {
              return function(idx) {
                const val = getObject(idx);
                return typeof val === 'function';
              };
            }
            // Uint8Array operations for getrandom
            if (prop.includes('new_with_length') || prop.includes('newwithlength')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: new_with_length shim matched for import: ${prop}`);
              return function(len) {
                // Create a standalone Uint8Array (NOT backed by WASM SharedArrayBuffer)
                // This allows crypto.getRandomValues to work
                const array = new Uint8Array(len);
                const idx = addHeapObject(array);
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY Uint8Array(${len}): created, heap idx=${idx}`);
                return idx;
              };
            }
            if (prop.includes('subarray')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: subarray shim matched for import: ${prop}`);
              return function(arrayIdx, start, end) {
                const array = getObject(arrayIdx);
                const subarray = array.subarray(start, end);
                const idx = addHeapObject(subarray);
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY subarray: arrayIdx=${arrayIdx}, [${start}:${end}], len=${subarray.length}, heap idx=${idx}`);
                return idx;
              };
            }
            if (prop.includes('_length_')) {
              return function(arrayIdx) {
                const array = getObject(arrayIdx);
                return array?.length || 0;
              };
            }
            // Uint8Array.prototype.set.call() - used by wasm-bindgen for raw_copy_to_ptr
            // CRITICAL: This is how getrandom copies random bytes back to WASM memory!
            // Signature: (wasmPtr, length, srcArrayIdx) -> copies srcArray to WASM memory at ptr
            if (prop.includes('prototypesetcall')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: prototypesetcall shim matched for import: ${prop}`);
              return function(wasmPtr, length, srcArrayIdx) {
                const srcArray = getObject(srcArrayIdx);
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY prototypesetcall: wasmPtr=${wasmPtr}, length=${length}, srcArrayIdx=${srcArrayIdx}, srcArray type=${srcArray?.constructor?.name}`);
                
                if (!wasmMemory || !wasmMemory.buffer) {
                  console.error(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY prototypesetcall: FAILED - wasmMemory not available!`);
                  return;
                }
                
                if (!(srcArray instanceof Uint8Array)) {
                  console.error(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY prototypesetcall: FAILED - srcArray is not Uint8Array!`);
                  return;
                }
                
                // Create a view into WASM memory at the destination pointer
                const dest = new Uint8Array(wasmMemory.buffer, wasmPtr, length);
                // Copy the source array (which has the random bytes) to WASM memory
                dest.set(srcArray.subarray(0, length));
                
                // Log preview of copied data
                const preview = Array.from(srcArray.slice(0, Math.min(8, length))).map(b => b.toString(16).padStart(2, '0')).join('');
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY prototypesetcall: SUCCESS - copied ${length} bytes to WASM ptr ${wasmPtr}, preview: ${preview}...`);
              };
            }
            
            // Uint8Array.prototype.set - copy data from one array to another
            // CRITICAL: getrandom's raw_copy_to_ptr uses this with (arrayIdx, wasmPtr) signature
            // where second arg is a RAW WASM MEMORY POINTER, not a heap object index!
            if (prop.includes('set_') || prop.includes('_set_')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: set shim matched for import: ${prop}`);
              return function(arg0, arg1, arg2) {
                const src = getObject(arg0);
                
                // Detect if arg1 is a raw WASM pointer (for raw_copy_to_ptr) or a heap index
                // If arg1 is large (> heap size threshold) or arg2 is undefined, treat as WASM pointer
                // getrandom's raw_copy_to_ptr passes (arrayIdx, wasmPtr) - 2 args
                // Standard set passes (targetIdx, srcIdx, offset) - 3 args or (targetIdx, srcIdx) - 2 args
                
                if (src instanceof Uint8Array && typeof arg1 === 'number' && arg2 === undefined) {
                  // Check if arg1 looks like a WASM memory pointer (typically > 1000)
                  // Heap indices are typically small (< 200)
                  if (arg1 > 200 || !getObject(arg1)) {
                    // This is raw_copy_to_ptr: copy JS array to WASM memory at pointer arg1
                    console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY set (raw_copy_to_ptr): copying ${src.length} bytes from JS array to WASM ptr ${arg1}`);
                    
                    if (!wasmMemory || !wasmMemory.buffer) {
                      console.error(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY set: FAILED - wasmMemory not available!`);
                      return;
                    }
                    
                    // Create a view into WASM memory and copy the data
                    const dest = new Uint8Array(wasmMemory.buffer, arg1, src.length);
                    dest.set(src);
                    
                    // Log preview of copied data
                    const preview = Array.from(src.slice(0, Math.min(8, src.length))).map(b => b.toString(16).padStart(2, '0')).join('');
                    console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY set: copied to WASM memory, preview: ${preview}...`);
                    return;
                  }
                }
                
                // Standard case: both args are heap objects
                const target = getObject(arg0);
                const srcObj = getObject(arg1);
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY set (standard): targetIdx=${arg0}, srcIdx=${arg1}, offset=${arg2}, srcLen=${srcObj?.length}`);
                if (arg2 !== undefined) {
                  target.set(srcObj, arg2);
                } else {
                  target.set(srcObj);
                }
                // Log preview of data being set
                if (srcObj && srcObj.length > 0) {
                  const preview = Array.from(srcObj.slice(0, Math.min(8, srcObj.length))).map(b => b.toString(16).padStart(2, '0')).join('');
                  console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY set data preview: ${preview}...`);
                }
              };
            }
            // Copy typed array to WASM memory (raw_copy_to_ptr)
            // getrandom uses: #[wasm_bindgen(method, js_name = set)] fn raw_copy_to_ptr
            // This may generate imports with various patterns
            if (prop.includes('copy_to') || prop.includes('copyto') || prop.includes('raw_copy')) {
              console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY: copy_to shim matched for import: ${prop}`);
              return function(arrayIdx, ptr) {
                const array = getObject(arrayIdx);
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY copy_to: arrayIdx=${arrayIdx}, ptr=${ptr}, len=${array?.length}, wasmMemory.buffer exists=${!!wasmMemory?.buffer}`);
                if (!wasmMemory || !wasmMemory.buffer) {
                  console.error(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY copy_to: FAILED - wasmMemory not available!`);
                  return;
                }
                const dest = new Uint8Array(wasmMemory.buffer, ptr, array.length);
                dest.set(array);
                // Log preview of data being copied
                const preview = Array.from(array.slice(0, Math.min(8, array.length))).map(b => b.toString(16).padStart(2, '0')).join('');
                console.log(`[worker:${WORKER_MEMORY_ID}:${workerPid}] PROXY copy_to WASM memory: ${preview}... (${array.length} bytes to ptr ${ptr})`);
              };
            }
            // Node.js detection - return undefined (we're in browser)
            if (prop.includes('_process_') || prop.includes('_versions_') || prop.includes('_node_') || prop.includes('_require_')) {
              return function() {
                return undefined;
              };
            }
            // msCrypto - IE fallback, return undefined in modern browsers
            if (prop.includes('msCrypto')) {
              return function() {
                return undefined;
              };
            }
            if (prop.includes('_call_')) {
              return function(fnIdx, thisArgIdx, ...argIdxs) {
                const fn = getObject(fnIdx);
                const thisArg = getObject(thisArgIdx);
                const args = argIdxs.map(idx => getObject(idx));
                if (typeof fn === 'function') {
                  const result = fn.call(thisArg, ...args);
                  if (result !== undefined && result !== null && typeof result === 'object') {
                    return addHeapObject(result);
                  }
                  return result;
                }
                return 0; // undefined
              };
            }
            if (prop === '__wbindgen_is_object') {
              return function(val) {
                return typeof val === 'object' && val !== null;
              };
            }
            if (prop === '__wbindgen_is_function') {
              return function(val) {
                return typeof val === 'function';
              };
            }
            
            // Return no-op function for other unknown wasm-bindgen imports
            // ALWAYS log unknown imports to catch crypto-related failures
            console.warn(`[worker:${WORKER_MEMORY_ID}:${workerPid}] UNKNOWN wasm-bindgen import: ${prop}`);
            return function (...args) {
              // Log with full details to help debug missing shims
              const argTypes = args.map((a, i) => {
                if (typeof a === 'number') {
                  // Check if it might be a heap index
                  const heapObj = getObject(a);
                  if (heapObj !== undefined) {
                    return `arg${i}=${a} (heap: ${heapObj?.constructor?.name || typeof heapObj})`;
                  }
                  return `arg${i}=${a} (number, possibly ptr)`;
                }
                return `arg${i}=${typeof a}`;
              });
              console.warn(`[worker:${WORKER_MEMORY_ID}:${workerPid}] Called unknown import '${prop}' with args: [${argTypes.join(', ')}]`);
              return undefined;
            };
          },
        }
      ),
    };

    if (importsMemory) {
      // Module imports memory - provide shared memory
      // Memory sizes must match what WASM module declares (set via linker args)
      // Current config: 2MB initial (32 pages), 4MB max (64 pages)
      sharedMemory = new WebAssembly.Memory({
        initial: 32, // 2MB initial (32 * 64KB)
        maximum: 64, // 4MB max (64 * 64KB)
        shared: true,
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
        console.warn(
          `[worker:${WORKER_MEMORY_ID}] Module memory is not shared - atomics may not work correctly`
        );
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
    if (pid === 1) {
      console.log(`[worker:${WORKER_MEMORY_ID}] Init (PID 1) sending SharedArrayBuffer to supervisor`);
      console.log(`[worker:${WORKER_MEMORY_ID}] Buffer is SharedArrayBuffer: ${wasmMemory.buffer instanceof SharedArrayBuffer}`);
      console.log(`[worker:${WORKER_MEMORY_ID}] Buffer size: ${wasmMemory.buffer.byteLength} bytes`);
      
      // Write a magic marker at a known offset to verify buffer identity
      const markerView = new Uint32Array(wasmMemory.buffer);
      markerView[4096] = 0xDEADBEEF; // Write marker at offset 16KB (well past mailbox)
      console.log(`[worker:${WORKER_MEMORY_ID}:${pid}] Wrote buffer marker 0xDEADBEEF at offset 16384`);
    }
    
    self.postMessage({
      type: 'memory',
      pid: pid,
      buffer: wasmMemory.buffer,
      workerId: WORKER_MEMORY_ID,
    });

    // Mark as initialized before running
    initialized = true;

    // Initialize runtime if the module exports it
    if (instance.exports.__zero_rt_init) {
      console.log(`[worker:${WORKER_MEMORY_ID}:${pid}] Calling __zero_rt_init`);
      instance.exports.__zero_rt_init(0);
    }

    // Run the process - blocks forever using atomics-based syscalls
    if (instance.exports._start) {
      console.log(`[worker:${WORKER_MEMORY_ID}:${pid}] Calling _start() - process will block on atomics`);
      instance.exports._start();
      console.log(`[worker:${WORKER_MEMORY_ID}:${pid}] _start() returned (should never happen)`);
    } else {
      console.error(`[worker:${WORKER_MEMORY_ID}:${pid}] No _start export found!`);
    }
  } catch (e) {
    console.error(`[worker:${WORKER_MEMORY_ID}] Error:`, e);
    self.postMessage({
      type: 'error',
      pid: workerPid,
      workerId: WORKER_MEMORY_ID,
      error: e.message,
    });
  }
};
