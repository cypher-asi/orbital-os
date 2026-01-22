import { createContext, useContext, useState, useCallback } from 'react';

// =============================================================================
// Identity Types
// =============================================================================

/** User ID type (128-bit UUID as hex string) */
export type UserId = string;

/** Session ID type (128-bit UUID as hex string) */
export type SessionId = string;

/** User status */
export type UserStatus = 'Active' | 'Offline' | 'Suspended';

/** User information */
export interface User {
  id: UserId;
  displayName: string;
  status: UserStatus;
  createdAt: number;
  lastActiveAt: number;
}

/** Session information */
export interface Session {
  id: SessionId;
  userId: UserId;
  createdAt: number;
  expiresAt: number;
  capabilities: string[];
}

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
  login: (userId: UserId) => Promise<Session>;
  
  /** Logout current session */
  logout: () => Promise<void>;
  
  /** Switch to another user */
  switchUser: (userId: UserId) => Promise<void>;
  
  /** Refresh current session */
  refreshSession: () => Promise<void>;
}

// =============================================================================
// Mock Implementation (for development)
// =============================================================================

/** Default mock user */
const MOCK_USER: User = {
  id: '00000000000000000000000000000001',
  displayName: 'CYPHER_01',
  status: 'Active',
  createdAt: Date.now() - 86400000,
  lastActiveAt: Date.now(),
};

/** Default mock session */
const MOCK_SESSION: Session = {
  id: '00000000000000000000000000000001',
  userId: MOCK_USER.id,
  createdAt: Date.now() - 3600000,
  expiresAt: Date.now() + 82800000, // 23 hours from now
  capabilities: ['endpoint.read', 'endpoint.write', 'console.read', 'console.write'],
};

/** Initial identity state */
const INITIAL_STATE: IdentityState = {
  currentUser: MOCK_USER,
  currentSession: MOCK_SESSION,
  users: [MOCK_USER],
  isLoading: false,
  error: null,
};

// =============================================================================
// Context and Hook
// =============================================================================

export const IdentityContext = createContext<IdentityService | null>(null);

/** Hook to access identity service */
export function useIdentity(): IdentityService | null {
  return useContext(IdentityContext);
}

/** Provider component */
export const IdentityProvider = IdentityContext.Provider;

// =============================================================================
// Hook for managing identity state
// =============================================================================

export function useIdentityState(): IdentityService {
  const [state, setState] = useState<IdentityState>(INITIAL_STATE);

  const listUsers = useCallback(async (): Promise<User[]> => {
    // TODO: Call supervisor to get users from zos-identity
    return state.users;
  }, [state.users]);

  const createUser = useCallback(async (displayName: string): Promise<User> => {
    // TODO: Call supervisor to create user via zos-identity
    const newUser: User = {
      id: Math.random().toString(16).slice(2).padEnd(32, '0'),
      displayName,
      status: 'Offline',
      createdAt: Date.now(),
      lastActiveAt: Date.now(),
    };
    setState(prev => ({
      ...prev,
      users: [...prev.users, newUser],
    }));
    return newUser;
  }, []);

  const login = useCallback(async (userId: UserId): Promise<Session> => {
    setState(prev => ({ ...prev, isLoading: true, error: null }));
    
    try {
      // TODO: Call supervisor to login via zos-identity
      const user = state.users.find(u => u.id === userId);
      if (!user) {
        throw new Error('User not found');
      }

      const session: Session = {
        id: Math.random().toString(16).slice(2).padEnd(32, '0'),
        userId,
        createdAt: Date.now(),
        expiresAt: Date.now() + 86400000, // 24 hours
        capabilities: ['endpoint.read', 'endpoint.write'],
      };

      setState(prev => ({
        ...prev,
        currentUser: { ...user, status: 'Active' },
        currentSession: session,
        isLoading: false,
      }));

      return session;
    } catch (error) {
      setState(prev => ({
        ...prev,
        isLoading: false,
        error: error instanceof Error ? error.message : 'Login failed',
      }));
      throw error;
    }
  }, [state.users]);

  const logout = useCallback(async (): Promise<void> => {
    setState(prev => ({ ...prev, isLoading: true, error: null }));

    try {
      // TODO: Call supervisor to logout via zos-identity
      setState(prev => ({
        ...prev,
        currentUser: prev.currentUser ? { ...prev.currentUser, status: 'Offline' } : null,
        currentSession: null,
        isLoading: false,
      }));
    } catch (error) {
      setState(prev => ({
        ...prev,
        isLoading: false,
        error: error instanceof Error ? error.message : 'Logout failed',
      }));
      throw error;
    }
  }, []);

  const switchUser = useCallback(async (userId: UserId): Promise<void> => {
    await logout();
    await login(userId);
  }, [logout, login]);

  const refreshSession = useCallback(async (): Promise<void> => {
    if (!state.currentSession) {
      throw new Error('No active session');
    }

    // TODO: Call supervisor to refresh session via zos-identity
    setState(prev => ({
      ...prev,
      currentSession: prev.currentSession
        ? {
            ...prev.currentSession,
            expiresAt: Date.now() + 86400000,
          }
        : null,
    }));
  }, [state.currentSession]);

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
// Utility functions
// =============================================================================

/** Format a user ID for display (shortened) */
export function formatUserId(id: UserId): string {
  return `UID-${id.slice(0, 4).toUpperCase()}-${id.slice(4, 8).toUpperCase()}-${id.slice(8, 12).toUpperCase()}-${id.slice(12, 16).toUpperCase()}`;
}

/** Get time until session expires (human-readable) */
export function getSessionTimeRemaining(session: Session): string {
  const remaining = session.expiresAt - Date.now();
  if (remaining <= 0) {
    return 'Expired';
  }
  const hours = Math.floor(remaining / 3600000);
  const minutes = Math.floor((remaining % 3600000) / 60000);
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  return `${minutes}m`;
}

/** Check if a session is expired */
export function isSessionExpired(session: Session): boolean {
  return Date.now() >= session.expiresAt;
}
