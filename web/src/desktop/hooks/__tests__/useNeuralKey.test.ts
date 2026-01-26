import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { createElement } from 'react';
import { useNeuralKey } from '../useNeuralKey';
import { SupervisorProvider } from '../useSupervisor';
import { createMockSupervisor } from '../../../../test/mocks';

// Mock the stores
const mockCurrentUser = { id: '12345', displayName: 'Test User' };
let mockSelectCurrentUser = vi.fn(() => mockCurrentUser);

vi.mock('../../../stores', () => ({
  useIdentityStore: (
    selector: (state: { currentUser: typeof mockCurrentUser | null }) => unknown
  ) => selector({ currentUser: mockSelectCurrentUser() }),
  selectCurrentUser: (state: { currentUser: typeof mockCurrentUser | null }) => state.currentUser,
}));

// Create mock functions that will be populated in the factory
let mockGenerateNeuralKey = vi.fn();
let mockRecoverNeuralKey = vi.fn();
const mockGetIdentityKey = vi.fn();
let mockKeystoreIsAvailable = vi.fn(() => true);
let mockKeystoreReadJsonSync = vi.fn(() => null);
const mockKeystoreGetCacheStats = vi.fn(() => ({ keys: 0 }));

vi.mock('../../../client-services', () => ({
  IdentityServiceClient: vi.fn().mockImplementation(() => ({
    generateNeuralKey: (...args: unknown[]) => mockGenerateNeuralKey(...args),
    recoverNeuralKey: (...args: unknown[]) => mockRecoverNeuralKey(...args),
    getIdentityKey: (...args: unknown[]) => mockGetIdentityKey(...args),
  })),
  KeystoreClient: {
    isAvailable: () => mockKeystoreIsAvailable(),
    readJsonSync: (...args: unknown[]) => mockKeystoreReadJsonSync(...args),
    getCacheStats: () => mockKeystoreGetCacheStats(),
  },
  getIdentityKeystorePath: vi.fn(() => '/keys/12345/identity/public_keys.json'),
  bytesToHex: vi.fn((bytes: number[]) => bytes.map(b => b.toString(16).padStart(2, '0')).join('')),
}));

// Mock identity service client shorthand
const mockIdentityServiceClient = {
  get generateNeuralKey() {
    return mockGenerateNeuralKey;
  },
  get recoverNeuralKey() {
    return mockRecoverNeuralKey;
  },
  get getIdentityKey() {
    return mockGetIdentityKey;
  },
};

// Mock Keystore storage shorthand
const mockKeystoreStorage = {
  get isAvailable() {
    return mockKeystoreIsAvailable;
  },
  get readJsonSync() {
    return mockKeystoreReadJsonSync;
  },
  get getCacheStats() {
    return mockKeystoreGetCacheStats;
  },
};

// Mock useIdentityServiceClient
const mockUseIdentityServiceClient = {
  userId: BigInt(12345),
  getClientOrThrow: vi.fn(() => mockIdentityServiceClient),
  getUserIdOrThrow: vi.fn(() => BigInt(12345)),
};

vi.mock('../useIdentityServiceClient', () => ({
  useIdentityServiceClient: () => mockUseIdentityServiceClient,
}));

function createWrapper(supervisor: ReturnType<typeof createMockSupervisor>) {
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(SupervisorProvider, { value: supervisor }, children);
  };
}

describe('useNeuralKey', () => {
  let mockSupervisor: ReturnType<typeof createMockSupervisor>;

  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    mockSupervisor = createMockSupervisor();
    mockSelectCurrentUser = vi.fn(() => mockCurrentUser);
    mockKeystoreIsAvailable = vi.fn(() => true);
    mockKeystoreReadJsonSync = vi.fn(() => null);
    mockGenerateNeuralKey = vi.fn();
    mockRecoverNeuralKey = vi.fn();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  describe('Initialization', () => {
    it('returns initial state', () => {
      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      expect(result.current.state).toBeDefined();
      expect(result.current.state.hasNeuralKey).toBe(false);
      expect(result.current.state.publicIdentifiers).toBeNull();
      expect(result.current.state.isLoading).toBe(true);
      expect(result.current.state.isInitializing).toBe(true);
    });

    it('provides action functions', () => {
      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      expect(typeof result.current.generateNeuralKey).toBe('function');
      expect(typeof result.current.recoverNeuralKey).toBe('function');
      expect(typeof result.current.confirmShardsSaved).toBe('function');
      expect(typeof result.current.refresh).toBe('function');
    });
  });

  describe('generateNeuralKey', () => {
    it('calls identity service client', async () => {
      const mockResponse = {
        public_identifiers: {
          identity_signing_pub_key: '0x' + '00'.repeat(32),
          machine_signing_pub_key: '0x' + '01'.repeat(32),
          machine_encryption_pub_key: '0x' + '02'.repeat(32),
        },
        shards: [
          { index: 1, hex: 'shard1hex' },
          { index: 2, hex: 'shard2hex' },
          { index: 3, hex: 'shard3hex' },
          { index: 4, hex: 'shard4hex' },
          { index: 5, hex: 'shard5hex' },
        ],
        created_at: Date.now(),
      };
      mockGenerateNeuralKey.mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        const response = await result.current.generateNeuralKey();
        expect(response.shards).toHaveLength(5);
        expect(response.publicIdentifiers.identitySigningPubKey).toBeDefined();
      });

      expect(mockGenerateNeuralKey).toHaveBeenCalledWith(BigInt(12345));
    });

    it('sets pending shards after generation', async () => {
      const mockResponse = {
        public_identifiers: {
          identity_signing_pub_key: '0x' + '00'.repeat(32),
          machine_signing_pub_key: '0x' + '01'.repeat(32),
          machine_encryption_pub_key: '0x' + '02'.repeat(32),
        },
        shards: [
          { index: 1, hex: 'shard1hex' },
          { index: 2, hex: 'shard2hex' },
          { index: 3, hex: 'shard3hex' },
          { index: 4, hex: 'shard4hex' },
          { index: 5, hex: 'shard5hex' },
        ],
        created_at: Date.now(),
      };
      mockGenerateNeuralKey.mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.generateNeuralKey();
      });

      expect(result.current.state.hasNeuralKey).toBe(true);
      expect(result.current.state.pendingShards).toHaveLength(5);
      expect(result.current.state.publicIdentifiers).toBeDefined();
    });

    it('sets error on failure', async () => {
      mockGenerateNeuralKey.mockRejectedValue(new Error('Generation failed'));

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.generateNeuralKey();
        } catch {
          // Expected error
        }
      });

      expect(result.current.state.error).toBe('Generation failed');
    });
  });

  describe('recoverNeuralKey', () => {
    it('calls identity service client with shards', async () => {
      const shards = [
        { index: 1, hex: 'shard1hex' },
        { index: 2, hex: 'shard2hex' },
        { index: 3, hex: 'shard3hex' },
      ];
      const mockResponse = {
        public_identifiers: {
          identity_signing_pub_key: '0x' + '00'.repeat(32),
          machine_signing_pub_key: '0x' + '01'.repeat(32),
          machine_encryption_pub_key: '0x' + '02'.repeat(32),
        },
        shards: [
          { index: 1, hex: 'newshard1' },
          { index: 2, hex: 'newshard2' },
          { index: 3, hex: 'newshard3' },
          { index: 4, hex: 'newshard4' },
          { index: 5, hex: 'newshard5' },
        ],
        created_at: Date.now(),
      };
      mockRecoverNeuralKey.mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        const response = await result.current.recoverNeuralKey(shards);
        expect(response.shards).toHaveLength(5);
      });

      expect(mockRecoverNeuralKey).toHaveBeenCalledWith(BigInt(12345), shards);
    });

    it('throws error for insufficient shards', async () => {
      const shards = [
        { index: 1, hex: 'shard1hex' },
        { index: 2, hex: 'shard2hex' },
      ];

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.recoverNeuralKey(shards);
        } catch (error) {
          expect((error as Error).message).toBe('At least 3 shards are required for recovery');
        }
      });

      expect(mockRecoverNeuralKey).not.toHaveBeenCalled();
    });

    it('sets error on failure', async () => {
      const shards = [
        { index: 1, hex: 'shard1hex' },
        { index: 2, hex: 'shard2hex' },
        { index: 3, hex: 'shard3hex' },
      ];
      mockRecoverNeuralKey.mockRejectedValue(new Error('Recovery failed'));

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.recoverNeuralKey(shards);
        } catch {
          // Expected error
        }
      });

      expect(result.current.state.error).toBe('Recovery failed');
    });
  });

  describe('confirmShardsSaved', () => {
    it('clears pending shards', async () => {
      const mockResponse = {
        public_identifiers: {
          identity_signing_pub_key: '0x' + '00'.repeat(32),
          machine_signing_pub_key: '0x' + '01'.repeat(32),
          machine_encryption_pub_key: '0x' + '02'.repeat(32),
        },
        shards: [
          { index: 1, hex: 'shard1hex' },
          { index: 2, hex: 'shard2hex' },
          { index: 3, hex: 'shard3hex' },
          { index: 4, hex: 'shard4hex' },
          { index: 5, hex: 'shard5hex' },
        ],
        created_at: Date.now(),
      };
      mockGenerateNeuralKey.mockResolvedValue(mockResponse);

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.generateNeuralKey();
      });

      expect(result.current.state.pendingShards).toHaveLength(5);

      act(() => {
        result.current.confirmShardsSaved();
      });

      expect(result.current.state.pendingShards).toBeNull();
    });
  });

  describe('refresh', () => {
    it('reads from Keystore cache when available', async () => {
      const keyStore = {
        user_id: 12345,
        identity_signing_public_key: new Array(32).fill(0),
        machine_signing_public_key: new Array(32).fill(1),
        machine_encryption_public_key: new Array(32).fill(2),
        epoch: 1,
        created_at: Date.now(),
      };
      mockKeystoreReadJsonSync.mockReturnValue(keyStore);

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      // Run timers to allow settle delay
      await act(async () => {
        vi.advanceTimersByTime(600);
      });

      expect(result.current.state.hasNeuralKey).toBe(true);
      expect(result.current.state.publicIdentifiers).toBeDefined();
    });

    it('handles Keystore not available', async () => {
      mockKeystoreIsAvailable.mockReturnValue(false);

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        vi.advanceTimersByTime(600);
      });

      expect(result.current.state.error).toBe('Keystore cache not ready');
    });

    it('resets state when no user', async () => {
      mockSelectCurrentUser = vi.fn(() => null);
      mockUseIdentityServiceClient.userId = null as unknown as bigint;

      const { result } = renderHook(() => useNeuralKey(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        vi.advanceTimersByTime(600);
        await result.current.refresh();
      });

      expect(result.current.state.hasNeuralKey).toBe(false);
      expect(result.current.state.isLoading).toBe(false);
    });
  });

  // Auto-refresh behavior is tested in the 'refresh' suite via the
  // 'reads from Keystore cache when available' test which validates the
  // full refresh flow works correctly.
});
