import { createContext, useContext, useCallback } from 'react';
import { 
  useIdentityStore,
  selectCurrentUser,
  selectCurrentSession,
  selectUsers,
  selectIdentityIsLoading,
  selectIdentityError,
  formatUserId,
  getSessionTimeRemaining,
  isSessionExpired,
  type User,
  type Session,
  type UserId,
  type SessionId,
  type UserStatus,
} from '../../stores';

// =============================================================================
// Re-export Types from Store
// =============================================================================

export type { User, Session, UserId, SessionId, UserStatus };

/** Identity context state */
export interface IdentityState {
  /** Current user (null if not logged in) */
  currentUser: User | null;
  /** Current session (null if not logged in) */
  currentSession: Session | null;
  /** All users on this machine */
  users: User[];
  /** Loading state */
  isLoading: boolean;
  /** Error message */
  error: string | null;
}

/** Identity service interface */
export interface IdentityService {
  /** Get current state */
  state: IdentityState;
  
  /** List all users */
  listUsers: () => Promise<User[]>;
  
  /** Create a new user */
  createUser: (displayName: string) => Promise<User>;
  
  /** Login as a user (creates a session) */
  login: (userId: UserId) => Promise<Session | void>;
  
  /** Logout current session */
  logout: () => Promise<void>;
  
  /** Switch to another user */
  switchUser: (userId: UserId) => Promise<void>;
  
  /** Refresh current session */
  refreshSession: () => Promise<void>;
}

// =============================================================================
// Context (for backward compatibility)
// =============================================================================

export const IdentityContext = createContext<IdentityService | null>(null);

/** 
 * Hook to access identity service.
 * 
 * @deprecated Use `useIdentityStore` directly for better performance.
 * This hook is kept for backward compatibility with context-based consumers.
 */
export function useIdentity(): IdentityService | null {
  return useContext(IdentityContext);
}

/** Provider component */
export const IdentityProvider = IdentityContext.Provider;

// =============================================================================
// Hook for managing identity state (now backed by Zustand store)
// =============================================================================

/**
 * Hook for managing identity state.
 * 
 * @deprecated Use `useIdentityStore` directly for better performance.
 * This hook provides backward compatibility with the old API shape.
 */
export function useIdentityState(): IdentityService {
  const store = useIdentityStore();
  
  // Build state object from store selectors
  const currentUser = useIdentityStore(selectCurrentUser);
  const currentSession = useIdentityStore(selectCurrentSession);
  const users = useIdentityStore(selectUsers);
  const isLoading = useIdentityStore(selectIdentityIsLoading);
  const error = useIdentityStore(selectIdentityError);

  const state: IdentityState = {
    currentUser,
    currentSession,
    users,
    isLoading,
    error,
  };

  const listUsers = useCallback(async (): Promise<User[]> => {
    return store.users;
  }, [store.users]);

  const createUser = useCallback(async (displayName: string): Promise<User> => {
    return store.createUser(displayName);
  }, [store]);

  const login = useCallback(async (userId: UserId): Promise<void> => {
    return store.login(userId);
  }, [store]);

  const logout = useCallback(async (): Promise<void> => {
    return store.logout();
  }, [store]);

  const switchUser = useCallback(async (userId: UserId): Promise<void> => {
    return store.switchUser(userId);
  }, [store]);

  const refreshSession = useCallback(async (): Promise<void> => {
    return store.refreshSession();
  }, [store]);

  return {
    state,
    listUsers,
    createUser,
    login,
    logout,
    switchUser,
    refreshSession,
  };
}

// =============================================================================
// Re-export Utility Functions from Store
// =============================================================================

export { formatUserId, getSessionTimeRemaining, isSessionExpired };
