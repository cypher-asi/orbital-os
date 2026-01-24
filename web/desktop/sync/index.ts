/**
 * Desktop Sync Module
 * 
 * Re-exports sync utilities for the desktop render loop and callbacks.
 */

export { syncStoresFromFrame, resetSyncState } from './renderLoopSync';
export { registerStoreCallbacks, type CallbackCleanup } from './callbackSync';
