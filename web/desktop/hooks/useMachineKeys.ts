import { useCallback, useEffect, useRef } from 'react';
import { useShallow } from 'zustand/react/shallow';
import { useIdentityStore, selectCurrentUser, useMachineKeysStore, selectMachineKeysState } from '../../stores';
import { useSupervisor } from './useSupervisor';
import {
  IdentityServiceClient,
  type MachineKeyCapabilities as ServiceMachineKeyCapabilities,
  type MachineKeyRecord as ServiceMachineKeyRecord,
  type Supervisor,
  VfsStorageClient,
  getMachineKeysDir,
} from '../../services';

// Re-export types from store for backward compatibility
export type {
  MachineKeyCapabilities,
  MachineKeyRecord,
  MachineKeysState,
} from '../../stores';

/**
 * Hook return type
 */
export interface UseMachineKeysReturn {
  /** Current state */
  state: import('../../stores').MachineKeysState;
  /** List all machine keys for current user */
  listMachineKeys: () => Promise<import('../../stores').MachineKeyRecord[]>;
  /** Get a specific machine key */
  getMachineKey: (machineId: string) => Promise<import('../../stores').MachineKeyRecord | null>;
  /** Create a new machine key */
  createMachineKey: (machineName?: string, capabilities?: Partial<import('../../stores').MachineKeyCapabilities>) => Promise<import('../../stores').MachineKeyRecord>;
  /** Revoke a machine key */
  revokeMachineKey: (machineId: string) => Promise<void>;
  /** Rotate a machine key (new epoch) */
  rotateMachineKey: (machineId: string) => Promise<import('../../stores').MachineKeyRecord>;
  /** Refresh state */
  refresh: () => Promise<void>;
}

// =============================================================================
// Response conversion helpers
// =============================================================================

function bytesToHex(bytes: number[] | string): string {
  if (typeof bytes === 'string') return bytes;
  return bytes.map(b => b.toString(16).padStart(2, '0')).join('');
}

/**
 * Convert service capabilities to public API format
 */
function convertCapabilities(caps: ServiceMachineKeyCapabilities): import('../../stores').MachineKeyCapabilities {
  return {
    canAuthenticate: caps.can_authenticate,
    canEncrypt: caps.can_encrypt,
    canSignMessages: caps.can_sign_messages,
    canAuthorizeMachines: caps.can_authorize_machines,
    canRevokeMachines: caps.can_revoke_machines,
    expiresAt: caps.expires_at,
  };
}

/**
 * Convert public API capabilities to service format
 */
function convertCapabilitiesForService(caps: Partial<import('../../stores').MachineKeyCapabilities>): ServiceMachineKeyCapabilities {
  return {
    can_authenticate: caps.canAuthenticate ?? true,
    can_encrypt: caps.canEncrypt ?? true,
    can_sign_messages: caps.canSignMessages ?? false,
    can_authorize_machines: caps.canAuthorizeMachines ?? false,
    can_revoke_machines: caps.canRevokeMachines ?? false,
    expires_at: caps.expiresAt ?? null,
  };
}

/**
 * Convert service machine record to public API format
 */
function convertMachineRecord(record: ServiceMachineKeyRecord, currentMachineId?: string): import('../../stores').MachineKeyRecord {
  // machine_id comes as a number from JSON, convert to hex string
  const machineIdHex = typeof record.machine_id === 'number'
    ? '0x' + record.machine_id.toString(16).padStart(32, '0')
    : record.machine_id.toString();
  
  const authorizedByHex = typeof record.authorized_by === 'number'
    ? '0x' + record.authorized_by.toString(16).padStart(32, '0')
    : record.authorized_by.toString();

  return {
    machineId: machineIdHex,
    signingPublicKey: bytesToHex(record.signing_public_key),
    encryptionPublicKey: bytesToHex(record.encryption_public_key),
    authorizedAt: record.authorized_at,
    authorizedBy: authorizedByHex,
    capabilities: convertCapabilities(record.capabilities),
    machineName: record.machine_name,
    lastSeenAt: record.last_seen_at,
    isCurrentDevice: machineIdHex === currentMachineId,
    epoch: record.epoch ?? 1, // Use service value, fallback to 1 for backward compatibility
  };
}

// =============================================================================
// Hook Implementation
// =============================================================================

export function useMachineKeys(): UseMachineKeysReturn {
  const currentUser = useIdentityStore(selectCurrentUser);
  const supervisor = useSupervisor();
  
  // Use Zustand store for shared state
  // useShallow prevents infinite loops by comparing object values instead of references
  const state = useMachineKeysStore(useShallow(selectMachineKeysState));
  const setMachines = useMachineKeysStore((s) => s.setMachines);
  const addMachine = useMachineKeysStore((s) => s.addMachine);
  const removeMachine = useMachineKeysStore((s) => s.removeMachine);
  const updateMachine = useMachineKeysStore((s) => s.updateMachine);
  const setLoading = useMachineKeysStore((s) => s.setLoading);
  const setError = useMachineKeysStore((s) => s.setError);
  const setInitializing = useMachineKeysStore((s) => s.setInitializing);
  const reset = useMachineKeysStore((s) => s.reset);

  // Create a stable reference to the IdentityServiceClient
  const clientRef = useRef<IdentityServiceClient | null>(null);

  // Initialize client when supervisor becomes available
  useEffect(() => {
    if (supervisor && !clientRef.current) {
      clientRef.current = new IdentityServiceClient(supervisor as unknown as Supervisor);
      console.log('[useMachineKeys] IdentityServiceClient initialized');
    }
  }, [supervisor]);

  // Get user ID as BigInt for client API
  const getUserIdBigInt = useCallback((): bigint | null => {
    const userId = currentUser?.id;
    if (!userId) return null;
    if (typeof userId === 'string') {
      if (userId.startsWith('0x')) {
        return BigInt(userId);
      }
      try {
        return BigInt(userId);
      } catch {
        return null;
      }
    }
    return BigInt(userId);
  }, [currentUser?.id]);

  const listMachineKeys = useCallback(async (): Promise<import('../../stores').MachineKeyRecord[]> => {
    const userId = getUserIdBigInt();
    if (!userId) {
      throw new Error('No user logged in');
    }

    // Read directly from VfsStorage cache (synchronous, no IPC deadlock)
    const machineDir = getMachineKeysDir(userId);

    console.log(`[useMachineKeys] Listing machine keys from VFS cache: ${machineDir}`);

    if (!VfsStorageClient.isAvailable()) {
      console.warn('[useMachineKeys] VfsStorage not available yet');
      throw new Error('VFS cache not ready');
    }

    setLoading(true);

    try {
      // List children of the machine keys directory
      const children = VfsStorageClient.listChildrenSync(machineDir);
      const machines: import('../../stores').MachineKeyRecord[] = [];

      // Read each machine key file
      for (const child of children) {
        if (!child.name.endsWith('.json')) continue;

        const content = VfsStorageClient.readJsonSync<ServiceMachineKeyRecord>(child.path);
        if (content) {
          machines.push(convertMachineRecord(content, state.currentMachineId || undefined));
        }
      }

      console.log(`[useMachineKeys] Found ${machines.length} machine keys in VFS cache`);

      setMachines(machines);

      return machines;
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to list machine keys';
      console.error('[useMachineKeys] listMachineKeys error:', errorMsg);
      setError(errorMsg);
      throw err;
    }
  }, [getUserIdBigInt, state.currentMachineId, setLoading, setMachines, setError]);

  const getMachineKey = useCallback(async (machineId: string): Promise<import('../../stores').MachineKeyRecord | null> => {
    // For now, look up in current state
    // Could add a specific get endpoint later
    return state.machines.find(m => m.machineId === machineId) || null;
  }, [state.machines]);

  const createMachineKey = useCallback(async (
    machineName?: string,
    capabilities?: Partial<import('../../stores').MachineKeyCapabilities>
  ): Promise<import('../../stores').MachineKeyRecord> => {
    const userId = getUserIdBigInt();
    if (!userId) {
      throw new Error('No user logged in');
    }

    const client = clientRef.current;
    if (!client) {
      throw new Error('Identity service client not available');
    }

    setLoading(true);

    try {
      console.log(`[useMachineKeys] Creating machine key for user ${userId}`);
      const serviceCaps = convertCapabilitiesForService(capabilities || {});
      const serviceRecord = await client.createMachineKey(
        userId,
        machineName || 'New Device',
        serviceCaps
      );
      const newMachine = convertMachineRecord(serviceRecord, state.currentMachineId || undefined);

      addMachine(newMachine);

      return newMachine;
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to create machine key';
      console.error('[useMachineKeys] createMachineKey error:', errorMsg);
      setError(errorMsg);
      throw err;
    }
  }, [getUserIdBigInt, state.currentMachineId, setLoading, addMachine, setError]);

  const revokeMachineKey = useCallback(async (machineId: string): Promise<void> => {
    const userId = getUserIdBigInt();
    if (!userId) {
      throw new Error('No user logged in');
    }

    const client = clientRef.current;
    if (!client) {
      throw new Error('Identity service client not available');
    }

    // Cannot revoke current machine
    if (machineId === state.currentMachineId) {
      throw new Error('Cannot revoke the current machine key');
    }

    setLoading(true);

    try {
      console.log(`[useMachineKeys] Revoking machine key ${machineId}`);
      // Parse machine ID from hex string to bigint
      const machineIdBigInt = BigInt(machineId);
      await client.revokeMachineKey(userId, machineIdBigInt);

      removeMachine(machineId);
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to revoke machine key';
      console.error('[useMachineKeys] revokeMachineKey error:', errorMsg);
      setError(errorMsg);
      throw err;
    }
  }, [getUserIdBigInt, state.currentMachineId, setLoading, removeMachine, setError]);

  const rotateMachineKey = useCallback(async (machineId: string): Promise<import('../../stores').MachineKeyRecord> => {
    const userId = getUserIdBigInt();
    if (!userId) {
      throw new Error('No user logged in');
    }

    const client = clientRef.current;
    if (!client) {
      throw new Error('Identity service client not available');
    }

    const existingMachine = state.machines.find(m => m.machineId === machineId);
    if (!existingMachine) {
      throw new Error('Machine not found');
    }

    setLoading(true);

    try {
      const oldEpoch = existingMachine.epoch;
      console.log(`[useMachineKeys] Rotating machine key ${machineId} (current epoch: ${oldEpoch})`);
      const machineIdBigInt = BigInt(machineId);
      const serviceRecord = await client.rotateMachineKey(userId, machineIdBigInt);
      const rotatedMachine = convertMachineRecord(serviceRecord, state.currentMachineId || undefined);

      console.log(`[useMachineKeys] Machine key rotated - epoch ${oldEpoch} -> ${rotatedMachine.epoch}`);
      updateMachine(machineId, rotatedMachine);

      return rotatedMachine;
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to rotate machine key';
      console.error('[useMachineKeys] rotateMachineKey error:', errorMsg);
      setError(errorMsg);
      throw err;
    }
  }, [getUserIdBigInt, state.machines, state.currentMachineId, setLoading, updateMachine, setError]);

  const refresh = useCallback(async (): Promise<void> => {
    const userId = getUserIdBigInt();
    if (!userId) {
      reset();
      setLoading(false);
      setInitializing(false);
      return;
    }

    // Reads directly from VfsStorage cache, no IPC client needed for listing
    try {
      await listMachineKeys();
    } catch {
      // Error already logged in listMachineKeys
      // isInitializing is set to false in setError
    }
  }, [getUserIdBigInt, listMachineKeys, reset, setLoading, setInitializing]);

  // Auto-refresh on mount and when user changes
  // Reads directly from VfsStorage cache, no IPC client needed
  useEffect(() => {
    if (currentUser?.id) {
      refresh();
    }
  }, [currentUser?.id, refresh]);

  return {
    state,
    listMachineKeys,
    getMachineKey,
    createMachineKey,
    revokeMachineKey,
    rotateMachineKey,
    refresh,
  };
}
