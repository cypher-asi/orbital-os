import { useState, useCallback, useMemo } from 'react';
import { GroupCollapsible, Button, Card, CardItem, Text, Label, Input } from '@cypher-asi/zui';
import { Brain, Copy, Check, Key, Calendar, AlertTriangle, Sparkles, Loader, Eye, EyeOff } from 'lucide-react';
import { useNeuralKey } from '@desktop/hooks/useNeuralKey';
import { useCopyToClipboard } from '@desktop/hooks/useCopyToClipboard';
import styles from './NeuralKeyPanel.module.css';

/** Minimum password length */
const MIN_PASSWORD_LENGTH = 12;

/**
 * Neural Key Panel
 *
 * States:
 * 1. Not Set - Show explanation, password inputs, and "Generate" button
 * 2. Generating - Show 3 external shards with copy buttons
 * 3. Active - Show fingerprint and created date
 */
export function NeuralKeyPanel() {
  const { state, generateNeuralKey, confirmShardsSaved } = useNeuralKey();
  const { copy, isCopied } = useCopyToClipboard();
  const [isGenerating, setIsGenerating] = useState(false);
  const [password, setPassword] = useState('');
  const [passwordConfirm, setPasswordConfirm] = useState('');
  const [showPassword, setShowPassword] = useState(false);
  const [showPasswordConfirm, setShowPasswordConfirm] = useState(false);

  // Password validation
  const passwordValidation = useMemo(() => {
    const isTooShort = password.length > 0 && password.length < MIN_PASSWORD_LENGTH;
    const passwordsMatch = password === passwordConfirm;
    const isValid = password.length >= MIN_PASSWORD_LENGTH && passwordsMatch;
    return { isTooShort, passwordsMatch, isValid };
  }, [password, passwordConfirm]);

  // Handle generate button click
  const handleGenerate = useCallback(async () => {
    if (!passwordValidation.isValid) return;
    
    setIsGenerating(true);
    try {
      await generateNeuralKey(password);
      // Clear password fields after successful generation
      setPassword('');
      setPasswordConfirm('');
    } catch (err) {
      console.error('Failed to generate Neural Key:', err);
    } finally {
      setIsGenerating(false);
    }
  }, [generateNeuralKey, password, passwordValidation.isValid]);

  // Handle copy all shards to clipboard
  const handleCopyAll = useCallback(() => {
    if (!state.pendingShards) return;
    const formattedShards = state.pendingShards
      .map((shard) => `Shard ${shard.index}: ${shard.hex}`)
      .join('\n');
    const text = `Neural Key Recovery Shards (1 of 3 + password required)\n${'='.repeat(50)}\n${formattedShards}`;
    copy(text, 'all');
  }, [state.pendingShards, copy]);

  // Handle "I've saved my shards" confirmation
  const handleConfirmSaved = useCallback(() => {
    confirmShardsSaved();
  }, [confirmShardsSaved]);

  // Truncate shard hex for display (first 8...last 8 chars)
  const truncateShardHex = (hex: string) => {
    if (hex.length <= 20) return hex;
    return `${hex.slice(0, 10)}...${hex.slice(-10)}`;
  };

  // Format date for display
  const formatDate = (timestamp: number) => {
    return new Date(timestamp).toLocaleDateString('en-US', {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  };

  // Format public key for display (truncate)
  const formatPubKey = (key: string) => {
    if (key.length <= 20) return key;
    return `${key.slice(0, 10)}...${key.slice(-8)}`;
  };

  // Show nothing during initial settling period
  if (state.isInitializing) {
    return null;
  }

  // Show loading state (only for operations like generate/recover, not initial load)
  if (state.isLoading && !isGenerating) {
    return (
      <div className={styles.panelContainer}>
        <GroupCollapsible title="Neural Key" defaultOpen className={styles.collapsibleSection}>
          <div className={styles.identitySection}>
            <div className={styles.loadingState}>
              <Loader size={24} className={styles.spinner} />
              <Text size="sm">Loading Neural Key status...</Text>
            </div>
          </div>
        </GroupCollapsible>
      </div>
    );
  }

  // Show error state
  if (state.error) {
    return (
      <div className={styles.panelContainer}>
        <GroupCollapsible title="Neural Key" defaultOpen className={styles.collapsibleSection}>
          <div className={styles.identitySection}>
            <Card className={styles.dangerCard}>
              <CardItem
                icon={<AlertTriangle size={16} />}
                title="Error"
                description={state.error}
                className={styles.dangerCardItem}
              />
            </Card>
          </div>
        </GroupCollapsible>
      </div>
    );
  }

  // State 1: No neural key - show explanation, password inputs, and generate button
  if (!state.hasNeuralKey && !state.pendingShards) {
    return (
      <div className={styles.panelContainer}>
        <GroupCollapsible title="Neural Key" defaultOpen className={styles.collapsibleSection}>
          <div className={styles.identitySection}>
            <div className={styles.identityHero}>
              <div className={styles.heroIcon}>
                <Brain size={48} strokeWidth={1} />
              </div>
              <Text size="md" className={styles.heroTitle}>
                Your Neural Key is Your Identity
              </Text>
              <Text size="sm" className={styles.heroDescription}>
                A Neural Key is a cryptographic identity that represents you across all devices.
                You'll receive 3 backup shards to store securely. To access your identity on a new
                device, you'll need 1 shard plus your password.
              </Text>
            </div>

            <div className={styles.passwordSection}>
              <div className={styles.passwordField}>
                <Label size="xs">Password (min {MIN_PASSWORD_LENGTH} characters)</Label>
                <div className={styles.passwordInputWrapper}>
                  <Input
                    type={showPassword ? 'text' : 'password'}
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    placeholder="Enter a secure password"
                    className={styles.passwordInput}
                  />
                  <button
                    type="button"
                    className={styles.passwordToggle}
                    onClick={() => setShowPassword(!showPassword)}
                  >
                    {showPassword ? <EyeOff size={16} /> : <Eye size={16} />}
                  </button>
                </div>
                {passwordValidation.isTooShort && (
                  <Text size="xs" className={styles.passwordError}>
                    Password must be at least {MIN_PASSWORD_LENGTH} characters
                  </Text>
                )}
              </div>

              <div className={styles.passwordField}>
                <Label size="xs">Confirm Password</Label>
                <div className={styles.passwordInputWrapper}>
                  <Input
                    type={showPasswordConfirm ? 'text' : 'password'}
                    value={passwordConfirm}
                    onChange={(e) => setPasswordConfirm(e.target.value)}
                    placeholder="Confirm your password"
                    className={styles.passwordInput}
                  />
                  <button
                    type="button"
                    className={styles.passwordToggle}
                    onClick={() => setShowPasswordConfirm(!showPasswordConfirm)}
                  >
                    {showPasswordConfirm ? <EyeOff size={16} /> : <Eye size={16} />}
                  </button>
                </div>
                {passwordConfirm.length > 0 && !passwordValidation.passwordsMatch && (
                  <Text size="xs" className={styles.passwordError}>
                    Passwords do not match
                  </Text>
                )}
              </div>
            </div>

            <div className={styles.buttonContainer}>
              <Button
                variant="primary"
                size="lg"
                onClick={handleGenerate}
                disabled={isGenerating || !passwordValidation.isValid}
                className={styles.generateButton}
              >
                {isGenerating ? (
                  <>
                    <Loader size={16} className={styles.spinner} />
                    Generating...
                  </>
                ) : (
                  <>
                    <Sparkles size={16} />
                    Generate Neural Key
                  </>
                )}
              </Button>
            </div>
          </div>
        </GroupCollapsible>
      </div>
    );
  }

  // State 2: Pending shards - show shards for user to backup
  if (state.pendingShards) {
    return (
      <div className={styles.panelContainer}>
        <GroupCollapsible
          title="Recovery Shards"
          count={state.pendingShards.length}
          defaultOpen
          className={styles.collapsibleSection}
        >
          <div className={styles.shardsSectionPadding}>
            <Card className={styles.warningCard}>
              <CardItem
                icon={<AlertTriangle size={16} />}
                title="Save these shards now!"
                description="These will only be shown once. Store each shard in a separate secure location. To recover your identity on a new device, you'll need 1 shard plus your password."
                className={styles.warningCardItem}
              />
            </Card>

            <div className={styles.copyAllContainer}>
              <Button
                variant={isCopied('all') ? 'primary' : 'secondary'}
                size="sm"
                onClick={handleCopyAll}
              >
                {isCopied('all') ? (
                  <>
                    <Check size={14} />
                    Copied All Shards
                  </>
                ) : (
                  <>
                    <Copy size={14} />
                    Copy All Shards
                  </>
                )}
              </Button>
            </div>

            {state.pendingShards.map((shard, index) => (
              <div key={shard.index} className={styles.shardItem}>
                <div className={styles.shardInfo}>
                  <Label size="xs" variant="default">
                    Shard {shard.index}
                  </Label>
                  <code className={styles.shardCodeInline}>{truncateShardHex(shard.hex)}</code>
                </div>
                <Button
                  variant={isCopied(`shard-${index}`) ? 'primary' : 'ghost'}
                  size="xs"
                  onClick={() => copy(shard.hex, `shard-${index}`)}
                >
                  {isCopied(`shard-${index}`) ? (
                    <>
                      <Check size={12} />
                      Copied
                    </>
                  ) : (
                    <>
                      <Copy size={12} />
                      Copy
                    </>
                  )}
                </Button>
              </div>
            ))}

            <div className={styles.buttonContainer}>
              <Button
                variant="primary"
                size="lg"
                onClick={handleConfirmSaved}
                className={styles.confirmButton}
              >
                <Check size={16} />
                I've Saved My Shards
              </Button>
            </div>
          </div>
        </GroupCollapsible>
      </div>
    );
  }

  // State 3: Active neural key - show status
  return (
    <div className={styles.panelContainer}>
      <GroupCollapsible title="Neural Key" defaultOpen className={styles.collapsibleSection}>
        <div className={styles.identitySection}>
          <div className={styles.statusHero}>
            <div className={styles.statusIconActive}>
              <Brain size={32} />
            </div>
            <Label size="sm" variant="success">
              Neural Key Active
            </Label>
          </div>

          {state.publicIdentifiers && (
            <div className={styles.keyDetails}>
              <div className={styles.keyDetailItem}>
                <div className={styles.keyDetailLabel}>
                  <Key size={14} />
                  <span>Identity Key</span>
                </div>
                <div className={styles.neuralprintRow}>
                  <code className={styles.keyDetailValue}>
                    {formatPubKey(state.publicIdentifiers.identitySigningPubKey)}
                  </code>
                  <Button
                    variant={isCopied('neuralprint') ? 'primary' : 'ghost'}
                    size="xs"
                    onClick={() =>
                      copy(state.publicIdentifiers?.identitySigningPubKey ?? '', 'neuralprint')
                    }
                  >
                    {isCopied('neuralprint') ? (
                      <>
                        <Check size={12} />
                        Copied
                      </>
                    ) : (
                      <>
                        <Copy size={12} />
                        Copy
                      </>
                    )}
                  </Button>
                </div>
              </div>

              <div className={styles.keyDetailItem}>
                <div className={styles.keyDetailLabel}>
                  <Calendar size={14} />
                  <span>Created</span>
                </div>
                <span className={styles.keyDetailValue}>
                  {state.createdAt ? formatDate(state.createdAt) : 'Unknown'}
                </span>
              </div>
            </div>
          )}

          <Card className={styles.infoCard}>
            <CardItem
              icon={<AlertTriangle size={16} />}
              title="Lost your shards or password?"
              description="If you forget your password and lose all 3 backup shards, you won't be able to recover your identity on a new device."
            />
          </Card>
        </div>
      </GroupCollapsible>
    </div>
  );
}
