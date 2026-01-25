import { useState, useCallback, useEffect } from 'react';
import { useIdentityStore, selectCurrentUser } from '../../stores';
import { useIdentityServiceClient } from './useIdentityServiceClient';
import {
  type LinkedCredential as ServiceLinkedCredential,
  type CredentialType as ServiceCredentialType,
  VfsStorageClient,
  getCredentialsPath,
} from '../../services';

// =============================================================================
// Linked Accounts Types (mirrors zos-identity/src/keystore.rs)
// =============================================================================

/**
 * Types of linkable credentials.
 * Corresponds to `CredentialType` in zos-identity/src/keystore.rs
 */
export type CredentialType = 'email' | 'phone' | 'oauth' | 'webauthn';

/**
 * A linked external credential.
 * Corresponds to `LinkedCredential` in zos-identity/src/keystore.rs
 */
export interface LinkedCredential {
  /** Credential type */
  type: CredentialType;
  /** Credential value (email address, phone number, etc.) */
  identifier: string;
  /** Whether this credential is verified */
  verified: boolean;
  /** When the credential was linked */
  linkedAt: number;
  /** When verification was completed */
  verifiedAt: number | null;
  /** Is this the primary credential of its type? */
  isPrimary: boolean;
}

/**
 * Linked Accounts state
 */
export interface LinkedAccountsState {
  /** Linked credentials */
  credentials: LinkedCredential[];
  /** Loading state */
  isLoading: boolean;
  /** Error message */
  error: string | null;
}

/**
 * Hook return type
 */
export interface UseLinkedAccountsReturn {
  /** Current state */
  state: LinkedAccountsState;
  /**
   * Attach an email credential via ZID API.
   * Requires active ZID session (access token from loginWithMachineKey).
   * @param email - Email address to attach
   * @param password - Password for ZID account (12+ characters)
   * @param accessToken - JWT access token from ZID login
   * @param zidEndpoint - ZID API endpoint
   */
  attachEmail: (
    email: string,
    password: string,
    accessToken: string,
    zidEndpoint: string
  ) => Promise<void>;
  /** Unlink an account */
  unlinkAccount: (type: CredentialType) => Promise<void>;
  /** Refresh state */
  refresh: () => Promise<void>;
}

// =============================================================================
// Initial State
// =============================================================================

const INITIAL_STATE: LinkedAccountsState = {
  credentials: [],
  isLoading: false,
  error: null,
};

// =============================================================================
// Conversion helpers
// =============================================================================

/**
 * Convert service credential type to local type
 */
function convertCredentialType(type: ServiceCredentialType): CredentialType {
  switch (type) {
    case 'Email':
      return 'email';
    case 'Phone':
      return 'phone';
    case 'OAuth':
      return 'oauth';
    case 'WebAuthn':
      return 'webauthn';
    default:
      return 'email';
  }
}

/**
 * Convert local credential type to service type
 */
function convertCredentialTypeForService(type: CredentialType): ServiceCredentialType {
  switch (type) {
    case 'email':
      return 'Email';
    case 'phone':
      return 'Phone';
    case 'oauth':
      return 'OAuth';
    case 'webauthn':
      return 'WebAuthn';
    default:
      return 'Email';
  }
}

/**
 * Convert service credential to local format
 */
function convertCredential(cred: ServiceLinkedCredential): LinkedCredential {
  return {
    type: convertCredentialType(cred.credential_type),
    identifier: cred.value,
    verified: cred.verified,
    linkedAt: cred.linked_at,
    verifiedAt: cred.verified_at,
    isPrimary: cred.is_primary,
  };
}

/**
 * Credential store format from VFS
 */
interface CredentialStoreJson {
  user_id: number;
  credentials: ServiceLinkedCredential[];
}

// =============================================================================
// Hook Implementation
// =============================================================================

export function useLinkedAccounts(): UseLinkedAccountsReturn {
  const currentUser = useIdentityStore(selectCurrentUser);
  const { userId, getClientOrThrow, getUserIdOrThrow } = useIdentityServiceClient();
  const [state, setState] = useState<LinkedAccountsState>(INITIAL_STATE);

  /**
   * Refresh credentials directly from VFS cache (synchronous read)
   * Defined early since verifyEmail and unlinkAccount depend on it
   */
  const refreshFromVfs = useCallback(async (userIdParam: bigint): Promise<void> => {
    const credPath = getCredentialsPath(userIdParam);
    console.log(`[useLinkedAccounts] Reading credentials from VFS cache: ${credPath}`);

    if (!VfsStorageClient.isAvailable()) {
      console.warn('[useLinkedAccounts] VfsStorage not available yet');
      setState((prev) => ({ ...prev, credentials: [], isLoading: false }));
      return;
    }

    try {
      const store = VfsStorageClient.readJsonSync<CredentialStoreJson>(credPath);

      if (store && store.credentials) {
        const credentials = store.credentials.map(convertCredential);
        console.log(`[useLinkedAccounts] Found ${credentials.length} credentials in VFS cache`);
        setState((prev) => ({
          ...prev,
          credentials,
          isLoading: false,
          error: null,
        }));
      } else {
        console.log('[useLinkedAccounts] No credentials found in VFS cache');
        setState((prev) => ({ ...prev, credentials: [], isLoading: false }));
      }
    } catch (err) {
      console.warn('[useLinkedAccounts] Failed to read credentials from VFS:', err);
      setState((prev) => ({ ...prev, credentials: [], isLoading: false }));
    }
  }, []);

  const attachEmail = useCallback(
    async (
      email: string,
      password: string,
      accessToken: string,
      zidEndpoint: string
    ): Promise<void> => {
      const userIdVal = getUserIdOrThrow();
      const client = getClientOrThrow();

      setState((prev) => ({ ...prev, isLoading: true, error: null }));

      try {
        console.log(`[useLinkedAccounts] Attaching email ${email} for user ${userIdVal} via ZID`);
        await client.attachEmail(userIdVal, email, password, accessToken, zidEndpoint);

        // Refresh credentials from VFS cache after successful attachment
        await refreshFromVfs(userIdVal);

        setState((prev) => ({
          ...prev,
          isLoading: false,
          error: null,
        }));
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : 'Failed to attach email';
        console.error('[useLinkedAccounts] attachEmail error:', errorMsg);
        setState((prev) => ({
          ...prev,
          isLoading: false,
          error: errorMsg,
        }));
        throw err;
      }
    },
    [getClientOrThrow, getUserIdOrThrow, refreshFromVfs]
  );

  const unlinkAccount = useCallback(
    async (type: CredentialType): Promise<void> => {
      const userIdVal = getUserIdOrThrow();
      const client = getClientOrThrow();

      setState((prev) => ({ ...prev, isLoading: true, error: null }));

      try {
        console.log(`[useLinkedAccounts] Unlinking ${type} for user ${userIdVal}`);
        await client.unlinkCredential(userIdVal, convertCredentialTypeForService(type));

        // Refresh credentials from VFS cache
        await refreshFromVfs(userIdVal);
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : 'Failed to unlink account';
        console.error('[useLinkedAccounts] unlinkAccount error:', errorMsg);
        setState((prev) => ({
          ...prev,
          isLoading: false,
          error: errorMsg,
        }));
        throw err;
      }
    },
    [getClientOrThrow, getUserIdOrThrow, refreshFromVfs]
  );

  const refresh = useCallback(async (): Promise<void> => {
    if (!userId) {
      setState(INITIAL_STATE);
      return;
    }

    // Reads directly from VfsStorage cache
    await refreshFromVfs(userId);
  }, [userId, refreshFromVfs]);

  // Auto-refresh on mount and when user changes
  useEffect(() => {
    if (currentUser?.id) {
      refresh();
    }
  }, [currentUser?.id, refresh]);

  return {
    state,
    attachEmail,
    unlinkAccount,
    refresh,
  };
}
