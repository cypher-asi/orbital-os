import { useState, useCallback, useEffect, useRef } from 'react';
import {
  IdentityServiceClient,
  type ZidTokens,
  type ZidSession,
  VfsStorageClient,
  formatUserId,
  ZidInvalidRefreshTokenError,
  ServiceNotFoundError,
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
  
  // Track last successful refresh to prevent rapid successive refreshes
  // This is critical for token rotation: we must wait for VFS to persist the new token
  const lastSuccessfulRefreshRef = useRef<number>(0);
  const MIN_REFRESH_COOLDOWN_MS = 5000; // Minimum 5 seconds between refreshes

  // Track the last refresh token used to prevent duplicate refresh with same token
  // This catches race conditions where the same token is sent multiple times
  const lastUsedRefreshTokenRef = useRef<string | null>(null);

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

        // Load session if it has a refresh_token - even if expired, we can refresh it
        if (session && session.refresh_token) {
          const isExpired = session.expires_at <= Date.now();
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
          if (isExpired) {
            console.log('[useZeroIdAuth] Loaded expired session from VFS cache, will auto-refresh');
          } else {
            console.log('[useZeroIdAuth] Restored valid session from VFS cache');
          }
        } else if (session) {
          console.log('[useZeroIdAuth] Found session without refresh_token in VFS cache');
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
        // Retry up to 3 times with delay if service is not found (startup timing)
        let tokens: ZidTokens | null = null;
        let lastError: Error | null = null;
        const maxRetries = 3;
        const retryDelayMs = 1000;

        for (let attempt = 0; attempt < maxRetries; attempt++) {
          try {
            tokens = await clientRef.current.loginWithEmail(
              currentUserId,
              email,
              password,
              zidEndpoint
            );
            break; // Success - exit retry loop
          } catch (err) {
            lastError = err instanceof Error ? err : new Error('Unknown error');
            
            // Only retry for ServiceNotFoundError (startup timing)
            if (err instanceof ServiceNotFoundError) {
              if (attempt < maxRetries - 1) {
                console.log(`[useZeroIdAuth] Identity service not ready, retrying in ${retryDelayMs}ms (attempt ${attempt + 1}/${maxRetries})`);
                await new Promise(resolve => setTimeout(resolve, retryDelayMs));
                continue;
              }
              // Last attempt failed - provide helpful error message
              throw new Error('Identity service is still starting up. Please wait a few seconds and try again.');
            }
            // For other errors, don't retry
            throw err;
          }
        }

        if (!tokens) {
          throw lastError || new Error('Email authentication failed');
        }

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
        // Retry up to 3 times with delay if service is not found (startup timing)
        let tokens: ZidTokens | null = null;
        let lastError: Error | null = null;
        const maxRetries = 3;
        const retryDelayMs = 1000;

        for (let attempt = 0; attempt < maxRetries; attempt++) {
          try {
            tokens = await clientRef.current.loginWithMachineKey(
              currentUserId,
              zidEndpoint
            );
            break; // Success - exit retry loop
          } catch (err) {
            lastError = err instanceof Error ? err : new Error('Unknown error');
            
            // Only retry for ServiceNotFoundError (startup timing)
            if (err instanceof ServiceNotFoundError) {
              if (attempt < maxRetries - 1) {
                console.log(`[useZeroIdAuth] Identity service not ready, retrying in ${retryDelayMs}ms (attempt ${attempt + 1}/${maxRetries})`);
                await new Promise(resolve => setTimeout(resolve, retryDelayMs));
                continue;
              }
              // Last attempt failed - provide helpful error message
              throw new Error('Identity service is still starting up. Please wait a few seconds and try again.');
            }
            // For other errors, don't retry
            throw err;
          }
        }

        if (!tokens) {
          throw lastError || new Error('Machine key authentication failed');
        }

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
        // Retry up to 3 times with delay if service is not found (startup timing)
        let tokens: ZidTokens | null = null;
        let lastError: Error | null = null;
        const maxRetries = 3;
        const retryDelayMs = 1000;

        for (let attempt = 0; attempt < maxRetries; attempt++) {
          try {
            tokens = await clientRef.current.enrollMachine(currentUserId, zidEndpoint);
            break; // Success - exit retry loop
          } catch (err) {
            lastError = err instanceof Error ? err : new Error('Unknown error');
            
            // Only retry for ServiceNotFoundError (startup timing)
            if (err instanceof ServiceNotFoundError) {
              if (attempt < maxRetries - 1) {
                console.log(`[useZeroIdAuth] Identity service not ready, retrying in ${retryDelayMs}ms (attempt ${attempt + 1}/${maxRetries})`);
                await new Promise(resolve => setTimeout(resolve, retryDelayMs));
                continue;
              }
              // Last attempt failed - provide helpful error message
              throw new Error('Identity service is still starting up. Please wait a few seconds and try again.');
            }
            // For other errors, don't retry
            throw err;
          }
        }

        if (!tokens) {
          throw lastError || new Error('Machine enrollment failed');
        }

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
    // Get latest state directly from store to avoid stale closure issues
    // This ensures we always use the most recent refresh token from React state,
    // which is passed directly to the backend to prevent VFS race conditions.
    const latestState = useIdentityStore.getState().remoteAuthState;
    
    if (!latestState?.refreshToken) {
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

    // Detect if we're trying to use the same refresh token that was already consumed
    // This catches edge cases where state hasn't been updated yet with the new token
    const currentRefreshToken = latestState.refreshToken;
    if (currentRefreshToken === lastUsedRefreshTokenRef.current) {
      console.warn(
        '[useZeroIdAuth] Blocking duplicate refresh with same token - token already consumed, ' +
        'waiting for new token'
      );
      // Return resolved promise to avoid throwing, the auto-refresh will retry later
      return Promise.resolve();
    }

    setIsAuthenticating(true);
    setError(null);
    lastRefreshAttemptRef.current = Date.now();
    globalRefreshInProgress = true;

    // Mark this token as being used BEFORE making the request
    // This prevents race conditions if another refresh attempt comes in
    lastUsedRefreshTokenRef.current = currentRefreshToken;

    const doRefresh = async () => {
      try {
        // Pass refresh token directly from React state to prevent race conditions
        // where VFS might not have the latest token yet
        const tokens = await clientRef.current!.refreshToken(
          currentUserId!,
          latestState.serverEndpoint,
          currentRefreshToken
        );

        // Update state with new tokens using functional update to ensure we have latest state
        // This prevents race conditions where the closure captures stale remoteAuthState
        const currentState = useIdentityStore.getState().remoteAuthState;
        const newState: RemoteAuthState = {
          serverEndpoint: currentState?.serverEndpoint ?? latestState.serverEndpoint,
          accessToken: tokens.access_token,
          refreshToken: tokens.refresh_token,
          tokenExpiresAt: new Date(tokens.expires_at).getTime(),
          scopes: currentState?.scopes ?? latestState.scopes,
          sessionId: tokens.session_id,
          machineId: tokens.machine_id,
          loginType: (tokens.login_type as RemoteAuthState['loginType']) ?? currentState?.loginType ?? latestState.loginType,
        };
        setRemoteAuthState(newState);

        // Reset fail count and record successful refresh time
        refreshFailCountRef.current = 0;
        lastSuccessfulRefreshRef.current = Date.now();
        // Note: We intentionally do NOT update lastUsedRefreshTokenRef here.
        // It should remain set to the OLD token that was just consumed.
        // This allows future refreshes with the NEW token (tokens.refresh_token)
        // while still blocking any stale attempts to use the old consumed token.
        console.log('[useZeroIdAuth] Token refresh successful, new refresh_token stored in state');
      } catch (err) {
        const errorMsg = err instanceof Error ? err.message : 'Token refresh failed';
        
        // ServiceNotFoundError means identity service isn't ready yet - don't count as failure
        // This is a startup timing issue, not a real refresh problem
        if (err instanceof ServiceNotFoundError) {
          console.log('[useZeroIdAuth] Identity service not ready, will retry later');
          // Don't set error or increment fail count - just clear the used token ref
          // so the next attempt with same token is allowed
          lastUsedRefreshTokenRef.current = null;
          throw err;
        }

        // Increment fail count for backoff (only for real failures)
        refreshFailCountRef.current += 1;
        setError(errorMsg);

        // Auto-clear session on InvalidRefreshToken to force re-authentication
        // This handles token reuse detection, expired tokens, and revoked tokens
        if (err instanceof ZidInvalidRefreshTokenError) {
          console.warn(
            '[useZeroIdAuth] Refresh token invalid/reused/revoked - clearing session to force re-auth'
          );
          setRemoteAuthState(null);
          // Clear the tracked token since session is being cleared
          lastUsedRefreshTokenRef.current = null;
        }

        throw err;
      } finally {
        setIsAuthenticating(false);
        globalRefreshInProgress = false;
        globalRefreshPromise = null;
      }
    };

    globalRefreshPromise = doRefresh();
    return globalRefreshPromise;
  }, [currentUserId, setRemoteAuthState]);

  // Auto-refresh tokens 5 minutes before expiry
  useEffect(() => {
    if (!remoteAuthState?.tokenExpiresAt || !remoteAuthState?.refreshToken) {
      return;
    }

    // Don't auto-refresh if client is not available (service not started yet)
    if (!clientRef.current) {
      console.log('[useZeroIdAuth] Skipping auto-refresh, client not available yet');
      return;
    }

    // Don't auto-refresh while another operation is in progress
    // This prevents concurrent VFS operations that can cause state machine errors
    if (isAuthenticating) {
      console.log('[useZeroIdAuth] Skipping auto-refresh scheduling, operation in progress');
      return;
    }

    const expiresIn = remoteAuthState.tokenExpiresAt - Date.now();
    const refreshBuffer = 5 * 60 * 1000; // 5 minutes
    const refreshIn = expiresIn - refreshBuffer;

    // Check if we're in cooldown after a recent successful refresh
    const timeSinceLastSuccess = Date.now() - lastSuccessfulRefreshRef.current;
    const inCooldown = timeSinceLastSuccess < MIN_REFRESH_COOLDOWN_MS && lastSuccessfulRefreshRef.current > 0;

    if (refreshIn <= 0) {
      // Token expired or about to expire
      
      // If in cooldown, schedule refresh after cooldown ends instead of refreshing immediately
      if (inCooldown) {
        const cooldownRemaining = MIN_REFRESH_COOLDOWN_MS - timeSinceLastSuccess;
        console.log(`[useZeroIdAuth] Token needs refresh but in cooldown, scheduling in ${Math.round(cooldownRemaining / 1000)}s`);
        const cooldownTimer = setTimeout(() => {
          console.log('[useZeroIdAuth] Cooldown ended, refreshing token');
          refreshTokenFn().catch((err) => {
            console.error('[useZeroIdAuth] Post-cooldown refresh failed:', err);
          });
        }, cooldownRemaining);
        return () => clearTimeout(cooldownTimer);
      }

      // Check for backoff after failed attempts
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

    // Token not expired yet - schedule refresh for later
    // If in cooldown, we still schedule normally since cooldown will be over by then
    console.log(`[useZeroIdAuth] Scheduling token refresh in ${Math.round(refreshIn / 1000 / 60)} minutes`);
    const timerId = setTimeout(() => {
      console.log('[useZeroIdAuth] Auto-refreshing token');
      refreshTokenFn().catch((err) => {
        console.error('[useZeroIdAuth] Scheduled auto-refresh failed:', err);
      });
    }, refreshIn);

    return () => clearTimeout(timerId);
  }, [remoteAuthState?.tokenExpiresAt, remoteAuthState?.refreshToken, refreshTokenFn, isAuthenticating, supervisor]);

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
