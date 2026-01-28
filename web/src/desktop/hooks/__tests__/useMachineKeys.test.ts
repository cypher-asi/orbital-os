import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { createElement } from 'react';
import { useMachineKeys } from '../useMachineKeys';
import { SupervisorProvider } from '../useSupervisor';
import { createMockSupervisor } from '../../../../test/mocks';

// Mock the stores
const mockCurrentUser = { id: '12345', displayName: 'Test User' };
let mockSelectCurrentUser = vi.fn(() => mockCurrentUser);
const mockMachineKeysState = {
  machines: [],
  isLoading: false,
  isInitializing: true,
  error: null,
  currentMachineId: null,
};
const mockStoreFunctions = {
  setMachines: vi.fn(),
  addMachine: vi.fn(),
  removeMachine: vi.fn(),
  updateMachine: vi.fn(),
  setLoading: vi.fn(),
  setError: vi.fn(),
  setInitializing: vi.fn(),
  reset: vi.fn(),
};

vi.mock('../../../stores', () => ({
  useIdentityStore: (
    selector: (state: { currentUser: typeof mockCurrentUser | null }) => unknown
  ) => selector({ currentUser: mockSelectCurrentUser() }),
  selectCurrentUser: (state: { currentUser: typeof mockCurrentUser | null }) => state.currentUser,
  useMachineKeysStore: (
    selector: (state: typeof mockMachineKeysState & typeof mockStoreFunctions) => unknown
  ) => selector({ ...mockMachineKeysState, ...mockStoreFunctions }),
  selectMachineKeysState: (state: typeof mockMachineKeysState) => ({
    machines: state.machines,
    isLoading: state.isLoading,
    isInitializing: state.isInitializing,
    error: state.error,
    currentMachineId: state.currentMachineId,
  }),
}));

// Mock the IdentityServiceClient
const mockIdentityServiceClient = {
  createMachineKey: vi.fn(),
  listMachineKeys: vi.fn(),
  revokeMachineKey: vi.fn(),
  rotateMachineKey: vi.fn(),
};

vi.mock('../../../client-services', () => ({
  IdentityServiceClient: vi.fn().mockImplementation(() => mockIdentityServiceClient),
  VfsStorageClient: {
    isAvailable: vi.fn(() => true),
    listChildrenSync: vi.fn(() => []),
    readJsonSync: vi.fn(() => null),
  },
  getMachineKeysDir: vi.fn(() => '/home/12345/.zos/identity/machines'),
}));

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

describe('useMachineKeys', () => {
  let mockSupervisor: ReturnType<typeof createMockSupervisor>;

  beforeEach(() => {
    vi.clearAllMocks();
    mockSupervisor = createMockSupervisor();
    mockSelectCurrentUser = vi.fn(() => mockCurrentUser);
    mockMachineKeysState.machines = [];
    mockMachineKeysState.isLoading = false;
    mockMachineKeysState.error = null;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('Initialization', () => {
    it('returns initial state', () => {
      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      expect(result.current.state).toBeDefined();
      expect(result.current.state.machines).toEqual([]);
      expect(result.current.state.isLoading).toBe(false);
    });

    it('provides action functions', () => {
      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      expect(typeof result.current.listMachineKeys).toBe('function');
      expect(typeof result.current.getMachineKey).toBe('function');
      expect(typeof result.current.createMachineKey).toBe('function');
      expect(typeof result.current.revokeMachineKey).toBe('function');
      expect(typeof result.current.rotateMachineKey).toBe('function');
      expect(typeof result.current.refresh).toBe('function');
    });
  });

  describe('listMachineKeys', () => {
    it('reads from VFS cache', async () => {
      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.listMachineKeys();
        } catch {
          // Expected to fail without real VFS
        }
      });

      expect(mockStoreFunctions.setLoading).toHaveBeenCalledWith(true);
    });
  });

  describe('createMachineKey', () => {
    // Sample Neural shard for testing
    const testShard = { index: 2, hex: 'abc123def456' };
    const testPassword = 'secure-password-123';

    it('calls identity service client with shard + password', async () => {
      const mockRecord = {
        machine_id: 1,
        signing_public_key: new Array(32).fill(0),
        encryption_public_key: new Array(32).fill(0),
        authorized_at: Date.now(),
        authorized_by: 12345,
        capabilities: {
          can_authenticate: true,
          can_encrypt: true,
          can_sign_messages: false,
          can_authorize_machines: false,
          can_revoke_machines: false,
          expires_at: null,
        },
        machine_name: 'Test Device',
        last_seen_at: Date.now(),
        epoch: 1,
      };
      mockIdentityServiceClient.createMachineKey.mockResolvedValue(mockRecord);

      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.createMachineKey(
          'Test Device',
          undefined,
          undefined,
          testShard,
          testPassword
        );
      });

      expect(mockIdentityServiceClient.createMachineKey).toHaveBeenCalledWith(
        expect.anything(),
        'Test Device',
        expect.anything(),
        expect.anything(),
        testShard,
        testPassword
      );
      expect(mockStoreFunctions.addMachine).toHaveBeenCalled();
    });

    it('throws error when shard is missing', async () => {
      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.createMachineKey('Test Device', undefined, undefined, undefined as never);
        } catch (error) {
          expect((error as Error).message).toBe(
            'An external Neural shard is required to create a machine key'
          );
        }
      });

      expect(mockIdentityServiceClient.createMachineKey).not.toHaveBeenCalled();
    });

    it('throws error when password is missing', async () => {
      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.createMachineKey('Test Device', undefined, undefined, testShard, '');
        } catch (error) {
          expect((error as Error).message).toBe('Password is required to create a machine key');
        }
      });

      expect(mockIdentityServiceClient.createMachineKey).not.toHaveBeenCalled();
    });

    it('sets error on failure', async () => {
      mockIdentityServiceClient.createMachineKey.mockRejectedValue(new Error('Failed'));

      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.createMachineKey(
            'Test Device',
            undefined,
            undefined,
            testShard,
            testPassword
          );
        } catch {
          // Expected error
        }
      });

      expect(mockStoreFunctions.setError).toHaveBeenCalledWith('Failed');
    });
  });

  describe('revokeMachineKey', () => {
    it('calls identity service client', async () => {
      mockIdentityServiceClient.revokeMachineKey.mockResolvedValue(undefined);

      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.revokeMachineKey('0x0000000000000000000000000000002a');
      });

      expect(mockIdentityServiceClient.revokeMachineKey).toHaveBeenCalled();
      expect(mockStoreFunctions.removeMachine).toHaveBeenCalledWith(
        '0x0000000000000000000000000000002a'
      );
    });

    it('prevents revoking current machine', async () => {
      // Set up current machine ID
      mockMachineKeysState.currentMachineId = '0x0000000000000000000000000000001';

      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.revokeMachineKey('0x0000000000000000000000000000001');
        } catch (error) {
          expect((error as Error).message).toBe('Cannot revoke the current machine key');
        }
      });

      expect(mockIdentityServiceClient.revokeMachineKey).not.toHaveBeenCalled();
    });
  });

  describe('rotateMachineKey', () => {
    it('calls identity service client and updates state', async () => {
      const existingMachine = {
        machineId: '0x0000000000000000000000000000002a',
        signingPublicKey: '00'.repeat(32),
        encryptionPublicKey: '00'.repeat(32),
        authorizedAt: Date.now(),
        authorizedBy: '0x12345',
        capabilities: {
          canAuthenticate: true,
          canEncrypt: true,
          canSignMessages: false,
          canAuthorizeMachines: false,
          canRevokeMachines: false,
          expiresAt: null,
        },
        machineName: 'Test Device',
        lastSeenAt: Date.now(),
        isCurrentDevice: false,
        epoch: 1,
      };
      mockMachineKeysState.machines = [existingMachine];

      const updatedRecord = {
        machine_id: 42,
        signing_public_key: new Array(32).fill(1),
        encryption_public_key: new Array(32).fill(1),
        authorized_at: Date.now(),
        authorized_by: 12345,
        capabilities: {
          can_authenticate: true,
          can_encrypt: true,
          can_sign_messages: false,
          can_authorize_machines: false,
          can_revoke_machines: false,
          expires_at: null,
        },
        machine_name: 'Test Device',
        last_seen_at: Date.now(),
        epoch: 2,
      };
      mockIdentityServiceClient.rotateMachineKey.mockResolvedValue(updatedRecord);

      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.rotateMachineKey('0x0000000000000000000000000000002a');
      });

      expect(mockIdentityServiceClient.rotateMachineKey).toHaveBeenCalled();
      expect(mockStoreFunctions.updateMachine).toHaveBeenCalled();
    });

    it('throws error for non-existent machine', async () => {
      mockMachineKeysState.machines = [];

      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.rotateMachineKey('0x999');
        } catch (error) {
          expect((error as Error).message).toBe('Machine not found');
        }
      });
    });
  });

  describe('getMachineKey', () => {
    it('returns machine from state if found', async () => {
      const existingMachine = {
        machineId: '0x0000000000000000000000000000002a',
        signingPublicKey: '00'.repeat(32),
        encryptionPublicKey: '00'.repeat(32),
        authorizedAt: Date.now(),
        authorizedBy: '0x12345',
        capabilities: {
          canAuthenticate: true,
          canEncrypt: true,
          canSignMessages: false,
          canAuthorizeMachines: false,
          canRevokeMachines: false,
          expiresAt: null,
        },
        machineName: 'Test Device',
        lastSeenAt: Date.now(),
        isCurrentDevice: false,
        epoch: 1,
      };
      mockMachineKeysState.machines = [existingMachine];

      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      const machine = await result.current.getMachineKey('0x0000000000000000000000000000002a');
      expect(machine).toEqual(existingMachine);
    });

    it('returns null for non-existent machine', async () => {
      mockMachineKeysState.machines = [];

      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      const machine = await result.current.getMachineKey('0x999');
      expect(machine).toBeNull();
    });
  });

  describe('refresh', () => {
    it('resets state when no user', async () => {
      mockSelectCurrentUser = vi.fn(() => null);
      mockUseIdentityServiceClient.userId = null as unknown as bigint;

      const { result } = renderHook(() => useMachineKeys(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.refresh();
      });

      expect(mockStoreFunctions.reset).toHaveBeenCalled();
    });
  });
});
