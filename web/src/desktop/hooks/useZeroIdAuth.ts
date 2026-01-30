import { useState, useCallback, useEffect, useRef } from 'react';
import {
  IdentityServiceClient,
  type ZidTokens,
  type ZidSession,
  VfsStorageClient,
  formatUserId,
} from '@/client-services';
import { useSupervisor } from './useSupervisor';
import {
  useIdentityStore,
  selectCurrentUser,
  selectRemoteAuthState,
  type RemoteAuthState,
} from '@/stores';

// Re-export RemoteAuthState for backward compatibility
export type { RemoteAuthState };

/** Hook return type */
export interface UseZeroIdAuthReturn {
  /** Current remote auth state (null if not logged in) */
  remoteAuthState: RemoteAuthState | null;
  /** Whether authentication is in progress */
  isAuthenticating: boolean;
  /** Whether we're loading session from VFS */
  isLoadingSession: boolean;
  /** Error message if any */
  error: string | null;
  /** Login with email and password */
  loginWithEmail: (email: string, password: string, zidEndpoint?: string) => Promise<void>;
  /** Login with machine key challenge-response */
  loginWithMachineKey: (zidEndpoint?: string) => Promise<void>;
  /** Enroll/register machine with ZID server */
  enrollMachine: (zidEndpoint?: string) => Promise<void>;
  /** Disconnect from ZERO ID (clears remote session, not local identity) */
  disconnect: () => Promise<void>;
  /** Refresh the access token */
  refreshToken: () => Promise<void>;
  /** Get time remaining until token expires */
  getTimeRemaining: () => string;
  /** Check if token is expired */
  isTokenExpired: () => boolean;
}

// =============================================================================
// Constants
// =============================================================================

const DEFAULT_ZID_ENDPOINT = 'http://127.0.0.1:9999';

// =============================================================================
// Global Refresh Lock (prevents concurrent refresh requests across all hook instances)
// =============================================================================

let globalRefreshInProgress = false;
let globalRefreshPromise: Promise<void> | null = null;

// =============================================================================
// Helpers
// =============================================================================

/**
 * Get the canonical path for a user's ZID session.
 */
function getZidSessionPath(userId: bigint | string | number): string {
  return `/home/${formatUserId(userId)}/.zos/identity/zid_session.json`;
}

function formatTimeRemaining(expiresAt: number): string {
  const remaining = expiresAt - Date.now();
  if (remaining <= 0) {
    return 'Expired';
  }

  const hours = Math.floor(remaining / (60 * 60 * 1000));
  const minutes = Math.floor((remaining % (60 * 60 * 1000)) / (60 * 1000));

  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  return `${minutes}m`;
}

// =============================================================================
// Hook Implementation
// =============================================================================

export function useZeroIdAuth(): UseZeroIdAuthReturn {
  // Use shared store for remoteAuthState so all consumers get updates
  const remoteAuthState = useIdentityStore(selectRemoteAuthState);
  const setRemoteAuthState = useIdentityStore((state) => state.setRemoteAuthState);

  // Local state for loading/error (these are per-component)
  const [isAuthenticating, setIsAuthenticating] = useState(false);
  const [isLoadingSession, setIsLoadingSession] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const supervisor = useSupervisor();
  const currentUser = useIdentityStore(selectCurrentUser);
  const currentUserId = currentUser?.id ?? null;

  // Track if we've initialized to avoid double-loading
  const initializedRef = useRef(false);

  // Track refresh attempts to implement backoff on failure
  const lastRefreshAttemptRef = useRef<number>(0);
  const refreshFailCountRef = useRef<number>(0);

  // Create client instance when supervisor is available
  const clientRef = useRef<IdentityServiceClient | null>(null);
  if (supervisor && !clientRef.current) {
    clientRef.current = new IdentityServiceClient(supervisor);
  }

  // Load session from VFS cache on mount (or when user changes)
  useEffect(() => {
    if (!currentUserId || initializedRef.current) {
      setIsLoadingSession(false);
      return;
    }

    const loadSession = () => {
      try {
        const sessionPath = getZidSessionPath(currentUserId);
        const session = VfsStorageClient.readJsonSync<ZidSession>(sessionPath);

        if (session && session.expires_at > Date.now()) {
          // Valid session found, restore state
          // Note: machine_id and login_type may not be in older cached sessions (backward compat)
          setRemoteAuthState({
            serverEndpoint: session.zid_endpoint,
            accessToken: session.access_token,
            tokenExpiresAt: session.expires_at,
            refreshToken: session.refresh_token,
            scopes: ['read', 'write', 'sync'], // Default scopes
            sessionId: session.session_id,
            machineId: session.machine_id ?? '',
            loginType: session.login_type as RemoteAuthState['loginType'],
          });
          console.log('[useZeroIdAuth] Restored session from VFS cache');
        } else if (session) {
          console.log('[useZeroIdAuth] Found expired session in VFS cache');
        }
      } catch (err) {
        console.warn('[useZeroIdAuth] Failed to load session from VFS:', err);
      } finally {
        setIsLoadingSession(false);
        initializedRef.current = true;
      }
    };

    loadSession();
  }, [currentUserId]);

  const loginWithEmail = useCallback(
    async (email: string, password: string, zidEndpoint: string = DEFAULT_ZID_ENDPOINT) => {
      setIsAuthenticating(true);
      setError(null);

      try {
        // Validate input
        if (!email || !password) {
          throw new Error('Email and password are required');
        }

        const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
        if (!emailRegex.test(email)) {
          throw new Error('Invalid email format');
        }

        if (!clientRef.current) {
          throw new Error('Supervisor not available - please wait for system to initialize');
        }
        if (!currentUserId) {
          throw new Error('You must be logged in locally before using email login');
        }

        // Call identity service to perform email login
        const tokens: ZidTokens = await clientRef.current.loginWithEmail(
          currentUserId,
          email,
          password,
          zidEndpoint
        );

        // Convert tokens to RemoteAuthState
        // login_type comes from the service (or falls back to 'email' for this flow)
        const authState: RemoteAuthState = {
          serverEndpoint: zidEndpoint,
          accessToken: tokens.access_token,
          tokenExpiresAt: new Date(tokens.expires_at).getTime(),
          refreshToken: tokens.refresh_token,
          scopes: ['read', 'write', 'sync'],
          sessionId: tokens.session_id,
          machineId: tokens.machine_id,
          loginType: (tokens.login_type as RemoteAuthState['loginType']) ?? 'email',
        };

        setRemoteAuthState(authState);
        console.log('[useZeroIdAuth] Email login successful');
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : 'Email authentication failed';
        setError(errorMsg);
        throw err;
      } finally {
        setIsAuthenticating(false);
      }
    },
    [currentUserId, setRemoteAuthState]
  );

  const loginWithMachineKey = useCallback(
    async (zidEndpoint: string = DEFAULT_ZID_ENDPOINT) => {
      setIsAuthenticating(true);
      setError(null);

      try {
        if (!clientRef.current) {
          throw new Error('Supervisor not available - please wait for system to initialize');
        }
        if (!currentUserId) {
          throw new Error('You must be logged in locally before using Machine Key login');
        }

        // Call identity service to perform machine key login
        const tokens: ZidTokens = await clientRef.current.loginWithMachineKey(
          currentUserId,
          zidEndpoint
        );

        // Convert tokens to RemoteAuthState
        // login_type comes from the service (or falls back to 'machine_key' for this flow)
        const authState: RemoteAuthState = {
          serverEndpoint: zidEndpoint,
          accessToken: tokens.access_token,
          tokenExpiresAt: new Date(tokens.expires_at).getTime(),
          refreshToken: tokens.refresh_token,
          scopes: ['read', 'write', 'sync'],
          sessionId: tokens.session_id,
          machineId: tokens.machine_id,
          loginType: (tokens.login_type as RemoteAuthState['loginType']) ?? 'machine_key',
        };

        setRemoteAuthState(authState);
        console.log('[useZeroIdAuth] Machine key login successful');
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : 'Machine key authentication failed';
        setError(errorMsg);
        throw err;
      } finally {
        setIsAuthenticating(false);
      }
    },
    [currentUserId]
  );

  const enrollMachine = useCallback(
    async (zidEndpoint: string = DEFAULT_ZID_ENDPOINT) => {
      setIsAuthenticating(true);
      setError(null);

      try {
        if (!clientRef.current) {
          throw new Error('Supervisor not available - please wait for system to initialize');
        }
        if (!currentUserId) {
          throw new Error('You must be logged in locally before enrolling machine');
        }

        // Call identity service to enroll machine with ZID server
        const tokens: ZidTokens = await clientRef.current.enrollMachine(currentUserId, zidEndpoint);

        // Convert tokens to RemoteAuthState (enrollment also logs you in)
        // login_type comes from the service (or falls back to 'machine_key' for enrollment)
        const authState: RemoteAuthState = {
          serverEndpoint: zidEndpoint,
          accessToken: tokens.access_token,
          tokenExpiresAt: new Date(tokens.expires_at).getTime(),
          refreshToken: tokens.refresh_token,
          scopes: ['read', 'write', 'sync'],
          sessionId: tokens.session_id,
          machineId: tokens.machine_id,
          loginType: (tokens.login_type as RemoteAuthState['loginType']) ?? 'machine_key',
        };

        setRemoteAuthState(authState);
        console.log('[useZeroIdAuth] Machine enrollment successful');
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : 'Machine enrollment failed';
        setError(errorMsg);
        throw err;
      } finally {
        setIsAuthenticating(false);
      }
    },
    [currentUserId]
  );

  const disconnect = useCallback(async () => {
    setIsAuthenticating(true);
    setError(null);

    try {
      // Delete session from VFS via IPC (canonical approach)
      if (clientRef.current && currentUserId) {
        try {
          await clientRef.current.zidLogout(currentUserId);
          console.log('[useZeroIdAuth] Session deleted from VFS via IPC');
        } catch (err) {
          // Log but don't fail - session file might not exist
          console.warn('[useZeroIdAuth] VFS session delete failed:', err);
        }
      }

      // Reset initialized flag so session can be loaded on next connect
      initializedRef.current = false;

      // Clear React state (remote session only, not local identity)
      setRemoteAuthState(null);
      console.log('[useZeroIdAuth] Disconnected from ZERO ID');
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Disconnect failed';
      setError(errorMsg);
      throw err;
    } finally {
      setIsAuthenticating(false);
    }
  }, [currentUserId]);

  const refreshTokenFn = useCallback(async () => {
    if (!remoteAuthState?.refreshToken) {
      throw new Error('No refresh token available');
    }
    if (!clientRef.current || !currentUserId) {
      throw new Error('Not authenticated');
    }

    // Global lock: if another refresh is in progress, wait for it instead of starting a new one
    // This prevents concurrent refresh requests that can cause "refresh token reuse" errors
    if (globalRefreshInProgress && globalRefreshPromise) {
      console.log('[useZeroIdAuth] Refresh already in progress globally, waiting for it');
      return globalRefreshPromise;
    }

    setIsAuthenticating(true);
    setError(null);
    lastRefreshAttemptRef.current = Date.now();
    globalRefreshInProgress = true;

    const doRefresh = async () => {
      try {
        const tokens = await clientRef.current!.refreshToken(
          currentUserId!,
          remoteAuthState.serverEndpoint
        );

        // Update state with new tokens (login_type is preserved from original session by service)
        setRemoteAuthState({
          ...remoteAuthState,
          accessToken: tokens.access_token,
          refreshToken: tokens.refresh_token,
          tokenExpiresAt: new Date(tokens.expires_at).getTime(),
          sessionId: tokens.session_id,
          machineId: tokens.machine_id,
          loginType: (tokens.login_type as RemoteAuthState['loginType']) ?? remoteAuthState.loginType,
        });

        // Reset fail count on success
        refreshFailCountRef.current = 0;
        console.log('[useZeroIdAuth] Token refresh successful');
      } catch (err) {
        // Increment fail count for backoff
        refreshFailCountRef.current += 1;
        const errorMsg = err instanceof Error ? err.message : 'Token refresh failed';
        setError(errorMsg);
        throw err;
      } finally {
        setIsAuthenticating(false);
        globalRefreshInProgress = false;
        globalRefreshPromise = null;
      }
    };

    globalRefreshPromise = doRefresh();
    return globalRefreshPromise;
  }, [remoteAuthState, currentUserId, setRemoteAuthState]);

  // Auto-refresh tokens 5 minutes before expiry
  useEffect(() => {
    if (!remoteAuthState?.tokenExpiresAt || !remoteAuthState?.refreshToken) {
      return;
    }

    // Don't auto-refresh while another operation is in progress
    // This prevents concurrent VFS operations that can cause state machine errors
    if (isAuthenticating) {
      console.log('[useZeroIdAuth] Skipping auto-refresh, operation in progress');
      return;
    }

    const expiresIn = remoteAuthState.tokenExpiresAt - Date.now();
    const refreshBuffer = 5 * 60 * 1000; // 5 minutes
    const refreshIn = expiresIn - refreshBuffer;

    if (refreshIn <= 0) {
      // Token expired or about to expire - check for backoff before retrying
      const timeSinceLastAttempt = Date.now() - lastRefreshAttemptRef.current;
      // Exponential backoff: 30s, 60s, 120s, 240s, max 5 min
      const backoffMs = Math.min(
        30_000 * Math.pow(2, refreshFailCountRef.current),
        5 * 60 * 1000
      );

      if (refreshFailCountRef.current > 0 && timeSinceLastAttempt < backoffMs) {
        const waitTime = Math.ceil((backoffMs - timeSinceLastAttempt) / 1000);
        console.log(
          `[useZeroIdAuth] Token refresh failed ${refreshFailCountRef.current} times, ` +
          `waiting ${waitTime}s before retry (backoff)`
        );
        // Schedule retry after backoff period
        const retryTimer = setTimeout(() => {
          console.log('[useZeroIdAuth] Retrying token refresh after backoff');
          refreshTokenFn().catch((err) => {
            console.error('[useZeroIdAuth] Auto-refresh retry failed:', err);
          });
        }, backoffMs - timeSinceLastAttempt);
        return () => clearTimeout(retryTimer);
      }

      // No backoff needed (first attempt or backoff period elapsed)
      console.log('[useZeroIdAuth] Token expiring soon, refreshing immediately');
      refreshTokenFn().catch((err) => {
        console.error('[useZeroIdAuth] Auto-refresh failed:', err);
      });
      return;
    }

    console.log(`[useZeroIdAuth] Scheduling token refresh in ${Math.round(refreshIn / 1000 / 60)} minutes`);
    const timerId = setTimeout(() => {
      console.log('[useZeroIdAuth] Auto-refreshing token');
      refreshTokenFn().catch((err) => {
        console.error('[useZeroIdAuth] Scheduled auto-refresh failed:', err);
      });
    }, refreshIn);

    return () => clearTimeout(timerId);
  }, [remoteAuthState?.tokenExpiresAt, remoteAuthState?.refreshToken, refreshTokenFn, isAuthenticating]);

  const getTimeRemaining = useCallback((): string => {
    if (!remoteAuthState) {
      return 'Not connected';
    }
    return formatTimeRemaining(remoteAuthState.tokenExpiresAt);
  }, [remoteAuthState]);

  const isTokenExpired = useCallback((): boolean => {
    if (!remoteAuthState) {
      return true;
    }
    return Date.now() >= remoteAuthState.tokenExpiresAt;
  }, [remoteAuthState]);

  return {
    remoteAuthState,
    isAuthenticating,
    isLoadingSession,
    error,
    loginWithEmail,
    loginWithMachineKey,
    enrollMachine,
    disconnect,
    refreshToken: refreshTokenFn,
    getTimeRemaining,
    isTokenExpired,
  };
}
