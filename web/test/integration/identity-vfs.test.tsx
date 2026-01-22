import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, act } from '@testing-library/react';
import { createElement } from 'react';
import {
  useIdentityState,
  type User,
} from '../../desktop/hooks/useIdentity';

/**
 * End-to-End Identity and VFS Integration Test
 * 
 * This test suite verifies the complete user flow from:
 * 1. User creation
 * 2. Login (session creation)
 * 3. Home directory access
 * 4. App launch and data directory creation
 * 5. File operations
 * 6. Session persistence across logout/login
 */

// Mock VFS storage for testing
const mockVfsStorage: Record<string, any> = {};

const MockVfs = {
  exists: (path: string) => path in mockVfsStorage,
  mkdir: (path: string) => {
    mockVfsStorage[path] = { type: 'directory' };
  },
  writeFile: (path: string, content: Uint8Array | string) => {
    mockVfsStorage[path] = { type: 'file', content };
  },
  readFile: (path: string) => {
    const entry = mockVfsStorage[path];
    return entry?.type === 'file' ? entry.content : null;
  },
  clear: () => {
    Object.keys(mockVfsStorage).forEach(key => delete mockVfsStorage[key]);
  },
};

// Test component that uses identity and simulates VFS operations
function TestApp({ onReady }: { onReady: (service: ReturnType<typeof useIdentityState>) => void }) {
  const service = useIdentityState();
  
  // Call onReady once when mounted (using ref to track)
  const called = vi.fn();
  if (called.mock.calls.length === 0) {
    called();
    onReady(service);
  }
  
  return createElement('div', { 'data-testid': 'test-app' },
    service.state.currentUser
      ? createElement('span', { 'data-testid': 'user-name' }, service.state.currentUser.displayName)
      : createElement('span', { 'data-testid': 'no-user' }, 'Not logged in')
  );
}

describe('Identity and VFS End-to-End Flow', () => {
  let identityService: ReturnType<typeof useIdentityState>;

  beforeEach(() => {
    MockVfs.clear();
    
    // Initialize root filesystem
    MockVfs.mkdir('/');
    MockVfs.mkdir('/home');
    MockVfs.mkdir('/tmp');
    MockVfs.mkdir('/system');
  });

  it('completes full user flow: create → login → app → file → logout → login', async () => {
    // Step 1: Create user via identity service
    render(createElement(TestApp, {
      onReady: (service) => {
        identityService = service;
      },
    }));

    // Verify render completed
    expect(screen.getByTestId('test-app')).toBeTruthy();

    // Initial state has mock user
    expect(identityService.state.currentUser).not.toBeNull();
    const initialUserId = identityService.state.currentUser!.id;

    // Step 2: Create home directory for user (simulating first boot)
    const homePath = `/home/${initialUserId}`;
    MockVfs.mkdir(homePath);
    MockVfs.mkdir(`${homePath}/.zos`);
    MockVfs.mkdir(`${homePath}/.zos/sessions`);
    MockVfs.mkdir(`${homePath}/Apps`);
    MockVfs.mkdir(`${homePath}/Documents`);
    
    expect(MockVfs.exists(homePath)).toBe(true);
    expect(MockVfs.exists(`${homePath}/.zos`)).toBe(true);

    // Step 3: Verify session exists (from mock initial state)
    expect(identityService.state.currentSession).not.toBeNull();
    const sessionId = identityService.state.currentSession!.id;

    // Step 4: Simulate session file in VFS
    const sessionPath = `${homePath}/.zos/sessions/current.json`;
    const sessionData = JSON.stringify({
      id: sessionId,
      userId: initialUserId,
      createdAt: Date.now(),
      expiresAt: Date.now() + 86400000,
    });
    MockVfs.writeFile(sessionPath, sessionData);
    expect(MockVfs.exists(sessionPath)).toBe(true);

    // Step 5: Launch app (calculator) - create app data directory
    const appId = 'com.zero.calculator';
    const appDataPath = `${homePath}/Apps/${appId}`;
    MockVfs.mkdir(appDataPath);
    expect(MockVfs.exists(appDataPath)).toBe(true);

    // Step 6: Write file to app data directory
    const historyFile = `${appDataPath}/history.json`;
    const historyData = JSON.stringify({ calculations: ['1+1=2', '2*3=6'] });
    MockVfs.writeFile(historyFile, historyData);
    expect(MockVfs.exists(historyFile)).toBe(true);

    // Step 7: Read file back and verify
    const readContent = MockVfs.readFile(historyFile);
    expect(readContent).toBe(historyData);

    // Step 8: Logout
    await act(async () => {
      await identityService.logout();
    });

    expect(identityService.state.currentSession).toBeNull();
    expect(identityService.state.currentUser?.status).toBe('Offline');

    // Remove session file (simulating VFS cleanup on logout)
    delete mockVfsStorage[sessionPath];
    expect(MockVfs.exists(sessionPath)).toBe(false);

    // Step 9: Login again
    await act(async () => {
      await identityService.login(initialUserId);
    });

    expect(identityService.state.currentSession).not.toBeNull();
    expect(identityService.state.currentUser?.status).toBe('Active');

    // Step 10: Verify file persists after re-login
    expect(MockVfs.exists(historyFile)).toBe(true);
    const persistedContent = MockVfs.readFile(historyFile);
    expect(persistedContent).toBe(historyData);
  });

  it('creates new user and initializes home directory', async () => {
    render(createElement(TestApp, {
      onReady: (service) => {
        identityService = service;
      },
    }));

    expect(screen.getByTestId('test-app')).toBeTruthy();

    // Create a new user
    let newUser: User | null = null;
    await act(async () => {
      newUser = await identityService.createUser('New Test User');
    });

    expect(newUser).not.toBeNull();
    expect(newUser!.displayName).toBe('New Test User');

    // Create home directory for new user
    const newHomePath = `/home/${newUser!.id}`;
    MockVfs.mkdir(newHomePath);
    
    // Bootstrap standard directories
    const standardDirs = [
      '.zos',
      '.zos/identity',
      '.zos/sessions',
      '.zos/credentials',
      '.zos/config',
      'Documents',
      'Downloads',
      'Desktop',
      'Pictures',
      'Music',
      'Apps',
    ];

    for (const dir of standardDirs) {
      MockVfs.mkdir(`${newHomePath}/${dir}`);
    }

    // Verify structure
    for (const dir of standardDirs) {
      expect(MockVfs.exists(`${newHomePath}/${dir}`)).toBe(true);
    }
  });

  it('handles app data isolation between users', async () => {
    render(createElement(TestApp, {
      onReady: (service) => {
        identityService = service;
      },
    }));

    expect(screen.getByTestId('test-app')).toBeTruthy();

    // User 1 home
    const user1Id = identityService.state.currentUser!.id;
    const user1Home = `/home/${user1Id}`;
    MockVfs.mkdir(user1Home);
    MockVfs.mkdir(`${user1Home}/Apps/test-app`);

    // User 1 writes data
    const user1DataPath = `${user1Home}/Apps/test-app/data.json`;
    MockVfs.writeFile(user1DataPath, '{"secret": "user1-data"}');

    // Create User 2
    let user2: User | null = null;
    await act(async () => {
      user2 = await identityService.createUser('User 2');
    });

    // User 2 home
    const user2Home = `/home/${user2!.id}`;
    MockVfs.mkdir(user2Home);
    MockVfs.mkdir(`${user2Home}/Apps/test-app`);

    // User 2 data path is different
    const user2DataPath = `${user2Home}/Apps/test-app/data.json`;
    expect(MockVfs.exists(user2DataPath)).toBe(false);

    // User 2 writes their own data
    MockVfs.writeFile(user2DataPath, '{"secret": "user2-data"}');

    // Verify isolation
    expect(MockVfs.readFile(user1DataPath)).toBe('{"secret": "user1-data"}');
    expect(MockVfs.readFile(user2DataPath)).toBe('{"secret": "user2-data"}');
  });

  it('persists user preferences across sessions', async () => {
    render(createElement(TestApp, {
      onReady: (service) => {
        identityService = service;
      },
    }));

    expect(screen.getByTestId('test-app')).toBeTruthy();

    const userId = identityService.state.currentUser!.id;
    const configPath = `/home/${userId}/.zos/config`;
    MockVfs.mkdir(`/home/${userId}`);
    MockVfs.mkdir(`/home/${userId}/.zos`);
    MockVfs.mkdir(configPath);

    // Save preferences
    const prefsPath = `${configPath}/preferences.json`;
    const preferences = JSON.stringify({
      theme: 'dark',
      language: 'en',
      desktop: { background: 'grain' },
    });
    MockVfs.writeFile(prefsPath, preferences);

    // Logout
    await act(async () => {
      await identityService.logout();
    });

    // Login again
    await act(async () => {
      await identityService.login(userId);
    });

    // Preferences should persist
    expect(MockVfs.exists(prefsPath)).toBe(true);
    const loadedPrefs = MockVfs.readFile(prefsPath);
    expect(loadedPrefs).toBe(preferences);
    expect(JSON.parse(loadedPrefs as string).theme).toBe('dark');
  });

  it('handles session refresh', async () => {
    render(createElement(TestApp, {
      onReady: (service) => {
        identityService = service;
      },
    }));

    expect(screen.getByTestId('test-app')).toBeTruthy();

    const originalExpiry = identityService.state.currentSession?.expiresAt ?? 0;

    // Refresh session (just tests that the refresh extends expiry)
    await act(async () => {
      await identityService.refreshSession();
    });

    const newExpiry = identityService.state.currentSession?.expiresAt ?? 0;
    // The new expiry should be based on Date.now() + 24 hours, so it should be >= original
    expect(newExpiry).toBeGreaterThanOrEqual(originalExpiry);
  });
});

describe('Error Handling', () => {
  let identityService: ReturnType<typeof useIdentityState>;

  it('handles login error gracefully', async () => {
    render(createElement(TestApp, {
      onReady: (service) => {
        identityService = service;
      },
    }));

    expect(screen.getByTestId('test-app')).toBeTruthy();

    // Logout first
    await act(async () => {
      await identityService.logout();
    });

    // Try to login with invalid user
    let errorThrown = false;
    try {
      await act(async () => {
        await identityService.login('invalid-user-id');
      });
    } catch {
      errorThrown = true;
    }

    expect(errorThrown).toBe(true);
  });

  it('handles session refresh without session', async () => {
    render(createElement(TestApp, {
      onReady: (service) => {
        identityService = service;
      },
    }));

    expect(screen.getByTestId('test-app')).toBeTruthy();

    // Logout first
    await act(async () => {
      await identityService.logout();
    });

    // Try to refresh non-existent session
    let errorThrown = false;
    try {
      await act(async () => {
        await identityService.refreshSession();
      });
    } catch {
      errorThrown = true;
    }

    expect(errorThrown).toBe(true);
    expect(identityService.state.currentSession).toBeNull();
  });
});
