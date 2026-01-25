import { useCallback, useEffect } from 'react';
import { useShallow } from 'zustand/react/shallow';
import {
  useIdentityStore,
  selectCurrentUser,
  useMachineKeysStore,
  selectMachineKeysState,
} from '../../stores';
import type { KeyScheme, MachineKeyCapability } from '../../stores';
import { useIdentityServiceClient } from './useIdentityServiceClient';
import {
  type MachineKeyCapabilities as ServiceMachineKeyCapabilities,
  type MachineKeyRecord as ServiceMachineKeyRecord,
  type KeyScheme as ServiceKeyScheme,
  type NeuralShard,
  isLegacyCapabilities,
  convertLegacyCapabilities,
  VfsStorageClient,
  getMachineKeysDir,
} from '../../services';

// Re-export types from store for backward compatibility
export type {
  MachineKeyCapabilities,
  MachineKeyRecord,
  MachineKeysState,
  KeyScheme,
  MachineKeyCapability,
} from '../../stores';

/** Neural shard for key derivation */
export type { NeuralShard } from '../../services';

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
  /** Create a new machine key (requires 3 Neural shards for key derivation) */
  createMachineKey: (
    machineName?: string,
    capabilities?: MachineKeyCapability[],
    keyScheme?: KeyScheme,
    shards?: NeuralShard[]
  ) => Promise<import('../../stores').MachineKeyRecord>;
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
  return bytes.map((b) => b.toString(16).padStart(2, '0')).join('');
}

/**
 * Convert service capabilities to public API format.
 * Handles both modern (string array) and legacy (boolean struct) formats.
 */
function convertCapabilities(
  caps: ServiceMachineKeyCapabilities
): import('../../stores').MachineKeyCapabilities {
  // Handle legacy format (boolean struct)
  if (isLegacyCapabilities(caps)) {
    const converted = convertLegacyCapabilities(caps);
    return {
      capabilities: converted.capabilities as MachineKeyCapability[],
      expiresAt: converted.expires_at,
    };
  }

  // Modern format (string array)
  return {
    capabilities: caps.capabilities as MachineKeyCapability[],
    expiresAt: caps.expires_at,
  };
}

/**
 * Convert public API capabilities (string array) to service format.
 */
function convertCapabilitiesForService(
  caps?: MachineKeyCapability[]
): ServiceMachineKeyCapabilities {
  // Default capabilities if none provided
  const defaultCaps: MachineKeyCapability[] = ['AUTHENTICATE', 'ENCRYPT'];
  const capabilities = caps && caps.length > 0 ? caps : defaultCaps;

  return {
    capabilities,
    expires_at: null,
  };
}

/**
 * Convert service machine record to public API format
 */
function convertMachineRecord(
  record: ServiceMachineKeyRecord,
  currentMachineId?: string
): import('../../stores').MachineKeyRecord {
  // machine_id comes as a number from JSON, convert to hex string
  const machineIdHex =
    typeof record.machine_id === 'number'
      ? '0x' + record.machine_id.toString(16).padStart(32, '0')
      : record.machine_id.toString();

  const authorizedByHex =
    typeof record.authorized_by === 'number'
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
    keyScheme: record.key_scheme ?? 'classical', // Default to classical for backward compatibility
    pqSigningPublicKey: record.pq_signing_public_key
      ? bytesToHex(record.pq_signing_public_key)
      : undefined,
    pqEncryptionPublicKey: record.pq_encryption_public_key
      ? bytesToHex(record.pq_encryption_public_key)
      : undefined,
  };
}

// =============================================================================
// Hook Implementation
// =============================================================================

export function useMachineKeys(): UseMachineKeysReturn {
  const currentUser = useIdentityStore(selectCurrentUser);

  // Use shared IdentityServiceClient hook
  const { userId, getClientOrThrow, getUserIdOrThrow } = useIdentityServiceClient();

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

  const listMachineKeys = useCallback(async (): Promise<
    import('../../stores').MachineKeyRecord[]
  > => {
    const userIdVal = getUserIdOrThrow();

    // Read directly from VfsStorage cache (synchronous, no IPC deadlock)
    const machineDir = getMachineKeysDir(userIdVal);

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
      const corruptFiles: string[] = [];

      // Read each machine key file
      for (const child of children) {
        if (!child.name.endsWith('.json')) continue;

        const content = VfsStorageClient.readJsonSync<ServiceMachineKeyRecord>(child.path);
        if (content) {
          try {
            machines.push(convertMachineRecord(content, state.currentMachineId || undefined));
          } catch (convErr) {
            console.warn(
              `[useMachineKeys] Failed to convert machine key at ${child.path}:`,
              convErr
            );
            corruptFiles.push(child.path);
          }
        } else {
          // JSON parsing failed - file might be corrupt/truncated
          console.warn(`[useMachineKeys] Skipping corrupt/invalid machine key file: ${child.path}`);
          corruptFiles.push(child.path);
        }
      }

      if (corruptFiles.length > 0) {
        console.warn(
          `[useMachineKeys] Found ${corruptFiles.length} corrupt machine key file(s). ` +
            `These may need to be deleted and recreated:`,
          corruptFiles
        );
      }

      // Deduplicate machines by machineId (in case of duplicate entries)
      const uniqueMachines = machines.filter((machine, index, self) => {
        const isDuplicate = self.findIndex(m => m.machineId === machine.machineId) !== index;
        if (isDuplicate) {
          console.warn(`[useMachineKeys] Skipping duplicate machine ID: ${machine.machineId}`);
        }
        return !isDuplicate;
      });

      // Warn about zero-ID machines (indicates entropy generation failure)
      const zeroIdMachines = uniqueMachines.filter(m => 
        m.machineId === '0x00000000000000000000000000000000' || 
        m.machineId === '0x0' ||
        m.machineId === '0'
      );
      if (zeroIdMachines.length > 0) {
        console.error(
          `[useMachineKeys] WARNING: Found ${zeroIdMachines.length} machine(s) with zero ID! ` +
          `This indicates entropy generation failed. These machines may need to be recreated.`
        );
      }

      console.log(
        `[useMachineKeys] Found ${uniqueMachines.length} unique machine keys in VFS cache` +
          (corruptFiles.length > 0 ? ` (${corruptFiles.length} corrupt/skipped)` : '') +
          (machines.length !== uniqueMachines.length ? ` (${machines.length - uniqueMachines.length} duplicates removed)` : '')
      );

      setMachines(uniqueMachines);

      return machines;
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to list machine keys';
      console.error('[useMachineKeys] listMachineKeys error:', errorMsg);
      setError(errorMsg);
      throw err;
    }
  }, [getUserIdOrThrow, state.currentMachineId, setLoading, setMachines, setError]);

  const getMachineKey = useCallback(
    async (machineId: string): Promise<import('../../stores').MachineKeyRecord | null> => {
      // For now, look up in current state
      // Could add a specific get endpoint later
      return state.machines.find((m) => m.machineId === machineId) || null;
    },
    [state.machines]
  );

  const createMachineKey = useCallback(
    async (
      machineName?: string,
      capabilities?: MachineKeyCapability[],
      keyScheme?: KeyScheme,
      shards?: NeuralShard[]
    ): Promise<import('../../stores').MachineKeyRecord> => {
      const userIdVal = getUserIdOrThrow();
      const client = getClientOrThrow();

      // Validate shards
      if (!shards || shards.length < 3) {
        throw new Error('At least 3 Neural shards are required to create a machine key');
      }

      setLoading(true);

      try {
        const schemeToUse = keyScheme ?? 'classical';
        console.log(
          `[useMachineKeys] Creating machine key for user ${userIdVal} (scheme: ${schemeToUse}, shards: ${shards.length})`
        );
        const serviceCaps = convertCapabilitiesForService(capabilities);
        const serviceRecord = await client.createMachineKey(
          userIdVal,
          machineName || 'New Device',
          serviceCaps,
          schemeToUse as ServiceKeyScheme,
          shards
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
    },
    [getClientOrThrow, getUserIdOrThrow, state.currentMachineId, setLoading, addMachine, setError]
  );

  const revokeMachineKey = useCallback(
    async (machineId: string): Promise<void> => {
      const userIdVal = getUserIdOrThrow();
      const client = getClientOrThrow();

      // Cannot revoke current machine
      if (machineId === state.currentMachineId) {
        throw new Error('Cannot revoke the current machine key');
      }

      setLoading(true);

      try {
        console.log(`[useMachineKeys] Revoking machine key ${machineId}`);
        // Parse machine ID from hex string to bigint
        const machineIdBigInt = BigInt(machineId);
        await client.revokeMachineKey(userIdVal, machineIdBigInt);

        removeMachine(machineId);
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : 'Failed to revoke machine key';
        console.error('[useMachineKeys] revokeMachineKey error:', errorMsg);
        setError(errorMsg);
        throw err;
      }
    },
    [
      getClientOrThrow,
      getUserIdOrThrow,
      state.currentMachineId,
      setLoading,
      removeMachine,
      setError,
    ]
  );

  const rotateMachineKey = useCallback(
    async (machineId: string): Promise<import('../../stores').MachineKeyRecord> => {
      const userIdVal = getUserIdOrThrow();
      const client = getClientOrThrow();

      const existingMachine = state.machines.find((m) => m.machineId === machineId);
      if (!existingMachine) {
        throw new Error('Machine not found');
      }

      setLoading(true);

      try {
        const oldEpoch = existingMachine.epoch;
        console.log(
          `[useMachineKeys] Rotating machine key ${machineId} (current epoch: ${oldEpoch})`
        );
        const machineIdBigInt = BigInt(machineId);
        const serviceRecord = await client.rotateMachineKey(userIdVal, machineIdBigInt);
        const rotatedMachine = convertMachineRecord(
          serviceRecord,
          state.currentMachineId || undefined
        );

        console.log(
          `[useMachineKeys] Machine key rotated - epoch ${oldEpoch} -> ${rotatedMachine.epoch}`
        );
        updateMachine(machineId, rotatedMachine);

        return rotatedMachine;
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : 'Failed to rotate machine key';
        console.error('[useMachineKeys] rotateMachineKey error:', errorMsg);
        setError(errorMsg);
        throw err;
      }
    },
    [
      getClientOrThrow,
      getUserIdOrThrow,
      state.machines,
      state.currentMachineId,
      setLoading,
      updateMachine,
      setError,
    ]
  );

  const refresh = useCallback(async (): Promise<void> => {
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
  }, [userId, listMachineKeys, reset, setLoading, setInitializing]);

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
