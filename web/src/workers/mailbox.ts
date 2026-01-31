/**
 * Zero OS Worker - Mailbox Operations
 *
 * Syscall mailbox functions for communicating with the supervisor via SharedArrayBuffer.
 */

import {
  STATUS_IDLE,
  STATUS_PENDING,
  MAILBOX_OFFSETS,
  MAILBOX_DATA_BYTE_OFFSET,
  MAILBOX_MAX_DATA_LEN,
  type WorkerState,
} from './types';

/**
 * Refresh typed array views if memory buffer has changed (e.g., after memory.grow())
 */
export function refreshViews(
  state: WorkerState,
  postMemoryUpdate: () => void
): void {
  if (
    state.wasmMemory &&
    state.mailboxView &&
    state.mailboxView.buffer !== state.wasmMemory.buffer
  ) {
    console.log(
      `[worker:${state.workerId}:${state.pid}] MEMORY GREW! Old buffer detached, creating new views. New size: ${state.wasmMemory.buffer.byteLength} bytes`
    );

    // CRITICAL: Save mailbox state from old buffer before it's detached
    const oldMailboxView = state.mailboxView;
    const oldMailboxBytes = state.mailboxBytes!;
    const savedStatus = Atomics.load(oldMailboxView, MAILBOX_OFFSETS.STATUS);
    const savedDataLen = Atomics.load(oldMailboxView, MAILBOX_OFFSETS.DATA_LEN);
    const savedResult = Atomics.load(oldMailboxView, MAILBOX_OFFSETS.RESULT);

    // Copy data buffer if there's pending data
    let savedData: Uint8Array | null = null;
    if (savedDataLen > 0) {
      savedData = new Uint8Array(savedDataLen);
      savedData.set(
        oldMailboxBytes.slice(
          MAILBOX_DATA_BYTE_OFFSET,
          MAILBOX_DATA_BYTE_OFFSET + savedDataLen
        )
      );
      console.log(
        `[worker:${state.workerId}:${state.pid}] Preserved ${savedDataLen} bytes of mailbox data across memory growth`
      );
    }

    // Update local views to new buffer
    state.mailboxView = new Int32Array(state.wasmMemory.buffer);
    state.mailboxBytes = new Uint8Array(state.wasmMemory.buffer);

    // Restore mailbox state to new buffer
    Atomics.store(state.mailboxView, MAILBOX_OFFSETS.STATUS, savedStatus);
    Atomics.store(state.mailboxView, MAILBOX_OFFSETS.DATA_LEN, savedDataLen);
    Atomics.store(state.mailboxView, MAILBOX_OFFSETS.RESULT, savedResult);
    Atomics.store(state.mailboxView, MAILBOX_OFFSETS.PID, state.pid);

    // Restore data buffer
    if (savedData) {
      state.mailboxBytes.set(savedData, MAILBOX_DATA_BYTE_OFFSET);
    }

    // CRITICAL: Re-send the new buffer to the supervisor
    postMemoryUpdate();

    console.log(
      `[worker:${state.workerId}:${state.pid}] Sent new SharedArrayBuffer to supervisor after memory growth`
    );
  }
}

/**
 * Make a syscall using SharedArrayBuffer + Atomics
 * Returns bigint (i64) to match the WASM extern declaration
 */
export function zos_syscall(
  state: WorkerState,
  postMemoryUpdate: () => void,
  syscall_num: number,
  arg0: number,
  arg1: number,
  arg2: number
): bigint {
  refreshViews(state, postMemoryUpdate);
  const view = state.mailboxView!;

  // Write syscall parameters
  Atomics.store(view, MAILBOX_OFFSETS.SYSCALL_NUM, syscall_num);
  Atomics.store(view, MAILBOX_OFFSETS.ARG0, arg0);
  Atomics.store(view, MAILBOX_OFFSETS.ARG1, arg1);
  Atomics.store(view, MAILBOX_OFFSETS.ARG2, arg2);

  // Set status to PENDING (signals the supervisor)
  Atomics.store(view, MAILBOX_OFFSETS.STATUS, STATUS_PENDING);

  // Wait for supervisor to process the syscall
  while (true) {
    Atomics.wait(view, MAILBOX_OFFSETS.STATUS, STATUS_PENDING, 1000);
    const status = Atomics.load(view, MAILBOX_OFFSETS.STATUS);
    if (status !== STATUS_PENDING) {
      break;
    }
  }

  // Read the result (as i64/BigInt to match WASM extern declaration)
  // Note: Mailbox currently stores only 32-bit result. For full 64-bit support,
  // we'd need to extend the mailbox format with RESULT_HIGH.
  const result = Atomics.load(view, MAILBOX_OFFSETS.RESULT);

  // Reset status to IDLE
  Atomics.store(view, MAILBOX_OFFSETS.STATUS, STATUS_IDLE);

  // Return as BigInt to match the i64 return type expected by WASM
  return BigInt(result);
}

/**
 * Send bytes to the syscall data buffer
 * Must be called before zos_syscall when the syscall needs data
 */
export function zos_send_bytes(
  state: WorkerState,
  postMemoryUpdate: () => void,
  ptr: number,
  len: number
): number {
  refreshViews(state, postMemoryUpdate);

  const actualLen = Math.min(len, MAILBOX_MAX_DATA_LEN);

  if (actualLen > 0 && state.wasmMemory) {
    // Copy data from WASM linear memory (at ptr) to mailbox data buffer
    const srcBytes = new Uint8Array(state.wasmMemory.buffer, ptr, actualLen);
    state.mailboxBytes!.set(srcBytes, MAILBOX_DATA_BYTE_OFFSET);
  }

  // Store data length
  Atomics.store(state.mailboxView!, MAILBOX_OFFSETS.DATA_LEN, actualLen);

  return actualLen;
}

/**
 * Receive bytes from the syscall result buffer
 */
export function zos_recv_bytes(
  state: WorkerState,
  postMemoryUpdate: () => void,
  ptr: number,
  maxLen: number
): number {
  refreshViews(state, postMemoryUpdate);

  const dataLen = Atomics.load(state.mailboxView!, MAILBOX_OFFSETS.DATA_LEN);
  const actualLen = Math.min(dataLen, maxLen);

  // DEBUG: Log for Init when receiving 0 bytes but expecting data
  if (state.pid === 1 && dataLen === 0 && maxLen > 0 && state.wasmMemory) {
    const markerView = new Uint32Array(state.wasmMemory.buffer);
    const marker = markerView[4096];
    console.error(
      `[worker:${state.workerId}:${state.pid}] RECV DEBUG: dataLen=0 but maxLen=${maxLen}. ` +
        `Buffer size: ${state.wasmMemory.buffer.byteLength}, ` +
        `Buffer is SharedArrayBuffer: ${state.wasmMemory.buffer instanceof SharedArrayBuffer}, ` +
        `Marker at offset 16384: 0x${marker.toString(16).toUpperCase().padStart(8, '0')}`
    );
  }

  if (actualLen > 0 && state.wasmMemory) {
    const dstBytes = new Uint8Array(state.wasmMemory.buffer, ptr, actualLen);
    dstBytes.set(
      state.mailboxBytes!.slice(
        MAILBOX_DATA_BYTE_OFFSET,
        MAILBOX_DATA_BYTE_OFFSET + actualLen
      )
    );
  }

  return actualLen;
}

/**
 * Yield the current process's time slice
 */
export function zos_yield(
  state: WorkerState,
  postMemoryUpdate: () => void
): void {
  refreshViews(state, postMemoryUpdate);
  // Wait up to 1ms before returning - prevents busy-loop when no messages
  Atomics.wait(state.mailboxView!, MAILBOX_OFFSETS.STATUS, STATUS_IDLE, 1);
}

/**
 * Get the process's assigned PID
 */
export function zos_get_pid(
  state: WorkerState,
  postMemoryUpdate: () => void
): number {
  refreshViews(state, postMemoryUpdate);
  return Atomics.load(state.mailboxView!, MAILBOX_OFFSETS.PID);
}
