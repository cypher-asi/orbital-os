import { useState, useCallback, useEffect, useRef } from 'react';
import { useIdentityStore, selectCurrentUser } from '@/stores';
import { useIdentityServiceClient } from './useIdentityServiceClient';
import {
  type LocalKeyStore as ServiceLocalKeyStore,
  KeystoreClient,
  getIdentityKeystorePath,
  bytesToHex,
} from '@/client-services';

// Import types from shared module
import type {
  NeuralShard,
  NeuralKeyGenerated,
  NeuralKeyState,
} from '@/shared/types';

// Import converters from shared module
import {
  convertNeuralKeyGenerated,
  convertShardsForService,
} from '@/shared/converters/identity';

// Re-export types for backward compatibility
export type {
  NeuralShard,
  PublicIdentifiers,
  NeuralKeyGenerated,
  NeuralKeyState,
} from '@/shared/types';

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
  const { userId, getClientOrThrow, getUserIdOrThrow } = useIdentityServiceClient();
  const [state, setState] = useState<NeuralKeyState>(INITIAL_STATE);

  // Track if we've completed the initial load (to avoid premature "no key" flash)
  const hasCompletedInitialLoadRef = useRef(false);

  const generateNeuralKey = useCallback(async (): Promise<NeuralKeyGenerated> => {
    const userIdVal = getUserIdOrThrow();
    const client = getClientOrThrow();

    setState((prev) => ({ ...prev, isLoading: true, error: null }));

    try {
      console.log(`[useNeuralKey] Generating Neural Key for user ${userIdVal}`);
      const serviceResult = await client.generateNeuralKey(userIdVal);
      const result = convertNeuralKeyGenerated(serviceResult);

      setState((prev) => ({
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
      setState((prev) => ({
        ...prev,
        isLoading: false,
        error: errorMsg,
      }));
      throw err;
    }
  }, [getClientOrThrow, getUserIdOrThrow]);

  const recoverNeuralKey = useCallback(
    async (shards: NeuralShard[]): Promise<NeuralKeyGenerated> => {
      const userIdVal = getUserIdOrThrow();
      const client = getClientOrThrow();

      if (shards.length < 3) {
        throw new Error('At least 3 shards are required for recovery');
      }

      setState((prev) => ({ ...prev, isLoading: true, error: null }));

      try {
        console.log(`[useNeuralKey] Recovering Neural Key for user ${userIdVal}`);
        const serviceShards = convertShardsForService(shards);
        const serviceResult = await client.recoverNeuralKey(userIdVal, serviceShards);
        const result = convertNeuralKeyGenerated(serviceResult);

        setState((prev) => ({
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
        setState((prev) => ({
          ...prev,
          isLoading: false,
          error: errorMsg,
        }));
        throw err;
      }
    },
    [getClientOrThrow, getUserIdOrThrow]
  );

  const confirmShardsSaved = useCallback(() => {
    setState((prev) => ({
      ...prev,
      pendingShards: null,
    }));
  }, []);

  const refresh = useCallback(async (): Promise<void> => {
    if (!userId) {
      hasCompletedInitialLoadRef.current = true;
      setState({ ...INITIAL_STATE, isLoading: false, isInitializing: false });
      return;
    }

    // Read directly from keystore cache (synchronous, no IPC deadlock)
    // This follows the canonical pattern: React reads from keystore cache, services write via async syscalls
    const keyPath = getIdentityKeystorePath(userId);

    console.log(`[useNeuralKey] Refreshing Neural Key state from keystore cache: ${keyPath}`);

    // Check keystore availability
    if (!KeystoreClient.isAvailable()) {
      console.warn('[useNeuralKey] Keystore not available yet');
      setState((prev) => ({
        ...prev,
        isLoading: false,
        isInitializing: false,
        error: 'Keystore cache not ready',
      }));
      return;
    }

    setState((prev) => ({ ...prev, isLoading: true, error: null }));

    // Helper to read key store and update state
    const readAndUpdateState = (): boolean => {
      try {
        // Read key store directly from keystore cache (synchronous)
        const keyStore = KeystoreClient.readJsonSync<ServiceLocalKeyStore>(keyPath);

        // Log received data for debugging
        console.log('[useNeuralKey] Read keyStore from keystore cache:', {
          hasKey: !!keyStore,
          userId: keyStore?.user_id,
          hasCreatedAt: keyStore?.created_at !== undefined,
          createdAt: keyStore?.created_at,
          epoch: keyStore?.epoch,
          cacheStats: KeystoreClient.getCacheStats(),
        });

        if (keyStore) {
          // Validate response structure - warn if expected fields are missing
          if (keyStore.created_at === undefined) {
            console.warn('[useNeuralKey] LocalKeyStore missing created_at - may be old format');
          }

          setState((prev) => ({
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
        setState((prev) => ({
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
      // This gives the keystore cache time to populate
      console.log('[useNeuralKey] No key found on initial load, waiting before retry...');
      await new Promise((resolve) => setTimeout(resolve, INITIAL_LOAD_SETTLE_DELAY));

      // Retry reading
      const foundKeyOnRetry = readAndUpdateState();

      if (!foundKeyOnRetry) {
        // Still no key after waiting - now we can show "no key" message
        console.log('[useNeuralKey] No key store found at', keyPath, '(after settle delay)');
        setState((prev) => ({
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
      setState((prev) => ({
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
  }, [userId]);

  // Auto-refresh on mount and when user changes
  // Reads directly from keystore cache, no IPC client needed
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
