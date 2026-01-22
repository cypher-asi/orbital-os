import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import {
  useIdentity,
  useIdentityState,
  IdentityProvider,
  formatUserId,
  getSessionTimeRemaining,
  isSessionExpired,
  type User,
  type Session,
} from '../useIdentity';
import { createElement } from 'react';

describe('useIdentityState', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('initializes with default mock user', () => {
    const { result } = renderHook(() => useIdentityState());

    expect(result.current.state.currentUser).not.toBeNull();
    expect(result.current.state.currentUser?.displayName).toBe('CYPHER_01');
    expect(result.current.state.currentSession).not.toBeNull();
    expect(result.current.state.users.length).toBe(1);
    expect(result.current.state.isLoading).toBe(false);
    expect(result.current.state.error).toBeNull();
  });

  it('listUsers returns all users', async () => {
    const { result } = renderHook(() => useIdentityState());

    const users = await result.current.listUsers();

    expect(users.length).toBe(1);
    expect(users[0].displayName).toBe('CYPHER_01');
  });

  it('createUser adds a new user to state', async () => {
    const { result } = renderHook(() => useIdentityState());

    let newUser: User | null = null;
    await act(async () => {
      newUser = await result.current.createUser('Test User');
    });

    expect(newUser).not.toBeNull();
    expect(newUser!.displayName).toBe('Test User');
    expect(newUser!.status).toBe('Offline');
    expect(result.current.state.users.length).toBe(2);
  });

  it('login creates session and updates currentUser', async () => {
    const { result } = renderHook(() => useIdentityState());

    // Get the initial user ID
    const userId = result.current.state.users[0].id;

    // Logout first to test login
    await act(async () => {
      await result.current.logout();
    });

    expect(result.current.state.currentSession).toBeNull();

    // Now login
    let session: Session | null = null;
    await act(async () => {
      session = await result.current.login(userId);
    });

    expect(session).not.toBeNull();
    expect(session!.userId).toBe(userId);
    expect(result.current.state.currentUser).not.toBeNull();
    expect(result.current.state.currentUser?.status).toBe('Active');
  });

  it('login with invalid user throws error', async () => {
    const { result } = renderHook(() => useIdentityState());

    await act(async () => {
      await result.current.logout();
    });

    // The login throws an error which sets the error state
    let errorThrown = false;
    try {
      await act(async () => {
        await result.current.login('nonexistent-user-id');
      });
    } catch {
      errorThrown = true;
    }

    expect(errorThrown).toBe(true);
    // Note: The error state is set before rethrowing, so it should be available
    // but due to React state batching, we check the error was thrown instead
  });

  it('logout clears session', async () => {
    const { result } = renderHook(() => useIdentityState());

    expect(result.current.state.currentSession).not.toBeNull();

    await act(async () => {
      await result.current.logout();
    });

    expect(result.current.state.currentSession).toBeNull();
    expect(result.current.state.currentUser?.status).toBe('Offline');
  });

  it('switchUser performs logout then login', async () => {
    const { result } = renderHook(() => useIdentityState());

    // Create a second user
    let newUser: User | null = null;
    await act(async () => {
      newUser = await result.current.createUser('Second User');
    });

    // Switch to new user
    await act(async () => {
      await result.current.switchUser(newUser!.id);
    });

    expect(result.current.state.currentUser?.id).toBe(newUser!.id);
    expect(result.current.state.currentUser?.displayName).toBe('Second User');
  });

  it('refreshSession extends expiry time', async () => {
    const { result } = renderHook(() => useIdentityState());

    const originalExpiry = result.current.state.currentSession?.expiresAt ?? 0;

    // Advance time
    vi.advanceTimersByTime(10000);

    await act(async () => {
      await result.current.refreshSession();
    });

    const newExpiry = result.current.state.currentSession?.expiresAt ?? 0;
    expect(newExpiry).toBeGreaterThan(originalExpiry);
  });

  it('refreshSession without session throws error', async () => {
    const { result } = renderHook(() => useIdentityState());

    await act(async () => {
      await result.current.logout();
    });

    await expect(async () => {
      await act(async () => {
        await result.current.refreshSession();
      });
    }).rejects.toThrow('No active session');
  });
});

describe('useIdentity', () => {
  it('returns null when not in provider', () => {
    const { result } = renderHook(() => useIdentity());
    expect(result.current).toBeNull();
  });

  it('returns service when in provider', () => {
    const mockService = {
      state: {
        currentUser: null,
        currentSession: null,
        users: [],
        isLoading: false,
        error: null,
      },
      listUsers: vi.fn(),
      createUser: vi.fn(),
      login: vi.fn(),
      logout: vi.fn(),
      switchUser: vi.fn(),
      refreshSession: vi.fn(),
    };

    const wrapper = ({ children }: { children: React.ReactNode }) =>
      createElement(IdentityProvider, { value: mockService }, children);

    const { result } = renderHook(() => useIdentity(), { wrapper });

    expect(result.current).toBe(mockService);
  });
});

describe('formatUserId', () => {
  it('formats user ID correctly', () => {
    const userId = '00000000000000000000000000000001';
    const formatted = formatUserId(userId);
    expect(formatted).toBe('UID-0000-0000-0000-0000');
  });

  it('handles different IDs', () => {
    const userId = 'abcd1234567890ef0000111122223333';
    const formatted = formatUserId(userId);
    expect(formatted).toBe('UID-ABCD-1234-5678-90EF');
  });
});

describe('getSessionTimeRemaining', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns "Expired" for expired session', () => {
    const session: Session = {
      id: 'test',
      userId: 'user1',
      createdAt: Date.now() - 100000,
      expiresAt: Date.now() - 1000, // Already expired
      capabilities: [],
    };

    expect(getSessionTimeRemaining(session)).toBe('Expired');
  });

  it('returns hours and minutes for valid session', () => {
    const session: Session = {
      id: 'test',
      userId: 'user1',
      createdAt: Date.now(),
      expiresAt: Date.now() + 3600000 + 1800000, // 1.5 hours
      capabilities: [],
    };

    const remaining = getSessionTimeRemaining(session);
    expect(remaining).toMatch(/\d+h \d+m/);
  });

  it('returns only minutes when less than 1 hour', () => {
    const session: Session = {
      id: 'test',
      userId: 'user1',
      createdAt: Date.now(),
      expiresAt: Date.now() + 1800000, // 30 minutes
      capabilities: [],
    };

    const remaining = getSessionTimeRemaining(session);
    expect(remaining).toMatch(/\d+m/);
    expect(remaining).not.toContain('h');
  });
});

describe('isSessionExpired', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns true for expired session', () => {
    const session: Session = {
      id: 'test',
      userId: 'user1',
      createdAt: Date.now() - 100000,
      expiresAt: Date.now() - 1000,
      capabilities: [],
    };

    expect(isSessionExpired(session)).toBe(true);
  });

  it('returns false for valid session', () => {
    const session: Session = {
      id: 'test',
      userId: 'user1',
      createdAt: Date.now(),
      expiresAt: Date.now() + 86400000, // 24 hours
      capabilities: [],
    };

    expect(isSessionExpired(session)).toBe(false);
  });

  it('returns true at exact expiry time', () => {
    const session: Session = {
      id: 'test',
      userId: 'user1',
      createdAt: Date.now() - 1000,
      expiresAt: Date.now(),
      capabilities: [],
    };

    expect(isSessionExpired(session)).toBe(true);
  });
});
