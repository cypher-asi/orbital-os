import { useState, useCallback, useMemo } from 'react';
import {
  GroupCollapsible,
  Menu,
  Button,
  Card,
  CardItem,
  Input,
  Label,
  type MenuItem,
} from '@cypher-asi/zui';
import { Mail, Twitter, Gamepad2, X, Loader, AlertCircle, Lock, LogIn } from 'lucide-react';
import { useLinkedAccounts } from '../../../../desktop/hooks/useLinkedAccounts';
import { useZeroIdAuth } from '../../../../desktop/hooks/useZeroIdAuth';
import styles from './LinkedAccountsPanel.module.css';

/** Menu item IDs for linked account types */
type LinkedAccountMenuId = 'email' | 'x' | 'epic';

/**
 * Linked Accounts Panel
 *
 * Features:
 * - Email: Add via ZID API (requires ZID login first)
 * - X (Twitter): Grayed out "Coming Soon"
 * - Epic Games: Grayed out "Coming Soon"
 */
export function LinkedAccountsPanel() {
  const { state, attachEmail, unlinkAccount } = useLinkedAccounts();
  const { remoteAuthState, isTokenExpired } = useZeroIdAuth();

  // UI state
  const [showEmailForm, setShowEmailForm] = useState(false);
  const [emailInput, setEmailInput] = useState('');
  const [passwordInput, setPasswordInput] = useState('');
  const [isSending, setIsSending] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  // Get email credential if exists
  const emailCredential = state.credentials.find((c) => c.type === 'email');

  // Check if user has valid ZID session
  const hasValidZidSession = remoteAuthState && !isTokenExpired();

  // Handle attach email
  const handleAttachEmail = useCallback(async () => {
    if (!emailInput.trim() || !passwordInput.trim()) return;
    if (!remoteAuthState) {
      setFormError('Please login to ZERO-ID first');
      return;
    }

    // Validate password
    if (passwordInput.length < 12) {
      setFormError('Password must be at least 12 characters');
      return;
    }

    setIsSending(true);
    setFormError(null);

    try {
      await attachEmail(
        emailInput.trim(),
        passwordInput,
        remoteAuthState.accessToken,
        remoteAuthState.serverEndpoint
      );

      // Success - reset form
      setEmailInput('');
      setPasswordInput('');
      setShowEmailForm(false);
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to attach email';
      console.error('Failed to attach email:', err);
      setFormError(errorMsg);
    } finally {
      setIsSending(false);
    }
  }, [emailInput, passwordInput, attachEmail, remoteAuthState]);

  // Handle cancel form
  const handleCancelForm = useCallback(() => {
    setShowEmailForm(false);
    setEmailInput('');
    setPasswordInput('');
    setFormError(null);
  }, []);

  // Handle unlink email
  const handleUnlinkEmail = useCallback(async () => {
    try {
      await unlinkAccount('email');
    } catch (err) {
      console.error('Failed to unlink:', err);
    }
  }, [unlinkAccount]);

  // Handle menu item selection
  const handleMenuSelect = useCallback(
    (id: string): void => {
      const menuId = id as LinkedAccountMenuId;
      if (menuId === 'email') {
        if (emailCredential?.verified) {
          // If connected, trigger disconnect
          handleUnlinkEmail();
        } else {
          // If not connected, show the form
          setShowEmailForm(true);
          setFormError(null);
        }
      }
      // Other items are disabled, no action needed
    },
    [emailCredential?.verified, handleUnlinkEmail]
  );

  // Build menu items
  const accountItems: MenuItem[] = useMemo(
    () => [
      // Email - first item
      {
        id: 'email',
        label: 'Email',
        icon: <Mail size={14} />,
        status: (
          <div className={styles.menuStatus}>
            {emailCredential?.verified ? (
              <Label size="xs" variant="success">
                Connected
              </Label>
            ) : (
              <Label size="xs">Connect</Label>
            )}
          </div>
        ),
      },
      // X (Twitter) - coming soon
      {
        id: 'x',
        label: 'X (Twitter)',
        icon: <Twitter size={14} />,
        disabled: true,
        status: (
          <div className={styles.menuStatus}>
            <Label size="xs" variant="secondary">
              Coming Soon
            </Label>
          </div>
        ),
      },
      // Epic Games - coming soon
      {
        id: 'epic',
        label: 'Epic Games',
        icon: <Gamepad2 size={14} />,
        disabled: true,
        status: (
          <div className={styles.menuStatus}>
            <Label size="xs" variant="secondary">
              Coming Soon
            </Label>
          </div>
        ),
      },
    ],
    [emailCredential?.verified]
  );

  // Render email attachment form
  const renderEmailForm = () => (
    <div className={styles.identitySection}>
      {!hasValidZidSession ? (
        // Show login required message
        <Card className={styles.infoCard}>
          <CardItem title="Login to ZERO-ID first">
            <div className={styles.loginPrompt}>
              <LogIn size={16} />
              <span>You need to login with your machine key before linking an email.</span>
            </div>
          </CardItem>
        </Card>
      ) : (
        // Show email + password form
        <div className={styles.addForm}>
          <Input
            type="email"
            value={emailInput}
            onChange={(e) => setEmailInput(e.target.value)}
            placeholder="Enter your email address"
            autoFocus
          />
          <div className={styles.passwordWrapper}>
            <Lock size={14} className={styles.passwordIcon} />
            <Input
              type="password"
              value={passwordInput}
              onChange={(e) => setPasswordInput(e.target.value)}
              placeholder="Create a password (12+ characters)"
            />
          </div>
          <p className={styles.passwordHint}>
            Password must include uppercase, lowercase, number, and symbol.
          </p>

          {(formError || state.error) && (
            <div className={styles.errorContainer}>
              <AlertCircle size={14} />
              <span className={styles.errorText}>{formError || state.error}</span>
            </div>
          )}

          <div className={styles.addFormButtons}>
            <Button variant="ghost" size="sm" onClick={handleCancelForm} disabled={isSending}>
              <X size={14} />
              Cancel
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={handleAttachEmail}
              disabled={isSending || !emailInput.includes('@') || passwordInput.length < 12}
            >
              {isSending ? (
                <>
                  <Loader size={14} className={styles.spinner} />
                  Linking...
                </>
              ) : (
                <>
                  <Mail size={14} />
                  Link Email
                </>
              )}
            </Button>
          </div>
        </div>
      )}
    </div>
  );

  return (
    <div className={styles.panelContainer}>
      {/* All Accounts in a single group */}
      <GroupCollapsible title="Accounts" defaultOpen className={styles.collapsibleSection}>
        <div className={styles.menuContent}>
          <Menu items={accountItems} onChange={handleMenuSelect} background="none" border="none" />
        </div>

        {/* Email form appears below the list when active */}
        {showEmailForm && !emailCredential?.verified && renderEmailForm()}
      </GroupCollapsible>
    </div>
  );
}
