import { useState, useCallback, useMemo } from 'react';
import { GroupCollapsible, Menu, Button, Card, CardItem, Label, Input, type MenuItem } from '@cypher-asi/zui';
import { 
  Mail, 
  Twitter, 
  Gamepad2,
  Check, 
  X, 
  Trash2,
  Send,
  Lock,
  Loader,
} from 'lucide-react';
import { useLinkedAccounts } from '../../../desktop/hooks/useLinkedAccounts';
import styles from './panels.module.css';

/**
 * Linked Accounts Panel
 * 
 * Features:
 * - Email: Add with verification flow
 * - X (Twitter): Grayed out "Coming Soon"
 * - Epic Games: Grayed out "Coming Soon"
 */
export function LinkedAccountsPanel() {
  const { state, attachEmail, verifyEmail, cancelEmailVerification, unlinkAccount } = useLinkedAccounts();
  
  // UI state
  const [showEmailForm, setShowEmailForm] = useState(false);
  const [emailInput, setEmailInput] = useState('');
  const [verificationCode, setVerificationCode] = useState('');
  const [isSending, setIsSending] = useState(false);
  const [isVerifying, setIsVerifying] = useState(false);

  // Get email credential if exists
  const emailCredential = state.credentials.find(c => c.type === 'email');

  // Handle send verification email
  const handleSendVerification = useCallback(async () => {
    if (!emailInput.trim()) return;
    
    setIsSending(true);
    try {
      await attachEmail(emailInput.trim());
    } catch (err) {
      console.error('Failed to send verification:', err);
    } finally {
      setIsSending(false);
    }
  }, [emailInput, attachEmail]);

  // Handle verify code
  const handleVerifyCode = useCallback(async () => {
    if (!verificationCode.trim() || !state.pendingEmail) return;
    
    setIsVerifying(true);
    try {
      await verifyEmail(state.pendingEmail, verificationCode.trim());
      setEmailInput('');
      setVerificationCode('');
      setShowEmailForm(false);
    } catch (err) {
      // Error is already set in store
      console.error('Verification failed:', err);
    } finally {
      setIsVerifying(false);
    }
  }, [verificationCode, state.pendingEmail, verifyEmail]);

  // Handle cancel verification
  const handleCancelVerification = useCallback(() => {
    cancelEmailVerification();
    setVerificationCode('');
  }, [cancelEmailVerification]);

  // Handle unlink email
  const handleUnlinkEmail = useCallback(async () => {
    try {
      await unlinkAccount('email');
    } catch (err) {
      console.error('Failed to unlink:', err);
    }
  }, [unlinkAccount]);

  // Build menu items for linked accounts
  const linkedAccountItems: MenuItem[] = useMemo(() => {
    const items: MenuItem[] = [];

    // Email item
    if (emailCredential?.verified) {
      items.push({
        id: 'email',
        label: emailCredential.identifier,
        icon: <Mail size={14} />,
        status: (
          <div className={styles.menuStatus}>
            <Label size="xs" variant="success">Verified</Label>
          </div>
        ),
      });
    }

    return items;
  }, [emailCredential]);

  // Coming soon items
  const comingSoonItems: MenuItem[] = useMemo(() => [
    {
      id: 'x',
      label: 'X (Twitter)',
      icon: <Twitter size={14} />,
      disabled: true,
      status: (
        <div className={styles.menuStatus}>
          <Lock size={12} className={styles.disabledIcon} />
          <Label size="xs" variant="default" className={styles.comingSoonLabel}>Coming Soon</Label>
        </div>
      ),
    },
    {
      id: 'epic',
      label: 'Epic Games',
      icon: <Gamepad2 size={14} />,
      disabled: true,
      status: (
        <div className={styles.menuStatus}>
          <Lock size={12} className={styles.disabledIcon} />
          <Label size="xs" variant="default" className={styles.comingSoonLabel}>Coming Soon</Label>
        </div>
      ),
    },
  ], []);

  return (
    <div className={styles.panelContainer}>
      {/* Connected Accounts */}
      {linkedAccountItems.length > 0 && (
        <GroupCollapsible
          title="Connected"
          count={linkedAccountItems.length}
          defaultOpen
          className={styles.collapsibleSection}
        >
          <div className={styles.menuContent}>
            <Menu 
              items={linkedAccountItems}
              background="none" 
              border="none" 
            />
          </div>
          
          {/* Email management */}
          {emailCredential?.verified && (
            <div className={styles.identitySection}>
              <Card className={styles.infoCard}>
                <CardItem
                  icon={<Mail size={16} />}
                  title={emailCredential.identifier}
                  description={`Linked on ${new Date(emailCredential.linkedAt).toLocaleDateString()}`}
                >
                  <Button
                    variant="danger"
                    size="xs"
                    onClick={handleUnlinkEmail}
                  >
                    <Trash2 size={12} />
                    Unlink
                  </Button>
                </CardItem>
              </Card>
            </div>
          )}
        </GroupCollapsible>
      )}

      {/* Email Section */}
      {!emailCredential?.verified && (
        <GroupCollapsible
          title="Email"
          defaultOpen
          className={styles.collapsibleSection}
        >
          <div className={styles.identitySection}>
            {/* Pending verification */}
            {state.pendingEmail ? (
              <div className={styles.verificationFlow}>
                <Card className={styles.infoCard}>
                  <CardItem
                    icon={<Mail size={16} />}
                    title="Verification code sent"
                    description={`We sent a 6-digit code to ${state.pendingEmail}`}
                  />
                </Card>

                <div className={styles.verificationInput}>
                  <Input
                    value={verificationCode}
                    onChange={(e) => setVerificationCode(e.target.value.replace(/\D/g, '').slice(0, 6))}
                    placeholder="Enter 6-digit code"
                    maxLength={6}
                    autoFocus
                  />
                  {state.verificationError && (
                    <span className={styles.errorText}>{state.verificationError}</span>
                  )}
                </div>

                <div className={styles.verificationButtons}>
                  <Button 
                    variant="ghost" 
                    size="sm"
                    onClick={handleCancelVerification}
                    disabled={isVerifying}
                  >
                    <X size={14} />
                    Cancel
                  </Button>
                  <Button 
                    variant="primary" 
                    size="sm"
                    onClick={handleVerifyCode}
                    disabled={isVerifying || verificationCode.length !== 6}
                  >
                    {isVerifying ? (
                      <>
                        <Loader size={14} className={styles.spinner} />
                        Verifying...
                      </>
                    ) : (
                      <>
                        <Check size={14} />
                        Verify
                      </>
                    )}
                  </Button>
                </div>
              </div>
            ) : showEmailForm ? (
              /* Email input form */
              <div className={styles.addForm}>
                <Input
                  type="email"
                  value={emailInput}
                  onChange={(e) => setEmailInput(e.target.value)}
                  placeholder="Enter your email address"
                  autoFocus
                />
                <div className={styles.addFormButtons}>
                  <Button 
                    variant="ghost" 
                    size="sm"
                    onClick={() => {
                      setShowEmailForm(false);
                      setEmailInput('');
                    }}
                    disabled={isSending}
                  >
                    <X size={14} />
                    Cancel
                  </Button>
                  <Button 
                    variant="primary" 
                    size="sm"
                    onClick={handleSendVerification}
                    disabled={isSending || !emailInput.includes('@')}
                  >
                    {isSending ? (
                      <>
                        <Loader size={14} className={styles.spinner} />
                        Sending...
                      </>
                    ) : (
                      <>
                        <Send size={14} />
                        Send Code
                      </>
                    )}
                  </Button>
                </div>
              </div>
            ) : (
              /* Add email button */
              <Button 
                variant="ghost" 
                size="md"
                onClick={() => setShowEmailForm(true)}
                className={styles.addButton}
              >
                <Mail size={14} />
                Link Email Address
              </Button>
            )}
          </div>
        </GroupCollapsible>
      )}

      {/* Coming Soon Section */}
      <GroupCollapsible
        title="More Services"
        defaultOpen
        className={styles.collapsibleSection}
      >
        <div className={styles.menuContent}>
          <Menu 
            items={comingSoonItems}
            background="none" 
            border="none" 
          />
        </div>
      </GroupCollapsible>
    </div>
  );
}
