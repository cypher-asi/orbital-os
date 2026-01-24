import { useState, useCallback, useEffect, useRef } from 'react';
import { useIdentityStore, selectCurrentUser } from '../../stores';
import { useSupervisor } from './useSupervisor';
import {
  IdentityServiceClient,
  type NeuralShard as ServiceNeuralShard,
  type NeuralKeyGenerated as ServiceNeuralKeyGenerated,
  type Supervisor,
  type LocalKeyStore as ServiceLocalKeyStore,
  VfsStorageClient,
  getIdentityKeyStorePath,
} from '../../services';

// =============================================================================
// Neural Key Types (public API - mirrors zos-identity/src/ipc.rs)
// =============================================================================

/**
 * A Shamir shard for Neural Key backup.
 * Corresponds to `NeuralShard` in zos-identity/src/ipc.rs
 */
export interface NeuralShard {
  /** Shard index (1-5) */
  index: number;
  /** Shard data as hex string */
  hex: string;
}

/**
 * Public identifiers derived from the Neural Key.
 * Corresponds to `PublicIdentifiers` in zos-identity/src/ipc.rs
 */
export interface PublicIdentifiers {
  /** Identity-level signing public key (Ed25519, hex string) */
  identitySigningPubKey: string;
  /** Machine-level signing public key (Ed25519, hex string) */
  machineSigningPubKey: string;
  /** Machine-level encryption public key (X25519, hex string) */
  machineEncryptionPubKey: string;
}

/**
 * Result of successful Neural Key generation.
 * Corresponds to `NeuralKeyGenerated` in zos-identity/src/ipc.rs
 */
export interface NeuralKeyGenerated {
  /** Public identifiers (stored server-side) */
  publicIdentifiers: PublicIdentifiers;
  /** Shamir shards (3-of-5) - returned to UI for backup, NOT stored */
  shards: NeuralShard[];
  /** Timestamp when the key was created */
  createdAt: number;
}

/**
 * Neural Key state
 */
export interface NeuralKeyState {
  /** Whether a Neural Key exists for the current user */
  hasNeuralKey: boolean;
  /** Public identifiers (if Neural Key exists) */
  publicIdentifiers: PublicIdentifiers | null;
  /** When the key was created */
  createdAt: number | null;
  /** Pending shards (shown during generation, cleared after confirmation) */
  pendingShards: NeuralShard[] | null;
  /** Loading state */
  isLoading: boolean;
  /** Whether we're in the initial settling period (component should show nothing) */
  isInitializing: boolean;
  /** Error message */
  error: string | null;
}

/**
 * Hook return type
 */
export interface UseNeuralKeyReturn {
  /** Current Neural Key state */
  state: NeuralKeyState;
  /** Generate a new Neural Key (returns shards for backup) */
  generateNeuralKey: () => Promise<NeuralKeyGenerated>;
  /** Recover Neural Key from shards */
  recoverNeuralKey: (shards: NeuralShard[]) => Promise<NeuralKeyGenerated>;
  /** Confirm shards have been saved - clears pending shards */
  confirmShardsSaved: () => void;
  /** Refresh state from identity service */
  refresh: () => Promise<void>;
}

// =============================================================================
// Response conversion helpers
// =============================================================================

function bytesToHex(bytes: number[]): string {
  return bytes.map(b => b.toString(16).padStart(2, '0')).join('');
}

/**
 * Convert service response format to public API format
 */
function convertServiceResponse(service: ServiceNeuralKeyGenerated): NeuralKeyGenerated {
  return {
    publicIdentifiers: {
      identitySigningPubKey: service.public_identifiers.identity_signing_pub_key,
      machineSigningPubKey: service.public_identifiers.machine_signing_pub_key,
      machineEncryptionPubKey: service.public_identifiers.machine_encryption_pub_key,
    },
    shards: service.shards.map(s => ({ index: s.index, hex: s.hex })),
    createdAt: service.created_at,
  };
}

/**
 * Convert public shard format to service format
 */
function convertShardsForService(shards: NeuralShard[]): ServiceNeuralShard[] {
  return shards.map(s => ({ index: s.index, hex: s.hex }));
}

// =============================================================================
// Initial State
// =============================================================================

const INITIAL_STATE: NeuralKeyState = {
  hasNeuralKey: false,
  publicIdentifiers: null,
  createdAt: null,
  pendingShards: null,
  isLoading: true,
  isInitializing: true, // Start with initializing true - component shows nothing during settle
  error: null,
};

// How long to wait before showing "no key" message (ms)
// This gives the VFS cache time to populate on initial load
const INITIAL_LOAD_SETTLE_DELAY = 500;

// =============================================================================
// Hook Implementation
// =============================================================================

export function useNeuralKey(): UseNeuralKeyReturn {
  const currentUser = useIdentityStore(selectCurrentUser);
  const supervisor = useSupervisor();
  const [state, setState] = useState<NeuralKeyState>(INITIAL_STATE);

  // Create a stable reference to the IdentityServiceClient
  const clientRef = useRef<IdentityServiceClient | null>(null);

  // Track if we've completed the initial load (to avoid premature "no key" flash)
  const hasCompletedInitialLoadRef = useRef(false);

  // Initialize client when supervisor becomes available
  useEffect(() => {
    if (supervisor && !clientRef.current) {
      // Cast supervisor to the Supervisor interface
      clientRef.current = new IdentityServiceClient(supervisor as unknown as Supervisor);
      console.log('[useNeuralKey] IdentityServiceClient initialized');
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

  const generateNeuralKey = useCallback(async (): Promise<NeuralKeyGenerated> => {
    const userId = getUserIdBigInt();
    if (!userId) {
      throw new Error('No user logged in');
    }

    const client = clientRef.current;
    if (!client) {
      throw new Error('Identity service client not available');
    }

    setState(prev => ({ ...prev, isLoading: true, error: null }));

    try {
      console.log(`[useNeuralKey] Generating Neural Key for user ${userId}`);
      const serviceResult = await client.generateNeuralKey(userId);
      const result = convertServiceResponse(serviceResult);

      setState(prev => ({
        ...prev,
        hasNeuralKey: true,
        publicIdentifiers: result.publicIdentifiers,
        createdAt: result.createdAt,
        pendingShards: result.shards,
        isLoading: false,
      }));

      return result;
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to generate Neural Key';
      console.error('[useNeuralKey] generateNeuralKey error:', errorMsg);
      setState(prev => ({
        ...prev,
        isLoading: false,
        error: errorMsg,
      }));
      throw err;
    }
  }, [getUserIdBigInt]);

  const recoverNeuralKey = useCallback(async (shards: NeuralShard[]): Promise<NeuralKeyGenerated> => {
    const userId = getUserIdBigInt();
    if (!userId) {
      throw new Error('No user logged in');
    }

    const client = clientRef.current;
    if (!client) {
      throw new Error('Identity service client not available');
    }

    if (shards.length < 3) {
      throw new Error('At least 3 shards are required for recovery');
    }

    setState(prev => ({ ...prev, isLoading: true, error: null }));

    try {
      console.log(`[useNeuralKey] Recovering Neural Key for user ${userId}`);
      const serviceShards = convertShardsForService(shards);
      const serviceResult = await client.recoverNeuralKey(userId, serviceShards);
      const result = convertServiceResponse(serviceResult);

      setState(prev => ({
        ...prev,
        hasNeuralKey: true,
        publicIdentifiers: result.publicIdentifiers,
        createdAt: result.createdAt,
        pendingShards: result.shards,
        isLoading: false,
      }));

      return result;
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to recover Neural Key';
      console.error('[useNeuralKey] recoverNeuralKey error:', errorMsg);
      setState(prev => ({
        ...prev,
        isLoading: false,
        error: errorMsg,
      }));
      throw err;
    }
  }, [getUserIdBigInt]);

  const confirmShardsSaved = useCallback(() => {
    setState(prev => ({
      ...prev,
      pendingShards: null,
    }));
  }, []);

  const refresh = useCallback(async (): Promise<void> => {
    const userId = getUserIdBigInt();
    if (!userId) {
      hasCompletedInitialLoadRef.current = true;
      setState({ ...INITIAL_STATE, isLoading: false, isInitializing: false });
      return;
    }

    // Read directly from VfsStorage cache (synchronous, no IPC deadlock)
    // This follows the canonical pattern: React reads from VFS cache, services write via async syscalls
    const keyPath = getIdentityKeyStorePath(userId);

    console.log(`[useNeuralKey] Refreshing Neural Key state from VFS cache: ${keyPath}`);

    // Check VfsStorage availability
    if (!VfsStorageClient.isAvailable()) {
      console.warn('[useNeuralKey] VfsStorage not available yet');
      setState(prev => ({
        ...prev,
        isLoading: false,
        isInitializing: false,
        error: 'VFS cache not ready',
      }));
      return;
    }

    setState(prev => ({ ...prev, isLoading: true, error: null }));

    // Helper to read key store and update state
    const readAndUpdateState = (): boolean => {
      try {
        // Read key store directly from VFS cache (synchronous)
        const keyStore = VfsStorageClient.readJsonSync<ServiceLocalKeyStore>(keyPath);

        // Log received data for debugging
        console.log('[useNeuralKey] Read keyStore from VFS cache:', {
          hasKey: !!keyStore,
          userId: keyStore?.user_id,
          hasCreatedAt: keyStore?.created_at !== undefined,
          createdAt: keyStore?.created_at,
          epoch: keyStore?.epoch,
          cacheStats: VfsStorageClient.getCacheStats(),
        });

        if (keyStore) {
          // Validate response structure - warn if expected fields are missing
          if (keyStore.created_at === undefined) {
            console.warn('[useNeuralKey] LocalKeyStore missing created_at - may be old format');
          }

          setState(prev => ({
            ...prev,
            hasNeuralKey: true,
            publicIdentifiers: {
              identitySigningPubKey: '0x' + bytesToHex(keyStore.identity_signing_public_key),
              machineSigningPubKey: '0x' + bytesToHex(keyStore.machine_signing_public_key),
              machineEncryptionPubKey: '0x' + bytesToHex(keyStore.machine_encryption_public_key),
            },
            // Set createdAt from keyStore.created_at, or null for backward compatibility
            createdAt: keyStore.created_at ?? null,
            isLoading: false,
            isInitializing: false,
          }));
          return true; // Key found
        }
        return false; // No key found
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : 'Failed to refresh Neural Key state';
        console.error('[useNeuralKey] refresh error:', errorMsg);
        setState(prev => ({
          ...prev,
          isLoading: false,
          isInitializing: false,
          error: errorMsg,
        }));
        return true; // Return true to stop retry (error case)
      }
    };

    // First attempt to read key
    const foundKey = readAndUpdateState();

    if (!foundKey && !hasCompletedInitialLoadRef.current) {
      // On initial load, wait and retry before showing "no key" message
      // This gives the VFS cache time to populate
      console.log('[useNeuralKey] No key found on initial load, waiting before retry...');
      await new Promise(resolve => setTimeout(resolve, INITIAL_LOAD_SETTLE_DELAY));

      // Retry reading
      const foundKeyOnRetry = readAndUpdateState();

      if (!foundKeyOnRetry) {
        // Still no key after waiting - now we can show "no key" message
        console.log('[useNeuralKey] No key store found at', keyPath, '(after settle delay)');
        setState(prev => ({
          ...prev,
          hasNeuralKey: false,
          publicIdentifiers: null,
          createdAt: null,
          isLoading: false,
          isInitializing: false,
        }));
      }
      // Note: if key was found on retry, readAndUpdateState already set isInitializing: false

      hasCompletedInitialLoadRef.current = true;
    } else if (!foundKey) {
      // Not initial load, immediately show "no key" message
      console.log('[useNeuralKey] No key store found at', keyPath);
      setState(prev => ({
        ...prev,
        hasNeuralKey: false,
        publicIdentifiers: null,
        createdAt: null,
        isLoading: false,
      }));
    }
    // Note: if key was found on first attempt, readAndUpdateState already set isInitializing: false

    // Mark initial load as complete
    hasCompletedInitialLoadRef.current = true;
  }, [getUserIdBigInt]);

  // Auto-refresh on mount and when user changes
  // Reads directly from VfsStorage cache, no IPC client needed
  useEffect(() => {
    if (currentUser?.id) {
      refresh();
    }
  }, [currentUser?.id, refresh]);

  return {
    state,
    generateNeuralKey,
    recoverNeuralKey,
    confirmShardsSaved,
    refresh,
  };
}
