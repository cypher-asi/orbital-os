/**
 * Machine Keys Store - Centralized state for machine key management.
 * 
 * Manages machine keys list, current machine ID, loading state.
 * Shared across all components that need machine keys state.
 */

import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';

// =============================================================================
// Machine Key Types (mirrors zos-identity/src/keystore.rs and ipc.rs)
// =============================================================================

/**
 * Capabilities of machine-level keys.
 * Corresponds to `MachineKeyCapabilities` in zos-identity/src/keystore.rs
 */
export interface MachineKeyCapabilities {
  /** Can sign authentication challenges */
  canAuthenticate: boolean;
  /** Can encrypt/decrypt local data */
  canEncrypt: boolean;
  /** Can sign messages on behalf of user */
  canSignMessages: boolean;
  /** Can authorize new machines */
  canAuthorizeMachines: boolean;
  /** Can revoke other machines */
  canRevokeMachines: boolean;
  /** Expiry time (null = no expiry) */
  expiresAt: number | null;
}

/**
 * Per-machine key record.
 * Corresponds to `MachineKeyRecord` in zos-identity/src/keystore.rs
 */
export interface MachineKeyRecord {
  /** Machine ID (128-bit as hex string) */
  machineId: string;
  /** Machine-specific signing public key (Ed25519, hex) */
  signingPublicKey: string;
  /** Machine-specific encryption public key (X25519, hex) */
  encryptionPublicKey: string;
  /** When this machine was authorized */
  authorizedAt: number;
  /** Who authorized this machine (user_id or machine_id as hex) */
  authorizedBy: string;
  /** Machine capabilities */
  capabilities: MachineKeyCapabilities;
  /** Human-readable machine name */
  machineName: string | null;
  /** Last seen timestamp */
  lastSeenAt: number;
  /** Whether this is the current device */
  isCurrentDevice: boolean;
  /** Key epoch (increments on rotation) */
  epoch: number;
}

/**
 * Machine Keys state
 */
export interface MachineKeysState {
  /** List of machine key records */
  machines: MachineKeyRecord[];
  /** Current machine ID (if applicable) */
  currentMachineId: string | null;
  /** Loading state */
  isLoading: boolean;
  /** Whether we're in the initial settling period (component should show nothing) */
  isInitializing: boolean;
  /** Error message */
  error: string | null;
}

// =============================================================================
// Store Types
// =============================================================================

interface MachineKeysStoreState extends MachineKeysState {
  // Actions
  setMachines: (machines: MachineKeyRecord[]) => void;
  addMachine: (machine: MachineKeyRecord) => void;
  removeMachine: (machineId: string) => void;
  updateMachine: (machineId: string, updates: Partial<MachineKeyRecord>) => void;
  setCurrentMachineId: (id: string | null) => void;
  setLoading: (loading: boolean) => void;
  setInitializing: (initializing: boolean) => void;
  setError: (error: string | null) => void;
  reset: () => void;
}

// =============================================================================
// Initial State
// =============================================================================

const INITIAL_STATE: MachineKeysState = {
  machines: [],
  currentMachineId: null,
  isLoading: true, // Start with loading true to avoid flash of empty state
  isInitializing: true, // Start with initializing true - component shows nothing during settle
  error: null,
};

// =============================================================================
// Store Creation
// =============================================================================

export const useMachineKeysStore = create<MachineKeysStoreState>()(
  subscribeWithSelector((set) => ({
    ...INITIAL_STATE,

    setMachines: (machines) => set({ machines, isLoading: false, isInitializing: false }),
    
    addMachine: (machine) => set((state) => ({
      machines: [...state.machines, machine],
      isLoading: false,
    })),
    
    removeMachine: (machineId) => set((state) => ({
      machines: state.machines.filter((m) => m.machineId !== machineId),
      isLoading: false,
    })),
    
    updateMachine: (machineId, updates) => set((state) => ({
      machines: state.machines.map((m) =>
        m.machineId === machineId ? { ...m, ...updates } : m
      ),
      isLoading: false,
    })),
    
    setCurrentMachineId: (currentMachineId) => set({ currentMachineId }),
    
    setLoading: (isLoading) => set({ isLoading }),
    
    setInitializing: (isInitializing) => set({ isInitializing }),
    
    setError: (error) => set({ error, isLoading: false, isInitializing: false }),
    
    reset: () => set(INITIAL_STATE),
  }))
);

// =============================================================================
// Selectors for Fine-Grained Subscriptions
// =============================================================================

/** Select all machines */
export const selectMachines = (state: MachineKeysStoreState) => state.machines;

/** Select machine count */
export const selectMachineCount = (state: MachineKeysStoreState) => state.machines.length;

/** Select current machine ID */
export const selectCurrentMachineId = (state: MachineKeysStoreState) => state.currentMachineId;

/** Select loading state */
export const selectMachineKeysIsLoading = (state: MachineKeysStoreState) => state.isLoading;

/** Select initializing state */
export const selectMachineKeysIsInitializing = (state: MachineKeysStoreState) => state.isInitializing;

/** Select error state */
export const selectMachineKeysError = (state: MachineKeysStoreState) => state.error;

/** Select machine by ID */
export const selectMachineById = (id: string) => (state: MachineKeysStoreState) =>
  state.machines.find((m) => m.machineId === id);

/** Select current device machine */
export const selectCurrentDevice = (state: MachineKeysStoreState) =>
  state.machines.find((m) => m.isCurrentDevice);

/** Composite selector for full state (useful for hooks that need everything) */
export const selectMachineKeysState = (state: MachineKeysStoreState): MachineKeysState => ({
  machines: state.machines,
  currentMachineId: state.currentMachineId,
  isLoading: state.isLoading,
  isInitializing: state.isInitializing,
  error: state.error,
});
