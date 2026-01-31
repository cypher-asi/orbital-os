"use strict";
(() => {
  // src/workers/types.ts
  var STATUS_IDLE = 0;
  var STATUS_PENDING = 1;
  var MAILBOX_OFFSETS = {
    STATUS: 0,
    SYSCALL_NUM: 1,
    ARG0: 2,
    ARG1: 3,
    ARG2: 4,
    RESULT: 5,
    DATA_LEN: 6,
    DATA: 7,
    // Byte offset 28
    PID: 14
    // PID storage location
  };
  var MAILBOX_DATA_BYTE_OFFSET = 28;
  var MAILBOX_MAX_DATA_LEN = 16356;
  function createWorkerState(workerId) {
    return {
      initialized: false,
      pid: 0,
      workerId,
      wasmMemory: null,
      mailboxView: null,
      mailboxBytes: null
    };
  }

  // src/workers/mailbox.ts
  function refreshViews(state2, postMemoryUpdate2) {
    if (state2.wasmMemory && state2.mailboxView && state2.mailboxView.buffer !== state2.wasmMemory.buffer) {
      console.log(
        `[worker:${state2.workerId}:${state2.pid}] MEMORY GREW! Old buffer detached, creating new views. New size: ${state2.wasmMemory.buffer.byteLength} bytes`
      );
      const oldMailboxView = state2.mailboxView;
      const oldMailboxBytes = state2.mailboxBytes;
      const savedStatus = Atomics.load(oldMailboxView, MAILBOX_OFFSETS.STATUS);
      const savedDataLen = Atomics.load(oldMailboxView, MAILBOX_OFFSETS.DATA_LEN);
      const savedResult = Atomics.load(oldMailboxView, MAILBOX_OFFSETS.RESULT);
      let savedData = null;
      if (savedDataLen > 0) {
        savedData = new Uint8Array(savedDataLen);
        savedData.set(
          oldMailboxBytes.slice(
            MAILBOX_DATA_BYTE_OFFSET,
            MAILBOX_DATA_BYTE_OFFSET + savedDataLen
          )
        );
        console.log(
          `[worker:${state2.workerId}:${state2.pid}] Preserved ${savedDataLen} bytes of mailbox data across memory growth`
        );
      }
      state2.mailboxView = new Int32Array(state2.wasmMemory.buffer);
      state2.mailboxBytes = new Uint8Array(state2.wasmMemory.buffer);
      Atomics.store(state2.mailboxView, MAILBOX_OFFSETS.STATUS, savedStatus);
      Atomics.store(state2.mailboxView, MAILBOX_OFFSETS.DATA_LEN, savedDataLen);
      Atomics.store(state2.mailboxView, MAILBOX_OFFSETS.RESULT, savedResult);
      Atomics.store(state2.mailboxView, MAILBOX_OFFSETS.PID, state2.pid);
      if (savedData) {
        state2.mailboxBytes.set(savedData, MAILBOX_DATA_BYTE_OFFSET);
      }
      postMemoryUpdate2();
      console.log(
        `[worker:${state2.workerId}:${state2.pid}] Sent new SharedArrayBuffer to supervisor after memory growth`
      );
    }
  }
  function zos_syscall(state2, postMemoryUpdate2, syscall_num, arg0, arg1, arg2) {
    refreshViews(state2, postMemoryUpdate2);
    const view = state2.mailboxView;
    Atomics.store(view, MAILBOX_OFFSETS.SYSCALL_NUM, syscall_num);
    Atomics.store(view, MAILBOX_OFFSETS.ARG0, arg0);
    Atomics.store(view, MAILBOX_OFFSETS.ARG1, arg1);
    Atomics.store(view, MAILBOX_OFFSETS.ARG2, arg2);
    Atomics.store(view, MAILBOX_OFFSETS.STATUS, STATUS_PENDING);
    while (true) {
      Atomics.wait(view, MAILBOX_OFFSETS.STATUS, STATUS_PENDING, 1e3);
      const status = Atomics.load(view, MAILBOX_OFFSETS.STATUS);
      if (status !== STATUS_PENDING) {
        break;
      }
    }
    const result = Atomics.load(view, MAILBOX_OFFSETS.RESULT);
    Atomics.store(view, MAILBOX_OFFSETS.STATUS, STATUS_IDLE);
    return BigInt(result);
  }
  function zos_send_bytes(state2, postMemoryUpdate2, ptr, len) {
    refreshViews(state2, postMemoryUpdate2);
    const actualLen = Math.min(len, MAILBOX_MAX_DATA_LEN);
    if (actualLen > 0 && state2.wasmMemory) {
      const srcBytes = new Uint8Array(state2.wasmMemory.buffer, ptr, actualLen);
      state2.mailboxBytes.set(srcBytes, MAILBOX_DATA_BYTE_OFFSET);
    }
    Atomics.store(state2.mailboxView, MAILBOX_OFFSETS.DATA_LEN, actualLen);
    return actualLen;
  }
  function zos_recv_bytes(state2, postMemoryUpdate2, ptr, maxLen) {
    refreshViews(state2, postMemoryUpdate2);
    const dataLen = Atomics.load(state2.mailboxView, MAILBOX_OFFSETS.DATA_LEN);
    const actualLen = Math.min(dataLen, maxLen);
    if (state2.pid === 1 && dataLen === 0 && maxLen > 0 && state2.wasmMemory) {
      const markerView = new Uint32Array(state2.wasmMemory.buffer);
      const marker = markerView[4096];
      console.error(
        `[worker:${state2.workerId}:${state2.pid}] RECV DEBUG: dataLen=0 but maxLen=${maxLen}. Buffer size: ${state2.wasmMemory.buffer.byteLength}, Buffer is SharedArrayBuffer: ${state2.wasmMemory.buffer instanceof SharedArrayBuffer}, Marker at offset 16384: 0x${marker.toString(16).toUpperCase().padStart(8, "0")}`
      );
    }
    if (actualLen > 0 && state2.wasmMemory) {
      const dstBytes = new Uint8Array(state2.wasmMemory.buffer, ptr, actualLen);
      dstBytes.set(
        state2.mailboxBytes.slice(
          MAILBOX_DATA_BYTE_OFFSET,
          MAILBOX_DATA_BYTE_OFFSET + actualLen
        )
      );
    }
    return actualLen;
  }
  function zos_yield(state2, postMemoryUpdate2) {
    refreshViews(state2, postMemoryUpdate2);
    Atomics.wait(state2.mailboxView, MAILBOX_OFFSETS.STATUS, STATUS_IDLE, 1);
  }
  function zos_get_pid(state2, postMemoryUpdate2) {
    refreshViews(state2, postMemoryUpdate2);
    return Atomics.load(state2.mailboxView, MAILBOX_OFFSETS.PID);
  }

  // src/workers/heap.ts
  var WasmBindgenHeap = class _WasmBindgenHeap {
    heap;
    heapNext;
    // Reserved indices for pre-populated values
    static RESERVED_SIZE = 128;
    static UNDEFINED_IDX = 128;
    static NULL_IDX = 129;
    static TRUE_IDX = 130;
    static FALSE_IDX = 131;
    static FIRST_FREE_IDX = 132;
    constructor() {
      this.heap = new Array(_WasmBindgenHeap.RESERVED_SIZE).fill(void 0);
      this.heap.push(void 0, null, true, false);
      this.heapNext = this.heap.length;
    }
    /**
     * Add an object to the heap and return its index
     */
    addObject(obj) {
      if (this.heapNext === this.heap.length) {
        this.heap.push(this.heap.length + 1);
      }
      const idx = this.heapNext;
      this.heapNext = this.heap[idx];
      this.heap[idx] = obj;
      return idx;
    }
    /**
     * Get an object from the heap by index
     */
    getObject(idx) {
      return this.heap[idx];
    }
    /**
     * Drop an object reference, returning its slot to the free list
     */
    dropObject(idx) {
      if (idx < _WasmBindgenHeap.FIRST_FREE_IDX) return;
      this.heap[idx] = this.heapNext;
      this.heapNext = idx;
    }
    /**
     * Take an object from the heap (get and drop in one operation)
     */
    takeObject(idx) {
      const ret = this.getObject(idx);
      this.dropObject(idx);
      return ret;
    }
  };

  // src/workers/crypto.ts
  function getRandomValues(memory, ptr, len, workerId, pid) {
    console.log(
      `[worker:${workerId}:${pid}] __wbindgen_get_random_values called: ptr=${ptr}, len=${len}`
    );
    if (!memory) {
      console.error(
        `[worker:${workerId}:${pid}] CRYPTO ERROR: Memory not initialized`
      );
      throw new Error("Memory not initialized for getRandomValues");
    }
    const buf = new Uint8Array(memory.buffer, ptr, len);
    if (memory.buffer instanceof SharedArrayBuffer) {
      const copy = new Uint8Array(len);
      self.crypto.getRandomValues(copy);
      buf.set(copy);
      const allZeros = copy.every((b) => b === 0);
      if (allZeros && len > 0) {
        console.error(
          `[worker:${workerId}:${pid}] CRYPTO WARNING: getRandomValues returned all zeros!`
        );
      }
    } else {
      self.crypto.getRandomValues(buf);
    }
    if (len <= 32) {
      const preview = Array.from(buf.slice(0, Math.min(8, len))).map((b) => b.toString(16).padStart(2, "0")).join("");
      console.log(`[worker:${workerId}:${pid}] Random bytes preview: ${preview}...`);
    }
  }
  function fillRandomArray(array, workerId, pid) {
    if (array.buffer instanceof SharedArrayBuffer) {
      const copy = new Uint8Array(array.length);
      self.crypto.getRandomValues(copy);
      array.set(copy);
      const preview = Array.from(copy.slice(0, 8)).map((b) => b.toString(16).padStart(2, "0")).join("");
      console.log(
        `[worker:${workerId}:${pid}] PROXY random bytes (SharedAB): ${preview}...`
      );
    } else {
      self.crypto.getRandomValues(array);
      const preview = Array.from(array.slice(0, 8)).map((b) => b.toString(16).padStart(2, "0")).join("");
      console.log(`[worker:${workerId}:${pid}] PROXY random bytes: ${preview}...`);
    }
  }

  // src/workers/wasm-bindgen-shims.ts
  function decodeWasmString(memory, ptr, len) {
    if (!memory) return "";
    const sharedView = new Uint8Array(memory.buffer, ptr, len);
    const copied = new Uint8Array(len);
    copied.set(sharedView);
    return new TextDecoder().decode(copied);
  }
  function createWasmBindgenShims(heap, getMemory, workerId, pid) {
    const decode = (ptr, len) => decodeWasmString(getMemory(), ptr, len);
    return {
      __wbindgen_throw(ptr, len) {
        const memory = getMemory();
        if (!memory) {
          throw new Error("wasm-bindgen error: memory not initialized");
        }
        const msg = decode(ptr, len);
        throw new Error(msg);
      },
      __wbindgen_rethrow(arg) {
        throw arg;
      },
      __wbindgen_memory() {
        return getMemory();
      },
      __wbindgen_is_undefined(idx) {
        return heap.getObject(idx) === void 0;
      },
      __wbindgen_is_null(idx) {
        return heap.getObject(idx) === null;
      },
      __wbindgen_object_drop_ref(idx) {
        heap.dropObject(idx);
      },
      __wbindgen_object_clone_ref(idx) {
        return heap.addObject(heap.getObject(idx));
      },
      __wbindgen_string_new(ptr, len) {
        return heap.addObject(decode(ptr, len));
      },
      __wbindgen_string_get(val, retptr) {
        const memory = getMemory();
        if (!memory || typeof val !== "string") {
          const view2 = new DataView(memory.buffer);
          view2.setUint32(retptr, 0, true);
          view2.setUint32(retptr + 4, 0, true);
          return;
        }
        const encoded = new TextEncoder().encode(val);
        const view = new DataView(memory.buffer);
        view.setUint32(retptr, encoded.byteOffset, true);
        view.setUint32(retptr + 4, encoded.byteLength, true);
      },
      __wbindgen_number_get(val) {
        return typeof val === "number" ? val : void 0;
      },
      __wbindgen_boolean_get(val) {
        return typeof val === "boolean" ? val : void 0;
      },
      __wbindgen_jsval_eq(a, b) {
        return a === b;
      },
      __wbindgen_describe(_val) {
      },
      __wbindgen_error_new(ptr, len) {
        const msg = decode(ptr, len) || "Unknown error";
        return heap.addObject(new Error(msg));
      },
      __wbindgen_cb_drop(_idx) {
        return true;
      },
      __wbindgen_describe_cast() {
      },
      // Mangled version of __wbindgen_throw with hash suffix
      __wbg___wbindgen_throw_be289d5034ed271b(ptr, len) {
        const memory = getMemory();
        if (!memory) {
          throw new Error("wasm-bindgen error: memory not initialized");
        }
        const msg = decode(ptr, len);
        throw new Error(msg);
      },
      // Crypto API for getrandom
      __wbindgen_get_random_values(ptr, len) {
        const memory = getMemory();
        if (!memory) {
          throw new Error("Memory not initialized for getRandomValues");
        }
        getRandomValues(memory, ptr, len, workerId, pid);
      }
    };
  }
  function createExternrefShims() {
    const table = new WebAssembly.Table({
      initial: 128,
      maximum: 128,
      element: "externref"
    });
    return {
      table,
      shims: {
        __wbindgen_externref_table_grow(_delta) {
          return table.length;
        },
        __wbindgen_externref_table_set_null(idx) {
          if (idx < table.length) {
            table.set(idx, null);
          }
        }
      }
    };
  }
  function createWasmBindgenProxy(baseShims, heap, getMemory, workerId, pid) {
    const decode = (ptr, len) => decodeWasmString(getMemory(), ptr, len);
    const log = (msg) => console.log(`[worker:${workerId}:${pid}] ${msg}`);
    const warn = (msg) => console.warn(`[worker:${workerId}:${pid}] ${msg}`);
    const error = (msg) => console.error(`[worker:${workerId}:${pid}] ${msg}`);
    return {
      get(target, prop) {
        if (typeof prop !== "string") return void 0;
        if (prop in target) {
          return target[prop];
        }
        if (prop.includes("crypto") && !prop.includes("msCrypto")) {
          log(`PROXY: crypto getter shim matched for import: ${prop}`);
          return function(selfIdx) {
            const selfObj = heap.getObject(selfIdx);
            const crypto = selfObj?.crypto || self.crypto;
            log(
              `PROXY crypto getter: selfIdx=${selfIdx}, selfObj type=${typeof selfObj}, found crypto=${!!crypto}`
            );
            if (!crypto) {
              error(`PROXY crypto getter: FAILED - no crypto object found!`);
            }
            return heap.addObject(crypto);
          };
        }
        if (prop.includes("getRandomValues")) {
          log(`PROXY: getRandomValues shim matched for import: ${prop}`);
          return function(cryptoIdx, arrayIdx) {
            const cryptoObj = heap.getObject(cryptoIdx) || self.crypto;
            const array = heap.getObject(arrayIdx);
            log(
              `PROXY getRandomValues: cryptoIdx=${cryptoIdx}, arrayIdx=${arrayIdx}, arrayLen=${array?.length}`
            );
            if (array instanceof Uint8Array) {
              fillRandomArray(array, workerId, pid);
            } else {
              error(
                `PROXY getRandomValues: expected Uint8Array, got ${typeof array}`
              );
              throw new Error(
                `getRandomValues: expected Uint8Array, got ${typeof array}`
              );
            }
          };
        }
        if (prop.includes("randomFillSync")) {
          return function(_cryptoObj, ptr, len) {
            const memory = getMemory();
            if (!memory) {
              throw new Error("Memory not initialized for randomFillSync");
            }
            const buf = new Uint8Array(memory.buffer, ptr, len);
            if (memory.buffer instanceof SharedArrayBuffer) {
              const copy = new Uint8Array(len);
              self.crypto.getRandomValues(copy);
              buf.set(copy);
            } else {
              self.crypto.getRandomValues(buf);
            }
          };
        }
        if (prop.includes("static_accessor_SELF") || prop.includes("_self_")) {
          log(`PROXY: SELF accessor shim matched for import: ${prop}`);
          return function() {
            log(`PROXY SELF(): returning self (Web Worker global)`);
            return heap.addObject(self);
          };
        }
        if (prop.includes("static_accessor_WINDOW") || prop.includes("_window_")) {
          log(`PROXY: WINDOW accessor shim matched for import: ${prop}`);
          return function() {
            const obj = typeof window !== "undefined" ? window : void 0;
            log(
              `PROXY WINDOW(): window exists=${typeof window !== "undefined"}, returning ${obj === void 0 ? "0 (undefined)" : "window object"}`
            );
            if (obj === void 0) return 0;
            return heap.addObject(obj);
          };
        }
        if (prop.includes("static_accessor_GLOBAL_THIS") || prop.includes("_globalThis_")) {
          log(`PROXY: GLOBAL_THIS accessor shim matched for import: ${prop}`);
          return function() {
            log(
              `PROXY GLOBAL_THIS(): returning globalThis (has crypto=${!!globalThis.crypto})`
            );
            return heap.addObject(globalThis);
          };
        }
        if (prop.includes("static_accessor_GLOBAL") || prop.includes("_global_")) {
          log(`PROXY: GLOBAL accessor shim matched for import: ${prop}`);
          return function() {
            const obj = typeof global !== "undefined" ? global : self;
            log(
              `PROXY GLOBAL(): global exists=${typeof global !== "undefined"}, returning ${typeof global !== "undefined" ? "global" : "self (fallback)"}`
            );
            return heap.addObject(obj);
          };
        }
        if (prop.includes("new_no_args") || prop.includes("newnoargs")) {
          return function(ptr, len) {
            const str = decode(ptr, len);
            const fn = new Function(str);
            return heap.addObject(fn);
          };
        }
        if (prop.includes("is_object")) {
          return function(idx) {
            const val = heap.getObject(idx);
            return typeof val === "object" && val !== null;
          };
        }
        if (prop.includes("is_undefined")) {
          return function(idx) {
            return heap.getObject(idx) === void 0;
          };
        }
        if (prop.includes("is_string")) {
          return function(idx) {
            return typeof heap.getObject(idx) === "string";
          };
        }
        if (prop.includes("is_function")) {
          return function(idx) {
            return typeof heap.getObject(idx) === "function";
          };
        }
        if (prop.includes("new_with_length") || prop.includes("newwithlength")) {
          log(`PROXY: new_with_length shim matched for import: ${prop}`);
          return function(len) {
            const array = new Uint8Array(len);
            const idx = heap.addObject(array);
            log(`PROXY Uint8Array(${len}): created, heap idx=${idx}`);
            return idx;
          };
        }
        if (prop.includes("subarray")) {
          log(`PROXY: subarray shim matched for import: ${prop}`);
          return function(arrayIdx, start, end) {
            const array = heap.getObject(arrayIdx);
            const subarray = array.subarray(start, end);
            const idx = heap.addObject(subarray);
            log(
              `PROXY subarray: arrayIdx=${arrayIdx}, [${start}:${end}], len=${subarray.length}, heap idx=${idx}`
            );
            return idx;
          };
        }
        if (prop.includes("_length_")) {
          return function(arrayIdx) {
            const array = heap.getObject(arrayIdx);
            return array?.length || 0;
          };
        }
        if (prop.includes("prototypesetcall")) {
          log(`PROXY: prototypesetcall shim matched for import: ${prop}`);
          return function(wasmPtr, length, srcArrayIdx) {
            const srcArray = heap.getObject(srcArrayIdx);
            log(
              `PROXY prototypesetcall: wasmPtr=${wasmPtr}, length=${length}, srcArrayIdx=${srcArrayIdx}, srcArray type=${srcArray?.constructor?.name}`
            );
            const memory = getMemory();
            if (!memory || !memory.buffer) {
              error(`PROXY prototypesetcall: FAILED - wasmMemory not available!`);
              return;
            }
            if (!(srcArray instanceof Uint8Array)) {
              error(
                `PROXY prototypesetcall: FAILED - srcArray is not Uint8Array!`
              );
              return;
            }
            const dest = new Uint8Array(memory.buffer, wasmPtr, length);
            dest.set(srcArray.subarray(0, length));
            const preview = Array.from(srcArray.slice(0, Math.min(8, length))).map((b) => b.toString(16).padStart(2, "0")).join("");
            log(
              `PROXY prototypesetcall: SUCCESS - copied ${length} bytes to WASM ptr ${wasmPtr}, preview: ${preview}...`
            );
          };
        }
        if (prop.includes("set_") || prop.includes("_set_")) {
          log(`PROXY: set shim matched for import: ${prop}`);
          return function(arg0, arg1, arg2) {
            const src = heap.getObject(arg0);
            if (src instanceof Uint8Array && typeof arg1 === "number" && arg2 === void 0) {
              if (arg1 > 200 || !heap.getObject(arg1)) {
                log(
                  `PROXY set (raw_copy_to_ptr): copying ${src.length} bytes from JS array to WASM ptr ${arg1}`
                );
                const memory = getMemory();
                if (!memory || !memory.buffer) {
                  error(`PROXY set: FAILED - wasmMemory not available!`);
                  return;
                }
                const dest = new Uint8Array(memory.buffer, arg1, src.length);
                dest.set(src);
                const preview = Array.from(src.slice(0, Math.min(8, src.length))).map((b) => b.toString(16).padStart(2, "0")).join("");
                log(
                  `PROXY set: copied to WASM memory, preview: ${preview}...`
                );
                return;
              }
            }
            const target2 = heap.getObject(arg0);
            const srcObj = heap.getObject(arg1);
            log(
              `PROXY set (standard): targetIdx=${arg0}, srcIdx=${arg1}, offset=${arg2}, srcLen=${srcObj?.length}`
            );
            if (arg2 !== void 0) {
              target2.set(srcObj, arg2);
            } else {
              target2.set(srcObj);
            }
            if (srcObj && srcObj.length > 0) {
              const preview = Array.from(
                srcObj.slice(0, Math.min(8, srcObj.length))
              ).map((b) => b.toString(16).padStart(2, "0")).join("");
              log(`PROXY set data preview: ${preview}...`);
            }
          };
        }
        if (prop.includes("copy_to") || prop.includes("copyto") || prop.includes("raw_copy")) {
          log(`PROXY: copy_to shim matched for import: ${prop}`);
          return function(arrayIdx, ptr) {
            const array = heap.getObject(arrayIdx);
            log(
              `PROXY copy_to: arrayIdx=${arrayIdx}, ptr=${ptr}, len=${array?.length}`
            );
            const memory = getMemory();
            if (!memory || !memory.buffer) {
              error(`PROXY copy_to: FAILED - wasmMemory not available!`);
              return;
            }
            const dest = new Uint8Array(memory.buffer, ptr, array.length);
            dest.set(array);
            const preview = Array.from(array.slice(0, Math.min(8, array.length))).map((b) => b.toString(16).padStart(2, "0")).join("");
            log(
              `PROXY copy_to WASM memory: ${preview}... (${array.length} bytes to ptr ${ptr})`
            );
          };
        }
        if (prop.includes("_process_") || prop.includes("_versions_") || prop.includes("_node_") || prop.includes("_require_")) {
          return function() {
            return void 0;
          };
        }
        if (prop.includes("msCrypto")) {
          return function() {
            return void 0;
          };
        }
        if (prop.includes("_call_")) {
          return function(fnIdx, thisArgIdx, ...argIdxs) {
            const fn = heap.getObject(fnIdx);
            const thisArg = heap.getObject(thisArgIdx);
            const args = argIdxs.map((idx) => heap.getObject(idx));
            if (typeof fn === "function") {
              const result = fn.call(thisArg, ...args);
              if (result !== void 0 && result !== null && typeof result === "object") {
                return heap.addObject(result);
              }
              return result;
            }
            return 0;
          };
        }
        if (prop === "__wbindgen_is_object") {
          return function(val) {
            return typeof val === "object" && val !== null;
          };
        }
        if (prop === "__wbindgen_is_function") {
          return function(val) {
            return typeof val === "function";
          };
        }
        warn(`UNKNOWN wasm-bindgen import: ${prop}`);
        return function(...args) {
          const argTypes = args.map((a, i) => {
            if (typeof a === "number") {
              const heapObj = heap.getObject(a);
              if (heapObj !== void 0) {
                return `arg${i}=${a} (heap: ${heapObj?.constructor?.name || typeof heapObj})`;
              }
              return `arg${i}=${a} (number, possibly ptr)`;
            }
            return `arg${i}=${typeof a}`;
          });
          warn(
            `Called unknown import '${prop}' with args: [${argTypes.join(", ")}]`
          );
          return void 0;
        };
      }
    };
  }
  function createExternrefProxy(shims, workerId) {
    return {
      get(target, prop) {
        if (typeof prop !== "string") return void 0;
        if (prop in target) {
          return target[prop];
        }
        console.log(
          `[worker:${workerId}] Shimming unknown externref import: ${prop}`
        );
        return function() {
          return void 0;
        };
      }
    };
  }

  // src/workers/worker.ts
  var WORKER_MEMORY_ID = Math.floor(performance.timeOrigin);
  var state = createWorkerState(WORKER_MEMORY_ID);
  function postMemoryUpdate() {
    if (!state.wasmMemory) return;
    const msg = {
      type: "memory",
      pid: state.pid,
      buffer: state.wasmMemory.buffer,
      workerId: state.workerId
    };
    self.postMessage(msg);
  }
  function createSyscallImports() {
    return {
      zos_syscall: (syscall_num, arg0, arg1, arg2) => zos_syscall(state, postMemoryUpdate, syscall_num, arg0, arg1, arg2),
      zos_send_bytes: (ptr, len) => zos_send_bytes(state, postMemoryUpdate, ptr, len),
      zos_recv_bytes: (ptr, maxLen) => zos_recv_bytes(state, postMemoryUpdate, ptr, maxLen),
      zos_yield: () => zos_yield(state, postMemoryUpdate),
      zos_get_pid: () => zos_get_pid(state, postMemoryUpdate)
    };
  }
  self.onmessage = async (event) => {
    const data = event.data;
    if (data.type === "terminate") {
      self.close();
      return;
    }
    if (data.type === "ipc") {
      return;
    }
    if (state.initialized) {
      console.log(
        `[worker:${WORKER_MEMORY_ID}] Ignoring message after init:`,
        data.type || "unknown"
      );
      return;
    }
    const { binary, pid } = data;
    if (!binary || !pid) {
      console.error(
        `[worker:${WORKER_MEMORY_ID}] Invalid init message - missing binary or pid`
      );
      return;
    }
    state.pid = pid;
    try {
      const module = await WebAssembly.compile(binary);
      const imports = WebAssembly.Module.imports(module);
      const exports = WebAssembly.Module.exports(module);
      const importsMemory = imports.some(
        (imp) => imp.module === "env" && imp.name === "memory" && imp.kind === "memory"
      );
      const exportsMemory = exports.some(
        (exp) => exp.name === "memory" && exp.kind === "memory"
      );
      console.log(
        `[worker:${WORKER_MEMORY_ID}] Module imports memory: ${importsMemory}, exports memory: ${exportsMemory}`
      );
      const heap = new WasmBindgenHeap();
      const getMemory = () => state.wasmMemory;
      const baseShims = createWasmBindgenShims(
        heap,
        getMemory,
        WORKER_MEMORY_ID,
        pid
      );
      const bindgenProxy = createWasmBindgenProxy(
        baseShims,
        heap,
        getMemory,
        WORKER_MEMORY_ID,
        pid
      );
      const { shims: externrefShims } = createExternrefShims();
      const externrefProxy = createExternrefProxy(externrefShims, WORKER_MEMORY_ID);
      let sharedMemory = null;
      const importObject = {
        env: {
          ...createSyscallImports()
        },
        __wbindgen_externref_xform__: new Proxy(externrefShims, externrefProxy),
        __wbindgen_placeholder__: new Proxy(baseShims, bindgenProxy)
      };
      if (importsMemory) {
        sharedMemory = new WebAssembly.Memory({
          initial: 32,
          // 2MB initial (32 * 64KB)
          maximum: 64,
          // 4MB max (64 * 64KB)
          shared: true
        });
        importObject.env.memory = sharedMemory;
      }
      const instance = await WebAssembly.instantiate(module, importObject);
      const wasmExports = instance.exports;
      if (importsMemory && sharedMemory) {
        state.wasmMemory = sharedMemory;
        console.log(`[worker:${WORKER_MEMORY_ID}] Using imported shared memory`);
      } else if (exportsMemory && wasmExports.memory) {
        state.wasmMemory = wasmExports.memory;
        console.log(`[worker:${WORKER_MEMORY_ID}] Using module's exported memory`);
        if (!(state.wasmMemory.buffer instanceof SharedArrayBuffer)) {
          console.warn(
            `[worker:${WORKER_MEMORY_ID}] Module memory is not shared - atomics may not work correctly`
          );
        }
      } else {
        throw new Error("WASM module has no accessible memory");
      }
      state.mailboxView = new Int32Array(state.wasmMemory.buffer);
      state.mailboxBytes = new Uint8Array(state.wasmMemory.buffer);
      Atomics.store(state.mailboxView, MAILBOX_OFFSETS.PID, pid);
      if (pid === 1) {
        console.log(
          `[worker:${WORKER_MEMORY_ID}] Init (PID 1) sending SharedArrayBuffer to supervisor`
        );
        console.log(
          `[worker:${WORKER_MEMORY_ID}] Buffer is SharedArrayBuffer: ${state.wasmMemory.buffer instanceof SharedArrayBuffer}`
        );
        console.log(
          `[worker:${WORKER_MEMORY_ID}] Buffer size: ${state.wasmMemory.buffer.byteLength} bytes`
        );
        const markerView = new Uint32Array(state.wasmMemory.buffer);
        markerView[4096] = 3735928559;
        console.log(
          `[worker:${WORKER_MEMORY_ID}:${pid}] Wrote buffer marker 0xDEADBEEF at offset 16384`
        );
      }
      postMemoryUpdate();
      state.initialized = true;
      if (wasmExports.__zero_rt_init) {
        console.log(`[worker:${WORKER_MEMORY_ID}:${pid}] Calling __zero_rt_init`);
        wasmExports.__zero_rt_init(0);
      }
      if (wasmExports._start) {
        console.log(
          `[worker:${WORKER_MEMORY_ID}:${pid}] Calling _start() - process will block on atomics`
        );
        wasmExports._start();
        console.log(
          `[worker:${WORKER_MEMORY_ID}:${pid}] _start() returned (should never happen)`
        );
      } else {
        console.error(`[worker:${WORKER_MEMORY_ID}:${pid}] No _start export found!`);
      }
    } catch (e) {
      const error = e;
      console.error(`[worker:${WORKER_MEMORY_ID}] Error:`, error);
      const msg = {
        type: "error",
        pid: state.pid,
        workerId: WORKER_MEMORY_ID,
        error: error.message
      };
      self.postMessage(msg);
    }
  };
})();
