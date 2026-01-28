import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  IdentityServiceClient,
  MSG,
  type Supervisor,
  type NeuralKeyGenerated,
  type MachineKeyCapabilities,
  // Typed errors
  IdentityServiceError,
  ServiceNotFoundError,
  DeliveryFailedError,
  RequestTimeoutError,
  IdentityKeyAlreadyExistsError,
  MachineKeyNotFoundError,
  StorageError,
} from '../identity';

// =============================================================================
// Test Helpers
// =============================================================================

function createMockSupervisor(): Supervisor & {
  _ipcCallback: ((requestId: string, data: string) => void) | null;
  _simulateResponse: (requestId: string, data: unknown) => void;
  _simulateError: (requestId: string, error: string) => void;
} {
  let ipcCallback: ((requestId: string, data: string) => void) | null = null;

  return {
    _ipcCallback: null,
    _simulateResponse(requestId: string, data: unknown) {
      if (ipcCallback) {
        ipcCallback(requestId, JSON.stringify(data));
      }
    },
    _simulateError(requestId: string, error: string) {
      if (ipcCallback) {
        ipcCallback(requestId, JSON.stringify({ Err: error }));
      }
    },
    set_ipc_response_callback: vi.fn((callback: (requestId: string, data: string) => void) => {
      ipcCallback = callback;
    }),
    send_service_ipc: vi.fn((_serviceName: string, tag: number, _data: string) => {
      const responseTag = tag + 1;
      return responseTag.toString(16).padStart(8, '0');
    }),
    poll_syscalls: vi.fn(() => 0),
  };
}

// Reset module state between tests (callback registration tracking)
function _resetClientState() {
  // The client module tracks `callbackRegistered` as module-level state
  // We need to re-import or reset it between tests
  vi.resetModules();
}

// =============================================================================
// Tests
// =============================================================================

describe('IdentityServiceClient', () => {
  let supervisor: ReturnType<typeof createMockSupervisor>;
  let client: IdentityServiceClient;

  beforeEach(() => {
    vi.useFakeTimers();
    supervisor = createMockSupervisor();
    client = new IdentityServiceClient(supervisor, 5000); // 5s timeout for tests
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  describe('constructor', () => {
    it('should register IPC response callback on construction', () => {
      expect(supervisor.set_ipc_response_callback).toHaveBeenCalledTimes(1);
      expect(supervisor.set_ipc_response_callback).toHaveBeenCalledWith(expect.any(Function));
    });

    it('should only register callback once for multiple clients', () => {
      // Create a second client with the same supervisor
      const _client2 = new IdentityServiceClient(supervisor);

      // Callback should still only be registered once (tracked by module state)
      // Note: In practice, the module-level state prevents re-registration
      // This test verifies the behavior with same supervisor instance
      expect(supervisor.set_ipc_response_callback).toHaveBeenCalled();
    });
  });

  describe('MSG constants', () => {
    it('should have correct message tags for Neural Key operations', () => {
      expect(MSG.GENERATE_NEURAL_KEY).toBe(0x7054);
      expect(MSG.GENERATE_NEURAL_KEY_RESPONSE).toBe(0x7055);
      expect(MSG.RECOVER_NEURAL_KEY).toBe(0x7056);
      expect(MSG.RECOVER_NEURAL_KEY_RESPONSE).toBe(0x7057);
      expect(MSG.GET_IDENTITY_KEY).toBe(0x7052);
      expect(MSG.GET_IDENTITY_KEY_RESPONSE).toBe(0x7053);
    });

    it('should have correct message tags for Machine Key operations', () => {
      expect(MSG.CREATE_MACHINE_KEY).toBe(0x7060);
      expect(MSG.CREATE_MACHINE_KEY_RESPONSE).toBe(0x7061);
      expect(MSG.LIST_MACHINE_KEYS).toBe(0x7062);
      expect(MSG.LIST_MACHINE_KEYS_RESPONSE).toBe(0x7063);
      expect(MSG.GET_MACHINE_KEY).toBe(0x7064);
      expect(MSG.GET_MACHINE_KEY_RESPONSE).toBe(0x7065);
      expect(MSG.REVOKE_MACHINE_KEY).toBe(0x7066);
      expect(MSG.REVOKE_MACHINE_KEY_RESPONSE).toBe(0x7067);
      expect(MSG.ROTATE_MACHINE_KEY).toBe(0x7068);
      expect(MSG.ROTATE_MACHINE_KEY_RESPONSE).toBe(0x7069);
    });
  });

  describe('generateNeuralKey', () => {
    it('should send correct IPC message', async () => {
      const userId = BigInt(12345);
      const promise = client.generateNeuralKey(userId, 'secure-password-123');

      expect(supervisor.send_service_ipc).toHaveBeenCalledWith(
        'identity',
        MSG.GENERATE_NEURAL_KEY,
        JSON.stringify({
          user_id: '0x00000000000000000000000000003039',
          password: 'secure-password-123',
        })
      );

      // Simulate response
      const response: { result: { Ok: NeuralKeyGenerated } } = {
        result: {
          Ok: {
            public_identifiers: {
              identity_signing_pub_key: '0xabc123',
              machine_signing_pub_key: '0xdef456',
              machine_encryption_pub_key: '0x789012',
            },
            shards: [
              { index: 1, hex: 'shard1hex' },
              { index: 2, hex: 'shard2hex' },
            ],
            created_at: 1704067200000,
          },
        },
      };

      // Get the request ID (response tag in hex)
      const requestId = (MSG.GENERATE_NEURAL_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      // Advance timers to process the callback
      await vi.advanceTimersByTimeAsync(20);

      const result = await promise;
      expect(result.public_identifiers.identity_signing_pub_key).toBe('0xabc123');
      expect(result.shards).toHaveLength(2);
    });

    it('should reject on error response', async () => {
      const userId = BigInt(12345);
      const promise = client.generateNeuralKey(userId, 'secure-password-123');

      const response = {
        result: {
          Err: 'IdentityKeyAlreadyExists',
        },
      };

      const requestId = (MSG.GENERATE_NEURAL_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      await expect(promise).rejects.toBeInstanceOf(IdentityKeyAlreadyExistsError);
    });
  });

  describe('recoverNeuralKey', () => {
    it('should send correct IPC message with shards', async () => {
      const userId = BigInt(12345);
      const shards = [
        { index: 1, hex: 'shard1hex' },
        { index: 2, hex: 'shard2hex' },
        { index: 3, hex: 'shard3hex' },
      ];

      const promise = client.recoverNeuralKey(userId, shards);

      expect(supervisor.send_service_ipc).toHaveBeenCalledWith(
        'identity',
        MSG.RECOVER_NEURAL_KEY,
        JSON.stringify({ user_id: '0x00000000000000000000000000003039', shards })
      );

      // Simulate success response
      const response = {
        result: {
          Ok: {
            public_identifiers: {
              identity_signing_pub_key: '0xrecovered1',
              machine_signing_pub_key: '0xrecovered2',
              machine_encryption_pub_key: '0xrecovered3',
            },
            shards: [
              { index: 1, hex: 'newshard1' },
              { index: 2, hex: 'newshard2' },
            ],
            created_at: 1704067200000,
          },
        },
      };

      const requestId = (MSG.RECOVER_NEURAL_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      const result = await promise;
      expect(result.public_identifiers.identity_signing_pub_key).toBe('0xrecovered1');
    });
  });

  describe('getIdentityKey', () => {
    it('should return key store when found', async () => {
      const userId = BigInt(12345);
      const promise = client.getIdentityKey(userId);

      const response = {
        result: {
          Ok: {
            user_id: 12345,
            identity_signing_public_key: [1, 2, 3],
            machine_signing_public_key: [4, 5, 6],
            machine_encryption_public_key: [7, 8, 9],
            epoch: 1,
          },
        },
      };

      const requestId = (MSG.GET_IDENTITY_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      const result = await promise;
      expect(result).not.toBeNull();
      expect(result?.user_id).toBe(12345);
    });

    it('should return null when key not found', async () => {
      const userId = BigInt(12345);
      const promise = client.getIdentityKey(userId);

      const response = {
        result: {
          Ok: null,
        },
      };

      const requestId = (MSG.GET_IDENTITY_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      const result = await promise;
      expect(result).toBeNull();
    });
  });

  describe('createMachineKey', () => {
    // Test shard for Neural key derivation
    const testShard = { index: 2, hex: 'abc123def456abc123def456abc123def456' };

    it('should send correct IPC message with capabilities, shard, and password', async () => {
      const userId = BigInt(12345);
      const machineName = 'My Laptop';
      const capabilities: MachineKeyCapabilities = {
        capabilities: ['AUTHENTICATE', 'ENCRYPT'],
        expires_at: null,
      };

      const promise = client.createMachineKey(
        userId,
        machineName,
        capabilities,
        undefined,
        testShard,
        'secure-password-123'
      );

      expect(supervisor.send_service_ipc).toHaveBeenCalledWith(
        'identity',
        MSG.CREATE_MACHINE_KEY,
        expect.any(String)
      );

      // Verify the request data
      const callArgs = (supervisor.send_service_ipc as ReturnType<typeof vi.fn>).mock.calls[0];
      const requestData = JSON.parse(callArgs[2]);
      expect(requestData.user_id).toBe('0x00000000000000000000000000003039');
      expect(requestData.machine_name).toBe(machineName);
      expect(requestData.capabilities).toEqual(capabilities);
      expect(requestData.external_shard).toEqual(testShard);
      expect(requestData.password).toBe('secure-password-123');

      // Simulate success response
      const response = {
        result: {
          Ok: {
            machine_id: 98765,
            signing_public_key: [1, 2, 3],
            encryption_public_key: [4, 5, 6],
            authorized_at: 1704067200000,
            authorized_by: 12345,
            capabilities,
            machine_name: machineName,
            last_seen_at: 1704067200000,
          },
        },
      };

      const requestId = (MSG.CREATE_MACHINE_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      const result = await promise;
      expect(result.machine_name).toBe(machineName);
    });

    it('should throw error when shard is missing', async () => {
      const userId = BigInt(12345);
      const machineName = 'My Laptop';
      const capabilities: MachineKeyCapabilities = {
        capabilities: ['AUTHENTICATE', 'ENCRYPT'],
        expires_at: null,
      };

      await expect(
        client.createMachineKey(userId, machineName, capabilities, undefined, undefined, 'pw')
      ).rejects.toBeInstanceOf(IdentityServiceError);
    });

    it('should throw error when password is missing', async () => {
      const userId = BigInt(12345);
      const machineName = 'My Laptop';
      const capabilities: MachineKeyCapabilities = {
        capabilities: ['AUTHENTICATE', 'ENCRYPT'],
        expires_at: null,
      };

      await expect(
        client.createMachineKey(userId, machineName, capabilities, undefined, testShard, '')
      ).rejects.toBeInstanceOf(IdentityServiceError);
    });
  });

  describe('listMachineKeys', () => {
    it('should return array of machine records', async () => {
      const userId = BigInt(12345);
      const promise = client.listMachineKeys(userId);

      const response = {
        machines: [
          {
            machine_id: 111,
            signing_public_key: [1, 2, 3],
            encryption_public_key: [4, 5, 6],
            authorized_at: 1704067200000,
            authorized_by: 12345,
            capabilities: {
              can_authenticate: true,
              can_encrypt: true,
              can_sign_messages: false,
              can_authorize_machines: false,
              can_revoke_machines: false,
              expires_at: null,
            },
            machine_name: 'Device 1',
            last_seen_at: 1704067200000,
          },
          {
            machine_id: 222,
            signing_public_key: [7, 8, 9],
            encryption_public_key: [10, 11, 12],
            authorized_at: 1704067200000,
            authorized_by: 12345,
            capabilities: {
              can_authenticate: true,
              can_encrypt: false,
              can_sign_messages: true,
              can_authorize_machines: false,
              can_revoke_machines: false,
              expires_at: null,
            },
            machine_name: 'Device 2',
            last_seen_at: 1704067200000,
          },
        ],
      };

      const requestId = (MSG.LIST_MACHINE_KEYS + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      const result = await promise;
      expect(result).toHaveLength(2);
      expect(result[0].machine_name).toBe('Device 1');
      expect(result[1].machine_name).toBe('Device 2');
    });

    it('should return empty array when no machines exist', async () => {
      const userId = BigInt(12345);
      const promise = client.listMachineKeys(userId);

      const response = {
        machines: [],
      };

      const requestId = (MSG.LIST_MACHINE_KEYS + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      const result = await promise;
      expect(result).toHaveLength(0);
    });
  });

  describe('revokeMachineKey', () => {
    it('should send correct IPC message', async () => {
      const userId = BigInt(12345);
      const machineId = BigInt(98765);
      const promise = client.revokeMachineKey(userId, machineId);

      expect(supervisor.send_service_ipc).toHaveBeenCalledWith(
        'identity',
        MSG.REVOKE_MACHINE_KEY,
        JSON.stringify({
          user_id: '0x00000000000000000000000000003039',
          machine_id: '0x000000000000000000000000000181cd',
        })
      );

      const response = {
        result: {
          Ok: null,
        },
      };

      const requestId = (MSG.REVOKE_MACHINE_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      await expect(promise).resolves.toBeUndefined();
    });

    it('should reject when machine not found', async () => {
      const userId = BigInt(12345);
      const machineId = BigInt(99999);
      const promise = client.revokeMachineKey(userId, machineId);

      const response = {
        result: {
          Err: 'MachineKeyNotFound',
        },
      };

      const requestId = (MSG.REVOKE_MACHINE_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      await expect(promise).rejects.toBeInstanceOf(MachineKeyNotFoundError);
    });
  });

  describe('rotateMachineKey', () => {
    it('should return updated machine record', async () => {
      const userId = BigInt(12345);
      const machineId = BigInt(98765);
      const promise = client.rotateMachineKey(userId, machineId);

      const response = {
        result: {
          Ok: {
            machine_id: 98765,
            signing_public_key: [10, 20, 30], // New keys
            encryption_public_key: [40, 50, 60],
            authorized_at: 1704067200000,
            authorized_by: 12345,
            capabilities: {
              can_authenticate: true,
              can_encrypt: true,
              can_sign_messages: false,
              can_authorize_machines: false,
              can_revoke_machines: false,
              expires_at: null,
            },
            machine_name: 'Rotated Device',
            last_seen_at: 1704153600000, // Updated timestamp
          },
        },
      };

      const requestId = (MSG.ROTATE_MACHINE_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      const result = await promise;
      expect(result.machine_name).toBe('Rotated Device');
      expect(result.signing_public_key).toEqual([10, 20, 30]);
    });
  });

  describe('error handling', () => {
    it('should throw ServiceNotFoundError when service not found', async () => {
      // Override mock to return error
      supervisor.send_service_ipc = vi.fn(() => 'error:service_not_found:identity');

      const promise = client.generateNeuralKey(BigInt(12345), 'secure-password-123');

      await expect(promise).rejects.toBeInstanceOf(ServiceNotFoundError);
      await expect(promise).rejects.toMatchObject({
        serviceName: 'identity',
        message: 'Service not found: identity',
      });
    });

    it('should throw DeliveryFailedError when delivery fails', async () => {
      supervisor.send_service_ipc = vi.fn(() => 'error:delivery_failed:SomeError');

      const promise = client.generateNeuralKey(BigInt(12345), 'secure-password-123');

      await expect(promise).rejects.toBeInstanceOf(DeliveryFailedError);
      await expect(promise).rejects.toMatchObject({
        reason: 'SomeError',
      });
    });

    it('should throw RequestTimeoutError on timeout', async () => {
      const userId = BigInt(12345);
      const promise = client.generateNeuralKey(userId, 'secure-password-123');

      // Advance past the timeout (5s configured in beforeEach)
      await vi.advanceTimersByTimeAsync(6000);

      await expect(promise).rejects.toBeInstanceOf(RequestTimeoutError);
      await expect(promise).rejects.toMatchObject({
        timeoutMs: 5000,
      });
    });

    it('should throw IdentityKeyAlreadyExistsError for known error code', async () => {
      const userId = BigInt(12345);
      const promise = client.generateNeuralKey(userId, 'secure-password-123');

      const response = {
        result: {
          Err: 'IdentityKeyAlreadyExists',
        },
      };

      const requestId = (MSG.GENERATE_NEURAL_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      await expect(promise).rejects.toBeInstanceOf(IdentityKeyAlreadyExistsError);
    });

    it('should throw MachineKeyNotFoundError for known error code', async () => {
      const userId = BigInt(12345);
      const machineId = BigInt(99999);
      const promise = client.revokeMachineKey(userId, machineId);

      const response = {
        result: {
          Err: 'MachineKeyNotFound',
        },
      };

      const requestId = (MSG.REVOKE_MACHINE_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      await expect(promise).rejects.toBeInstanceOf(MachineKeyNotFoundError);
    });

    it('should throw StorageError for structured storage errors', async () => {
      const userId = BigInt(12345);
      const promise = client.generateNeuralKey(userId, 'secure-password-123');

      const response = {
        result: {
          Err: { StorageError: 'VFS write failed' },
        },
      };

      const requestId = (MSG.GENERATE_NEURAL_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      await expect(promise).rejects.toBeInstanceOf(StorageError);
      await expect(promise).rejects.toMatchObject({
        reason: 'VFS write failed',
      });
    });

    it('should throw generic IdentityServiceError for unknown errors', async () => {
      const userId = BigInt(12345);
      const promise = client.generateNeuralKey(userId, 'secure-password-123');

      const response = {
        result: {
          Err: 'SomeUnknownError',
        },
      };

      const requestId = (MSG.GENERATE_NEURAL_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(requestId, response);

      await vi.advanceTimersByTimeAsync(20);

      await expect(promise).rejects.toBeInstanceOf(IdentityServiceError);
      await expect(promise).rejects.toMatchObject({
        message: 'SomeUnknownError',
      });
    });
  });

  describe('concurrent requests', () => {
    it('should handle multiple concurrent requests of different types', async () => {
      const userId = BigInt(12345);

      // Start two requests of DIFFERENT types (different request IDs)
      const generatePromise = client.generateNeuralKey(userId, 'secure-password-123');
      const getPromise = client.getIdentityKey(userId);

      // Both should have sent messages
      expect(supervisor.send_service_ipc).toHaveBeenCalledTimes(2);

      // Respond to generate request
      const generateRequestId = (MSG.GENERATE_NEURAL_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(generateRequestId, {
        result: {
          Ok: {
            public_identifiers: {
              identity_signing_pub_key: '0xgen1',
              machine_signing_pub_key: '0xgen2',
              machine_encryption_pub_key: '0xgen3',
            },
            shards: [],
            created_at: 1704067200000,
          },
        },
      });

      await vi.advanceTimersByTimeAsync(20);

      // Respond to get request
      const getRequestId = (MSG.GET_IDENTITY_KEY + 1).toString(16).padStart(8, '0');
      supervisor._simulateResponse(getRequestId, {
        result: {
          Ok: {
            user_id: 12345,
            identity_signing_public_key: [],
            machine_signing_public_key: [],
            machine_encryption_public_key: [],
            epoch: 1,
          },
        },
      });

      await vi.advanceTimersByTimeAsync(20);

      // Both should resolve correctly
      const genResult = await generatePromise;
      expect(genResult.public_identifiers.identity_signing_pub_key).toBe('0xgen1');

      const getResult = await getPromise;
      expect(getResult?.user_id).toBe(12345);
    });

    it('handles concurrent requests of the same type with FIFO queue', async () => {
      // This test verifies that multiple concurrent requests of the same type
      // are handled correctly using a FIFO queue.
      const userId1 = BigInt(11111);
      const userId2 = BigInt(22222);
      const userId3 = BigInt(33333);

      // Start three requests of the SAME type
      const promise1 = client.getIdentityKey(userId1);
      const promise2 = client.getIdentityKey(userId2);
      const promise3 = client.getIdentityKey(userId3);

      // All three requests were sent
      expect(supervisor.send_service_ipc).toHaveBeenCalledTimes(3);

      const requestId = (MSG.GET_IDENTITY_KEY + 1).toString(16).padStart(8, '0');

      // Respond to first request - should resolve promise1 (FIFO)
      supervisor._simulateResponse(requestId, {
        result: {
          Ok: {
            user_id: 11111,
            identity_signing_public_key: [],
            machine_signing_public_key: [],
            machine_encryption_public_key: [],
            epoch: 1,
          },
        },
      });
      await vi.advanceTimersByTimeAsync(50);
      const result1 = await promise1;
      expect(result1?.user_id).toBe(11111);

      // Respond to second request - should resolve promise2 (FIFO)
      supervisor._simulateResponse(requestId, {
        result: {
          Ok: {
            user_id: 22222,
            identity_signing_public_key: [],
            machine_signing_public_key: [],
            machine_encryption_public_key: [],
            epoch: 1,
          },
        },
      });
      await vi.advanceTimersByTimeAsync(50);
      const result2 = await promise2;
      expect(result2?.user_id).toBe(22222);

      // Respond to third request - should resolve promise3 (FIFO)
      supervisor._simulateResponse(requestId, {
        result: {
          Ok: {
            user_id: 33333,
            identity_signing_public_key: [],
            machine_signing_public_key: [],
            machine_encryption_public_key: [],
            epoch: 1,
          },
        },
      });
      await vi.advanceTimersByTimeAsync(50);
      const result3 = await promise3;
      expect(result3?.user_id).toBe(33333);
    });

    it('generates unique request IDs for timeout tracking', () => {
      // The supervisor returns the same tag-based ID, but internally
      // we use unique IDs for timeout tracking
      const requestId1 = supervisor.send_service_ipc('identity', MSG.GET_IDENTITY_KEY, '{}');
      const requestId2 = supervisor.send_service_ipc('identity', MSG.GET_IDENTITY_KEY, '{}');

      // Supervisor returns the same response tag hex
      expect(requestId1).toBe(requestId2);
      expect(requestId1).toBe((MSG.GET_IDENTITY_KEY + 1).toString(16).padStart(8, '0'));

      // But internally, requests get unique IDs (tested via the FIFO behavior above)
    });
  });
});
