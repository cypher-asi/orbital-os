import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { createElement } from 'react';
import { useLinkedAccounts } from '../useLinkedAccounts';
import { SupervisorProvider } from '../useSupervisor';
import { createMockSupervisor } from '../../../test/mocks';

// Mock the stores
const mockCurrentUser = { id: '12345', displayName: 'Test User' };
let mockSelectCurrentUser = vi.fn(() => mockCurrentUser);

vi.mock('../../../stores', () => ({
  useIdentityStore: (
    selector: (state: { currentUser: typeof mockCurrentUser | null }) => unknown
  ) => selector({ currentUser: mockSelectCurrentUser() }),
  selectCurrentUser: (state: { currentUser: typeof mockCurrentUser | null }) => state.currentUser,
}));

// Mock the IdentityServiceClient
const mockIdentityServiceClient = {
  attachEmail: vi.fn(),
  verifyEmail: vi.fn(),
  getCredentials: vi.fn(),
  unlinkCredential: vi.fn(),
};

vi.mock('../../../services', () => ({
  IdentityServiceClient: vi.fn().mockImplementation(() => mockIdentityServiceClient),
  VfsStorageClient: {
    isAvailable: vi.fn(() => true),
    readJsonSync: vi.fn(() => null),
  },
  getCredentialsPath: vi.fn(() => '/home/12345/.zos/identity/credentials.json'),
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

describe('useLinkedAccounts', () => {
  let mockSupervisor: ReturnType<typeof createMockSupervisor>;

  beforeEach(() => {
    vi.clearAllMocks();
    mockSupervisor = createMockSupervisor();
    mockSelectCurrentUser = vi.fn(() => mockCurrentUser);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('Initialization', () => {
    it('returns initial state', () => {
      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      expect(result.current.state).toBeDefined();
      expect(result.current.state.credentials).toEqual([]);
      expect(result.current.state.isLoading).toBe(false);
      expect(result.current.state.pendingEmail).toBeNull();
      expect(result.current.state.error).toBeNull();
    });

    it('provides action functions', () => {
      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      expect(typeof result.current.attachEmail).toBe('function');
      expect(typeof result.current.verifyEmail).toBe('function');
      expect(typeof result.current.cancelEmailVerification).toBe('function');
      expect(typeof result.current.unlinkAccount).toBe('function');
      expect(typeof result.current.refresh).toBe('function');
    });
  });

  describe('attachEmail', () => {
    it('calls identity service client with email', async () => {
      mockIdentityServiceClient.attachEmail.mockResolvedValue({
        verification_required: true,
        verification_code: '123456',
      });

      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        const response = await result.current.attachEmail('test@example.com');
        expect(response.verificationRequired).toBe(true);
        expect(response.verificationCode).toBe('123456');
      });

      expect(mockIdentityServiceClient.attachEmail).toHaveBeenCalledWith(
        BigInt(12345),
        'test@example.com'
      );
    });

    it('sets pending email state', async () => {
      mockIdentityServiceClient.attachEmail.mockResolvedValue({
        verification_required: true,
        verification_code: '123456',
      });

      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.attachEmail('test@example.com');
      });

      expect(result.current.state.pendingEmail).toBe('test@example.com');
      expect(result.current.state.pendingVerificationCode).toBe('123456');
    });

    it('sets error on failure', async () => {
      mockIdentityServiceClient.attachEmail.mockRejectedValue(new Error('Failed to attach'));

      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.attachEmail('test@example.com');
        } catch {
          // Expected error
        }
      });

      expect(result.current.state.error).toBe('Failed to attach');
    });
  });

  describe('verifyEmail', () => {
    it('calls identity service client with code', async () => {
      mockIdentityServiceClient.attachEmail.mockResolvedValue({
        verification_required: true,
        verification_code: '123456',
      });
      mockIdentityServiceClient.verifyEmail.mockResolvedValue(undefined);

      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      // First attach the email
      await act(async () => {
        await result.current.attachEmail('test@example.com');
      });

      // Then verify
      await act(async () => {
        await result.current.verifyEmail('test@example.com', '123456');
      });

      expect(mockIdentityServiceClient.verifyEmail).toHaveBeenCalledWith(
        BigInt(12345),
        'test@example.com',
        '123456'
      );
    });

    it('clears pending email after verification', async () => {
      mockIdentityServiceClient.attachEmail.mockResolvedValue({
        verification_required: true,
        verification_code: '123456',
      });
      mockIdentityServiceClient.verifyEmail.mockResolvedValue(undefined);

      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.attachEmail('test@example.com');
      });

      await act(async () => {
        await result.current.verifyEmail('test@example.com', '123456');
      });

      expect(result.current.state.pendingEmail).toBeNull();
      expect(result.current.state.pendingVerificationCode).toBeNull();
    });

    it('throws error for invalid code format', async () => {
      mockIdentityServiceClient.attachEmail.mockResolvedValue({
        verification_required: true,
        verification_code: '123456',
      });

      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.attachEmail('test@example.com');
      });

      await act(async () => {
        try {
          await result.current.verifyEmail('test@example.com', 'invalid');
        } catch (error) {
          expect((error as Error).message).toBe('Invalid code format');
        }
      });

      expect(result.current.state.verificationError).toBe(
        'Invalid code format. Please enter 6 digits.'
      );
    });

    it('throws error when no pending verification', async () => {
      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.verifyEmail('test@example.com', '123456');
        } catch (error) {
          expect((error as Error).message).toBe('No pending verification for this email');
        }
      });
    });
  });

  describe('cancelEmailVerification', () => {
    it('clears pending email state', async () => {
      mockIdentityServiceClient.attachEmail.mockResolvedValue({
        verification_required: true,
        verification_code: '123456',
      });

      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.attachEmail('test@example.com');
      });

      expect(result.current.state.pendingEmail).toBe('test@example.com');

      act(() => {
        result.current.cancelEmailVerification();
      });

      expect(result.current.state.pendingEmail).toBeNull();
      expect(result.current.state.pendingVerificationCode).toBeNull();
    });
  });

  describe('unlinkAccount', () => {
    it('calls identity service client', async () => {
      mockIdentityServiceClient.unlinkCredential.mockResolvedValue(undefined);

      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.unlinkAccount('email');
      });

      expect(mockIdentityServiceClient.unlinkCredential).toHaveBeenCalledWith(
        BigInt(12345),
        'Email'
      );
    });

    it('sets error on failure', async () => {
      mockIdentityServiceClient.unlinkCredential.mockRejectedValue(new Error('Failed to unlink'));

      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        try {
          await result.current.unlinkAccount('email');
        } catch {
          // Expected error
        }
      });

      expect(result.current.state.error).toBe('Failed to unlink');
    });
  });

  describe('refresh', () => {
    it('reads credentials from VFS cache', async () => {
      const { result } = renderHook(() => useLinkedAccounts(), {
        wrapper: createWrapper(mockSupervisor),
      });

      await act(async () => {
        await result.current.refresh();
      });

      // Should have called VFS read (mocked)
      expect(result.current.state.credentials).toEqual([]);
    });
  });
});
