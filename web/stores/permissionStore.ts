/**
 * Permission Store - Centralized state for app permission management.
 * 
 * Manages capability permission requests, grants, and revocations.
 * Replaces the PermissionsContext and usePermissions hook state.
 */

import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';
import type { ObjectType, Permissions, CapabilityRequest } from '../apps/shared/app-protocol';

// =============================================================================
// Permission Types
// =============================================================================

/**
 * App manifest for permission dialog
 */
export interface AppManifest {
  id: string;
  name: string;
  version: string;
  description: string;
  capabilities: CapabilityRequest[];
  isFactory?: boolean;
}

/**
 * Information about a granted capability
 */
export interface CapabilityInfo {
  slot: number;
  objectType: ObjectType;
  permissions: Permissions;
}

/**
 * Permission request state (shown in dialog)
 */
export interface PermissionRequest {
  /** App requesting permissions */
  app: AppManifest;
  /** Process ID of the app */
  pid: number;
  /** Callback when user approves */
  onApprove: (approved: CapabilityRequest[]) => void;
  /** Callback when user denies */
  onDeny: () => void;
}

// =============================================================================
// Store Types
// =============================================================================

interface PermissionStoreState {
  pendingRequest: PermissionRequest | null;
  grantedCapabilities: Map<number, CapabilityInfo[]>;
  isLoading: boolean;
  
  // Actions
  setPendingRequest: (request: PermissionRequest | null) => void;
  grantCapabilities: (pid: number, caps: CapabilityInfo[]) => void;
  revokeCapability: (pid: number, objectType: ObjectType) => void;
  revokeAllCapabilities: (pid: number) => void;
  clearPendingRequest: () => void;
  setLoading: (loading: boolean) => void;
  getGrantedCaps: (pid: number) => CapabilityInfo[];
}

// =============================================================================
// Store Creation
// =============================================================================

export const usePermissionStore = create<PermissionStoreState>()(
  subscribeWithSelector((set, get) => ({
    pendingRequest: null,
    grantedCapabilities: new Map(),
    isLoading: false,
    
    setPendingRequest: (pendingRequest) => set({ pendingRequest }),
    
    grantCapabilities: (pid, caps) => set((state) => {
      const next = new Map(state.grantedCapabilities);
      const existing = next.get(pid) || [];
      next.set(pid, [...existing, ...caps]);
      return { grantedCapabilities: next };
    }),
    
    revokeCapability: (pid, objectType) => set((state) => {
      const next = new Map(state.grantedCapabilities);
      const existing = next.get(pid) || [];
      next.set(pid, existing.filter(c => c.objectType !== objectType));
      return { grantedCapabilities: next };
    }),
    
    revokeAllCapabilities: (pid) => set((state) => {
      const next = new Map(state.grantedCapabilities);
      next.delete(pid);
      return { grantedCapabilities: next };
    }),
    
    clearPendingRequest: () => {
      const pending = get().pendingRequest;
      if (pending) {
        pending.onDeny();
      }
      set({ pendingRequest: null });
    },
    
    setLoading: (isLoading) => set({ isLoading }),
    
    getGrantedCaps: (pid) => {
      return get().grantedCapabilities.get(pid) || [];
    },
  }))
);

// =============================================================================
// Selectors for Fine-Grained Subscriptions
// =============================================================================

/** Select pending permission request */
export const selectPendingRequest = (state: PermissionStoreState) => state.pendingRequest;

/** Select loading state */
export const selectIsLoading = (state: PermissionStoreState) => state.isLoading;

/** Select all granted capabilities (the Map) */
export const selectAllGrantedCapabilities = (state: PermissionStoreState) => 
  state.grantedCapabilities;

/** Select granted capabilities for a specific PID (returns selector function) */
export const selectGrantedCapabilities = (pid: number) => (state: PermissionStoreState) =>
  state.grantedCapabilities.get(pid) || [];

/** Select whether a PID has any capabilities */
export const selectHasCapabilities = (pid: number) => (state: PermissionStoreState) =>
  (state.grantedCapabilities.get(pid)?.length ?? 0) > 0;

/** Select count of processes with capabilities */
export const selectProcessCount = (state: PermissionStoreState) => 
  state.grantedCapabilities.size;

/** Select total capability count across all processes */
export const selectTotalCapabilityCount = (state: PermissionStoreState) => {
  let total = 0;
  for (const caps of state.grantedCapabilities.values()) {
    total += caps.length;
  }
  return total;
};
