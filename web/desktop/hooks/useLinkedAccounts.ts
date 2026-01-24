import { useState, useCallback } from 'react';
import { useIdentityStore, selectCurrentUser } from '../../stores';

// =============================================================================
// Linked Accounts Types (mirrors zos-identity/src/keystore.rs)
// =============================================================================

/**
 * Types of linkable credentials.
 * Corresponds to `CredentialType` in zos-identity/src/keystore.rs
 */
export type CredentialType = 'email' | 'phone' | 'oauth' | 'webauthn';

/**
 * A linked external credential.
 * Corresponds to `LinkedCredential` in zos-identity/src/keystore.rs
 */
export interface LinkedCredential {
  /** Credential type */
  type: CredentialType;
  /** Credential value (email address, phone number, etc.) */
  identifier: string;
  /** Whether this credential is verified */
  verified: boolean;
  /** When the credential was linked */
  linkedAt: number;
  /** When verification was completed */
  verifiedAt: number | null;
  /** Is this the primary credential of its type? */
  isPrimary: boolean;
}

/**
 * Linked Accounts state
 */
export interface LinkedAccountsState {
  /** Linked credentials */
  credentials: LinkedCredential[];
  /** Email pending verification (if any) */
  pendingEmail: string | null;
  /** Verification error message */
  verificationError: string | null;
  /** Loading state */
  isLoading: boolean;
  /** Error message */
  error: string | null;
}

/**
 * Hook return type
 */
export interface UseLinkedAccountsReturn {
  /** Current state */
  state: LinkedAccountsState;
  /** Attach an email (initiates verification) */
  attachEmail: (email: string) => Promise<{ verificationRequired: boolean }>;
  /** Verify email with code */
  verifyEmail: (email: string, code: string) => Promise<void>;
  /** Cancel pending email verification */
  cancelEmailVerification: () => void;
  /** Unlink an account */
  unlinkAccount: (type: CredentialType) => Promise<void>;
  /** Refresh state */
  refresh: () => Promise<void>;
}

// =============================================================================
// Initial State
// =============================================================================

const INITIAL_STATE: LinkedAccountsState = {
  credentials: [],
  pendingEmail: null,
  verificationError: null,
  isLoading: false,
  error: null,
};

// =============================================================================
// Hook Implementation
// 
// NOTE: This is currently a frontend-only implementation.
// In production, this would integrate with a credential verification service
// via the supervisor API. The backend types are defined in zos-identity but
// the IPC handlers are not yet implemented.
// =============================================================================

export function useLinkedAccounts(): UseLinkedAccountsReturn {
  const currentUser = useIdentityStore(selectCurrentUser);
  const [state, setState] = useState<LinkedAccountsState>(INITIAL_STATE);

  const attachEmail = useCallback(async (email: string): Promise<{ verificationRequired: boolean }> => {
    if (!currentUser?.id) {
      throw new Error('No user logged in');
    }

    setState(prev => ({ ...prev, isLoading: true, error: null }));

    try {
      // TODO: Call supervisor API to initiate email verification
      // This would typically:
      // 1. Generate a verification code
      // 2. Send it to the email address
      // 3. Store the pending verification state
      
      // For now, just set the pending email state
      setState(prev => ({
        ...prev,
        pendingEmail: email,
        verificationError: null,
        isLoading: false,
      }));

      return { verificationRequired: true };
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to initiate email verification';
      setState(prev => ({
        ...prev,
        isLoading: false,
        error: errorMsg,
      }));
      throw err;
    }
  }, [currentUser?.id]);

  const verifyEmail = useCallback(async (email: string, code: string): Promise<void> => {
    if (!currentUser?.id) {
      throw new Error('No user logged in');
    }

    if (!state.pendingEmail || state.pendingEmail !== email) {
      throw new Error('No pending verification for this email');
    }

    // Validate code format
    if (!/^\d{6}$/.test(code)) {
      setState(prev => ({
        ...prev,
        verificationError: 'Invalid code format. Please enter 6 digits.',
      }));
      throw new Error('Invalid code format');
    }

    setState(prev => ({ ...prev, isLoading: true, verificationError: null }));

    try {
      // TODO: Call supervisor API to verify the code
      // This would typically:
      // 1. Check the code against the stored verification code
      // 2. Mark the credential as verified
      // 3. Store the credential in the user's credential store
      
      // For now, accept any valid 6-digit code
      const now = Date.now();
      const newCredential: LinkedCredential = {
        type: 'email',
        identifier: email,
        verified: true,
        linkedAt: now,
        verifiedAt: now,
        isPrimary: true,
      };

      setState(prev => ({
        ...prev,
        credentials: [...prev.credentials.filter(c => c.type !== 'email'), newCredential],
        pendingEmail: null,
        verificationError: null,
        isLoading: false,
      }));
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Verification failed';
      setState(prev => ({
        ...prev,
        isLoading: false,
        verificationError: errorMsg,
      }));
      throw err;
    }
  }, [currentUser?.id, state.pendingEmail]);

  const cancelEmailVerification = useCallback(() => {
    setState(prev => ({
      ...prev,
      pendingEmail: null,
      verificationError: null,
    }));
  }, []);

  const unlinkAccount = useCallback(async (type: CredentialType): Promise<void> => {
    if (!currentUser?.id) {
      throw new Error('No user logged in');
    }

    setState(prev => ({ ...prev, isLoading: true, error: null }));

    try {
      // TODO: Call supervisor API to unlink the credential
      // This would remove it from the user's credential store in VFS
      
      setState(prev => ({
        ...prev,
        credentials: prev.credentials.filter(c => c.type !== type),
        isLoading: false,
      }));
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to unlink account';
      setState(prev => ({
        ...prev,
        isLoading: false,
        error: errorMsg,
      }));
      throw err;
    }
  }, [currentUser?.id]);

  const refresh = useCallback(async (): Promise<void> => {
    if (!currentUser?.id) {
      setState(INITIAL_STATE);
      return;
    }

    // TODO: Call supervisor API to fetch credentials from VFS
    // Path: /home/{user_id}/.zos/credentials/credentials.json
    
    // For now, keep current state
  }, [currentUser?.id]);

  return {
    state,
    attachEmail,
    verifyEmail,
    cancelEmailVerification,
    unlinkAccount,
    refresh,
  };
}
