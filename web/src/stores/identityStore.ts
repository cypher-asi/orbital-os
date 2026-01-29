/**
 * Identity Store - Centralized state for user/session management.
 *
 * Manages user authentication, sessions, and user list.
 * Also manages ZERO ID remote auth state (shared across all consumers).
 * Persists user list to localStorage for development.
 */

import { create } from 'zustand';
import { persist, subscribeWithSelector } from 'zustand/middleware';

// =============================================================================
// ZERO ID Remote Auth Types
// =============================================================================

/** Remote authentication state for ZERO ID */
export interface RemoteAuthState {
  /** Remote authentication server endpoint */
  serverEndpoint: string;
  /** OAuth2/OIDC access token */
  accessToken: string;
  /** When the access token expires (timestamp) */
  tokenExpiresAt: number;
  /** Refresh token (if available) */
  refreshToken: string | null;
  /** Granted OAuth scopes */
  scopes: string[];
  /** Session ID from ZID server */
  sessionId: string;
  /** Machine ID used for this session */
  machineId: string;
}

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

// =============================================================================
// Store Types
// =============================================================================

interface IdentityStoreState {
  currentUser: User | null;
  currentSession: Session | null;
  users: User[];
  isLoading: boolean;
  error: string | null;

  // ZERO ID remote auth state (shared across all consumers)
  remoteAuthState: RemoteAuthState | null;

  // Actions
  setCurrentUser: (user: User | null) => void;
  setCurrentSession: (session: Session | null) => void;
  setUsers: (users: User[]) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
  setRemoteAuthState: (state: RemoteAuthState | null) => void;

  // Async actions (will call supervisor when integrated)
  login: (userId: UserId) => Promise<void>;
  logout: () => Promise<void>;
  createUser: (displayName: string) => Promise<User>;
  switchUser: (userId: UserId) => Promise<void>;
  refreshSession: () => Promise<void>;
}

// =============================================================================
// Default Mock Data (for development)
// =============================================================================

const MOCK_USER: User = {
  id: '00000000000000000000000000000001',
  displayName: 'CYPHER_01',
  status: 'Active',
  createdAt: Date.now() - 86400000,
  lastActiveAt: Date.now(),
};

const MOCK_SESSION: Session = {
  id: '00000000000000000000000000000001',
  userId: MOCK_USER.id,
  createdAt: Date.now() - 3600000,
  expiresAt: Date.now() + 82800000, // 23 hours from now
  capabilities: ['endpoint.read', 'endpoint.write', 'console.read', 'console.write'],
};

// =============================================================================
// Store Creation
// =============================================================================

export const useIdentityStore = create<IdentityStoreState>()(
  subscribeWithSelector(
    persist(
      (set, get) => ({
        currentUser: MOCK_USER,
        currentSession: MOCK_SESSION,
        users: [MOCK_USER],
        isLoading: false,
        error: null,
        remoteAuthState: null,

        setCurrentUser: (currentUser) => set({ currentUser }),
        setCurrentSession: (currentSession) => set({ currentSession }),
        setUsers: (users) => set({ users }),
        setLoading: (isLoading) => set({ isLoading }),
        setError: (error) => set({ error }),
        setRemoteAuthState: (remoteAuthState) => set({ remoteAuthState }),

        login: async (userId) => {
          set({ isLoading: true, error: null });
          try {
            // TODO: Call supervisor.identity_* methods when available
            const user = get().users.find((u) => u.id === userId);
            if (!user) throw new Error('User not found');

            const session: Session = {
              id: crypto.randomUUID().replace(/-/g, ''),
              userId,
              createdAt: Date.now(),
              expiresAt: Date.now() + 86400000, // 24 hours
              capabilities: ['endpoint.read', 'endpoint.write'],
            };

            set({
              currentUser: { ...user, status: 'Active', lastActiveAt: Date.now() },
              currentSession: session,
              isLoading: false,
            });
          } catch (error) {
            set({
              isLoading: false,
              error: error instanceof Error ? error.message : 'Login failed',
            });
            throw error;
          }
        },

        logout: async () => {
          set({ isLoading: true, error: null });
          try {
            // Clear current user completely to allow a different user to log in
            set({
              currentUser: null,
              currentSession: null,
              isLoading: false,
            });
          } catch (error) {
            set({
              isLoading: false,
              error: error instanceof Error ? error.message : 'Logout failed',
            });
            throw error;
          }
        },

        createUser: async (displayName) => {
          const newUser: User = {
            id: crypto.randomUUID().replace(/-/g, ''),
            displayName,
            status: 'Offline',
            createdAt: Date.now(),
            lastActiveAt: Date.now(),
          };
          set({ users: [...get().users, newUser] });
          return newUser;
        },

        switchUser: async (userId) => {
          await get().logout();
          await get().login(userId);
        },

        refreshSession: async () => {
          const session = get().currentSession;
          if (!session) {
            throw new Error('No active session');
          }

          // TODO: Call supervisor to refresh session via zos-identity
          set({
            currentSession: {
              ...session,
              expiresAt: Date.now() + 86400000, // Extend by 24 hours
            },
          });
        },
      }),
      {
        name: 'zero-identity-store',
        partialize: (state) => ({
          users: state.users,
          // Don't persist session - should be re-authenticated
        }),
      }
    )
  )
);

// =============================================================================
// Selectors for Fine-Grained Subscriptions
// =============================================================================

/** Select current user */
export const selectCurrentUser = (state: IdentityStoreState) => state.currentUser;

/** Select current session */
export const selectCurrentSession = (state: IdentityStoreState) => state.currentSession;

/** Select all users */
export const selectUsers = (state: IdentityStoreState) => state.users;

/** Select loading state */
export const selectIsLoading = (state: IdentityStoreState) => state.isLoading;

/** Select error state */
export const selectError = (state: IdentityStoreState) => state.error;

/** Select whether user is logged in */
export const selectIsLoggedIn = (state: IdentityStoreState) =>
  state.currentUser !== null && state.currentSession !== null;

/** Select remote auth state (ZERO ID) */
export const selectRemoteAuthState = (state: IdentityStoreState) => state.remoteAuthState;

/** Select user by ID */
export const selectUserById = (id: UserId) => (state: IdentityStoreState) =>
  state.users.find((u) => u.id === id);

// =============================================================================
// Utility Functions
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
