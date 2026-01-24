import { useCallback, createContext, useContext } from 'react';
import { useSupervisor } from './useSupervisor';
import {
  OBJECT_TYPE,
  encodePermissions,
  type ObjectType,
  type Permissions,
  type CapabilityRequest,
} from '../../apps/shared/app-protocol';
import {
  usePermissionStore,
  selectPendingRequest,
  selectAllGrantedCapabilities,
  selectPermissionIsLoading,
  type AppManifest,
  type CapabilityInfo,
  type PermissionRequest,
} from '../../stores';

// =============================================================================
// Re-export Types from Store
// =============================================================================

export type { AppManifest, CapabilityInfo, PermissionRequest };

/**
 * Permissions hook state
 */
export interface UsePermissionsState {
  /** Current pending permission request (if any) */
  pendingRequest: PermissionRequest | null;
  /** Map of PID -> granted capabilities */
  grantedCapabilities: Map<number, CapabilityInfo[]>;
  /** Whether a permission operation is in progress */
  isLoading: boolean;
}

/**
 * Permissions hook actions
 */
export interface UsePermissionsActions {
  /** Request permissions for an app */
  requestPermissions: (
    pid: number,
    app: AppManifest,
    onComplete?: (success: boolean) => void
  ) => void;
  /** Grant permissions after user approval */
  grantPermissions: (
    pid: number,
    capabilities: CapabilityRequest[]
  ) => Promise<boolean>;
  /** Revoke a permission */
  revokePermission: (pid: number, objectType: ObjectType) => Promise<boolean>;
  /** Revoke all permissions for a process */
  revokeAllPermissions: (pid: number) => Promise<boolean>;
  /** Clear pending request (user denied or dismissed) */
  clearPendingRequest: () => void;
  /** Get granted capabilities for a process */
  getGrantedCaps: (pid: number) => CapabilityInfo[];
}

// =============================================================================
// Hook Implementation (now backed by Zustand store)
// =============================================================================

/**
 * Hook for managing app permissions.
 *
 * Provides an interface for:
 * - Requesting permissions (shows dialog)
 * - Granting permissions (via Init)
 * - Revoking permissions (via Init)
 * - Tracking granted capabilities
 * 
 * @deprecated Use `usePermissionStore` directly for better performance.
 * This hook is kept for backward compatibility.
 */
export function usePermissions(): UsePermissionsState & UsePermissionsActions {
  const supervisor = useSupervisor();
  const store = usePermissionStore();
  
  // Get state from store via selectors
  const pendingRequest = usePermissionStore(selectPendingRequest);
  const grantedCapabilities = usePermissionStore(selectAllGrantedCapabilities);
  const isLoading = usePermissionStore(selectPermissionIsLoading);

  /**
   * Request permissions for an app - shows the permission dialog
   */
  const requestPermissions = useCallback(
    (
      pid: number,
      app: AppManifest,
      onComplete?: (success: boolean) => void
    ) => {
      // Factory apps are auto-granted basic capabilities
      if (app.isFactory) {
        // Auto-grant Endpoint for factory apps
        const factoryCaps: CapabilityInfo[] = [
          {
            slot: 0,
            objectType: 'Endpoint',
            permissions: { read: true, write: true, grant: false },
          },
        ];
        store.grantCapabilities(pid, factoryCaps);
        onComplete?.(true);
        return;
      }

      // For third-party apps, show the permission dialog
      store.setPendingRequest({
        app,
        pid,
        onApprove: async (approved) => {
          store.setPendingRequest(null);
          const success = await grantPermissionsInternal(pid, approved);
          onComplete?.(success);
        },
        onDeny: () => {
          store.setPendingRequest(null);
          onComplete?.(false);
        },
      });
    },
    [store]
  );

  /**
   * Internal function to grant permissions via Init
   */
  const grantPermissionsInternal = async (
    pid: number,
    capabilities: CapabilityRequest[]
  ): Promise<boolean> => {
    if (!supervisor) {
      console.error('Supervisor not available');
      return false;
    }

    store.setLoading(true);

    try {
      const grantedCaps: CapabilityInfo[] = [];

      for (const cap of capabilities) {
        // Encode grant request
        const objectTypeValue = OBJECT_TYPE[cap.objectType];
        const permsValue = encodePermissions(cap.permissions);
        const reasonBytes = new TextEncoder().encode(cap.reason);

        // Build message: [target_pid: u32, object_type: u8, perms: u8, reason_len: u16, reason]
        const msgSize = 4 + 1 + 1 + 2 + reasonBytes.length;
        const msgData = new Uint8Array(msgSize);
        let offset = 0;

        // Target PID (little-endian)
        msgData[offset++] = pid & 0xff;
        msgData[offset++] = (pid >> 8) & 0xff;
        msgData[offset++] = (pid >> 16) & 0xff;
        msgData[offset++] = (pid >> 24) & 0xff;

        // Object type
        msgData[offset++] = objectTypeValue;

        // Permissions
        msgData[offset++] = permsValue;

        // Reason (length-prefixed)
        msgData[offset++] = reasonBytes.length & 0xff;
        msgData[offset++] = (reasonBytes.length >> 8) & 0xff;
        msgData.set(reasonBytes, offset);

        // Send to Init via supervisor IPC
        // The supervisor intercepts INIT:GRANT messages and forwards to init process
        const hex = Array.from(msgData)
          .map((b) => b.toString(16).padStart(2, '0'))
          .join('');
        supervisor.send_input(`grant ${pid} ${hex}`);

        // For now, assume success and track locally
        // In a full implementation, we'd wait for MSG_PERMISSION_RESPONSE
        grantedCaps.push({
          slot: grantedCaps.length, // Placeholder slot
          objectType: cap.objectType,
          permissions: cap.permissions,
        });
      }

      // Update store
      store.grantCapabilities(pid, grantedCaps);

      return true;
    } catch (error) {
      console.error('Failed to grant permissions:', error);
      return false;
    } finally {
      store.setLoading(false);
    }
  };

  /**
   * Grant permissions (public API)
   */
  const grantPermissions = useCallback(
    async (pid: number, capabilities: CapabilityRequest[]): Promise<boolean> => {
      return grantPermissionsInternal(pid, capabilities);
    },
    [supervisor, store]
  );

  /**
   * Revoke a single permission using direct supervisor API
   */
  const revokePermission = useCallback(
    async (pid: number, objectType: ObjectType): Promise<boolean> => {
      if (!supervisor) {
        console.error('Supervisor not available');
        return false;
      }

      store.setLoading(true);

      try {
        // Find the capability slot for this object type from store
        const caps = store.getGrantedCaps(pid);
        const cap = caps.find((c) => c.objectType === objectType);
        
        if (!cap) {
          console.warn(`No capability found for PID ${pid} objectType ${objectType}`);
          return false;
        }

        // Use direct supervisor API to revoke the capability
        // Note: pid must be BigInt for wasm-bindgen u64 parameter
        const success = supervisor.revoke_capability(BigInt(pid), cap.slot);
        
        if (success) {
          // Update store
          store.revokeCapability(pid, objectType);
        }

        return success;
      } catch (error) {
        console.error('Failed to revoke permission:', error);
        return false;
      } finally {
        store.setLoading(false);
      }
    },
    [supervisor, store]
  );

  /**
   * Revoke all permissions for a process
   */
  const revokeAllPermissions = useCallback(
    async (pid: number): Promise<boolean> => {
      const caps = store.getGrantedCaps(pid);
      let allSuccess = true;

      for (const cap of caps) {
        const success = await revokePermission(pid, cap.objectType);
        if (!success) {
          allSuccess = false;
        }
      }

      return allSuccess;
    },
    [store, revokePermission]
  );

  /**
   * Clear pending request
   */
  const clearPendingRequest = useCallback(() => {
    store.clearPendingRequest();
  }, [store]);

  /**
   * Get granted capabilities for a process
   */
  const getGrantedCaps = useCallback(
    (pid: number): CapabilityInfo[] => {
      return store.getGrantedCaps(pid);
    },
    [store]
  );

  return {
    // State
    pendingRequest,
    grantedCapabilities,
    isLoading,
    // Actions
    requestPermissions,
    grantPermissions,
    revokePermission,
    revokeAllPermissions,
    clearPendingRequest,
    getGrantedCaps,
  };
}

// =============================================================================
// Context for sharing permissions state across the app
// =============================================================================

export type PermissionsContextType = UsePermissionsState & UsePermissionsActions;

export const PermissionsContext = createContext<PermissionsContextType | null>(null);

/**
 * Hook to access the permissions context.
 * Must be used within a PermissionsProvider.
 */
export function usePermissionsContext(): PermissionsContextType | null {
  return useContext(PermissionsContext);
}

/**
 * Provider component for permissions state.
 */
export const PermissionsProvider = PermissionsContext.Provider;
