import { useState, useCallback, createContext, useContext } from 'react';
import { useSupervisor } from './useSupervisor';
import {
  OBJECT_TYPE,
  encodePermissions,
  type ObjectType,
  type Permissions,
  type CapabilityRequest,
} from '../../apps/shared/app-protocol';

// =============================================================================
// Types
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
 * Permission request state
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
// Hook Implementation
// =============================================================================

/**
 * Hook for managing app permissions.
 *
 * Provides an interface for:
 * - Requesting permissions (shows dialog)
 * - Granting permissions (via Init)
 * - Revoking permissions (via Init)
 * - Tracking granted capabilities
 */
export function usePermissions(): UsePermissionsState & UsePermissionsActions {
  const supervisor = useSupervisor();

  const [pendingRequest, setPendingRequest] = useState<PermissionRequest | null>(null);
  const [grantedCapabilities, setGrantedCapabilities] = useState<Map<number, CapabilityInfo[]>>(
    new Map()
  );
  const [isLoading, setIsLoading] = useState(false);

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
        setGrantedCapabilities((prev) => {
          const next = new Map(prev);
          next.set(pid, factoryCaps);
          return next;
        });
        onComplete?.(true);
        return;
      }

      // For third-party apps, show the permission dialog
      setPendingRequest({
        app,
        pid,
        onApprove: async (approved) => {
          setPendingRequest(null);
          const success = await grantPermissionsInternal(pid, approved);
          onComplete?.(success);
        },
        onDeny: () => {
          setPendingRequest(null);
          onComplete?.(false);
        },
      });
    },
    []
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

    setIsLoading(true);

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

      // Update local tracking
      setGrantedCapabilities((prev) => {
        const next = new Map(prev);
        const existing = next.get(pid) || [];
        next.set(pid, [...existing, ...grantedCaps]);
        return next;
      });

      return true;
    } catch (error) {
      console.error('Failed to grant permissions:', error);
      return false;
    } finally {
      setIsLoading(false);
    }
  };

  /**
   * Grant permissions (public API)
   */
  const grantPermissions = useCallback(
    async (pid: number, capabilities: CapabilityRequest[]): Promise<boolean> => {
      return grantPermissionsInternal(pid, capabilities);
    },
    [supervisor]
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

      setIsLoading(true);

      try {
        // Find the capability slot for this object type from local tracking
        const caps = grantedCapabilities.get(pid) || [];
        const cap = caps.find((c) => c.objectType === objectType);
        
        if (!cap) {
          console.warn(`No capability found for PID ${pid} objectType ${objectType}`);
          return false;
        }

        // Use direct supervisor API to revoke the capability
        // Note: pid must be BigInt for wasm-bindgen u64 parameter
        const success = supervisor.revoke_capability(BigInt(pid), cap.slot);
        
        if (success) {
          // Update local tracking
          setGrantedCapabilities((prev) => {
            const next = new Map(prev);
            const existing = next.get(pid) || [];
            next.set(
              pid,
              existing.filter((c) => c.objectType !== objectType)
            );
            return next;
          });
        }

        return success;
      } catch (error) {
        console.error('Failed to revoke permission:', error);
        return false;
      } finally {
        setIsLoading(false);
      }
    },
    [supervisor, grantedCapabilities]
  );

  /**
   * Revoke all permissions for a process
   */
  const revokeAllPermissions = useCallback(
    async (pid: number): Promise<boolean> => {
      const caps = grantedCapabilities.get(pid) || [];
      let allSuccess = true;

      for (const cap of caps) {
        const success = await revokePermission(pid, cap.objectType);
        if (!success) {
          allSuccess = false;
        }
      }

      return allSuccess;
    },
    [grantedCapabilities, revokePermission]
  );

  /**
   * Clear pending request
   */
  const clearPendingRequest = useCallback(() => {
    if (pendingRequest) {
      pendingRequest.onDeny();
    }
    setPendingRequest(null);
  }, [pendingRequest]);

  /**
   * Get granted capabilities for a process
   */
  const getGrantedCaps = useCallback(
    (pid: number): CapabilityInfo[] => {
      return grantedCapabilities.get(pid) || [];
    },
    [grantedCapabilities]
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
