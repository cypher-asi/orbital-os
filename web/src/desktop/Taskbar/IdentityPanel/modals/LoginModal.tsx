import { useState, useEffect, useRef, useMemo, useCallback, type ReactNode } from 'react';
import { PanelLogin, Text, Button, type LoginProvider } from '@cypher-asi/zui';
import { Key, Github } from 'lucide-react';
import { useZeroIdAuth } from '../../../hooks/useZeroIdAuth';
import { useLinkedAccounts } from '../../../hooks/useLinkedAccounts';
import styles from './LoginModal.module.css';

interface LoginModalProps {
  onClose: () => void;
}

/**
 * Get provider icon based on provider name
 */
function getProviderIcon(providerName: string): ReactNode {
  const name = providerName.toLowerCase();

  switch (name) {
    case 'google':
      return (
        <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
          <path d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z" />
          <path d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" />
          <path d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z" />
          <path d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" />
        </svg>
      );

    case 'github':
      return <Github size={20} />;

    case 'twitter':
    case 'x':
      return (
        <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
          <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
        </svg>
      );

    default:
      return null;
  }
}

/**
 * Capitalize first letter of a string
 */
function capitalize(str: string): string {
  return str.charAt(0).toUpperCase() + str.slice(1);
}

export function LoginModal({ onClose }: LoginModalProps) {
  const overlayRef = useRef<HTMLDivElement>(null);
  const modalRef = useRef<HTMLDivElement>(null);

  // Auth hooks
  const {
    loginWithEmail,
    loginWithMachineKey,
    isAuthenticating,
    error: authError,
  } = useZeroIdAuth();

  const { state: linkedAccountsState } = useLinkedAccounts();

  // Form state
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Handle email/password login
  const handleEmailPasswordLogin = async (emailValue: string, passwordValue: string) => {
    setIsLoading(true);
    setError(null);

    try {
      await loginWithEmail(emailValue, passwordValue);
      // Success - close modal
      onClose();
    } catch (err) {
      // Error is set in useZeroIdAuth hook, display it
      const errorMsg = err instanceof Error ? err.message : 'Login failed';
      setError(errorMsg);
    } finally {
      setIsLoading(false);
    }
  };

  // Handle machine key login
  const handleMachineKeyLogin = useCallback(async () => {
    setIsLoading(true);
    setError(null);

    try {
      await loginWithMachineKey();
      // Success - close modal
      onClose();
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Machine key login failed';
      setError(errorMsg);
    } finally {
      setIsLoading(false);
    }
  }, [loginWithMachineKey, onClose]);

  // Build login providers (OAuth from linked accounts only)
  const loginProviders: LoginProvider[] = useMemo(() => {
    const oauthCredentials = linkedAccountsState.credentials.filter((c) => c.type === 'oauth');
    return oauthCredentials.map((cred) => ({
      id: cred.identifier,
      icon: getProviderIcon(cred.identifier),
      label: `Continue with ${capitalize(cred.identifier)}`,
      onClick: async () => {
        // TODO: Implement OAuth login flow
        console.log(`[LoginModal] OAuth login with ${cred.identifier} - not yet implemented`);
        setError(`OAuth login with ${capitalize(cred.identifier)} is not yet implemented`);
      },
    }));
  }, [linkedAccountsState.credentials]);

  // Click outside to close
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      // Only close if clicking directly on the overlay (not on modal content)
      if (event.target === overlayRef.current) {
        onClose();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [onClose]);

  // ESC key to close
  useEffect(() => {
    const handleEscKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };

    document.addEventListener('keydown', handleEscKey);
    return () => {
      document.removeEventListener('keydown', handleEscKey);
    };
  }, [onClose]);

  const displayError = error || authError;
  const isAnyLoading = isLoading || isAuthenticating;

  return (
    <div ref={overlayRef} className={styles.overlay}>
      <div ref={modalRef} className={styles.modal}>
        <div className={styles.modalContent}>
          <PanelLogin
            appName="ZERO OS"
            description="Please enter your credentials"
            emailValue={email}
            onEmailChange={setEmail}
            passwordValue={password}
            onPasswordChange={setPassword}
            onSubmit={handleEmailPasswordLogin}
            isLoading={isAnyLoading}
            image="https://i.pinimg.com/736x/7d/a9/93/7da993ea181a49912defefcc4c41c33a.jpg"
            imageHeight="200px"
            loginProviders={loginProviders}
            error={displayError}
            bottomContent={
              <Button
                variant="ghost"
                onClick={handleMachineKeyLogin}
                disabled={isAnyLoading}
                className={styles.machineKeyButton}
              >
                <Key size={16} />
                Login with Machine Key
              </Button>
            }
          />
        </div>

        {/* Error display below login panel - positioned absolutely */}
        {displayError && (
          <div className={styles.errorBox}>
            <Text variant="secondary" size="sm" style={{ color: '#ef4444' }}>
              {displayError}
            </Text>
          </div>
        )}
      </div>
    </div>
  );
}
