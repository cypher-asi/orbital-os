import { useState, useCallback, useMemo } from 'react';
import { Button, Card, CardItem, Input, Text } from '@cypher-asi/zui';
import { Cpu, Check, X, AlertTriangle, Loader, Shield, Key } from 'lucide-react';
import {
  useMachineKeys,
  type KeyScheme,
  type NeuralShard,
} from '@desktop/hooks/useMachineKeys';
import { useSettingsStore } from '@/stores/settingsStore';
import { usePanelDrill } from '../../context';
import styles from './GenerateMachineKeyPanel.module.css';

/**
 * Check if a string is valid hex (optionally with 0x prefix)
 */
function isValidHex(value: string): boolean {
  const cleaned = value.trim().replace(/^0x/i, '');
  if (cleaned.length === 0) return false;
  return /^[0-9a-fA-F]+$/.test(cleaned);
}

/**
 * Generate Machine Key Panel
 *
 * Drill-down panel for creating a new machine key.
 * Requires 1 Neural shard + password for key derivation.
 */
export function GenerateMachineKeyPanel() {
  const { createMachineKey } = useMachineKeys();
  const { navigateBack } = usePanelDrill();
  const { defaultKeyScheme } = useSettingsStore();
  const [machineName, setMachineName] = useState('');
  const [keyScheme, setKeyScheme] = useState<KeyScheme>(defaultKeyScheme);
  const [isGenerating, setIsGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // State for external shard + password
  const [externalShard, setExternalShard] = useState('');
  const [externalShardIndex, setExternalShardIndex] = useState(1);
  const [password, setPassword] = useState('');

  const shardValid = useMemo(() => {
    return externalShard.trim().length > 0 && isValidHex(externalShard);
  }, [externalShard]);
  const shardIndexValid = useMemo(() => {
    return Number.isInteger(externalShardIndex) && externalShardIndex >= 1 && externalShardIndex <= 5;
  }, [externalShardIndex]);

  const canGenerate =
    machineName.trim().length > 0 &&
    shardValid &&
    shardIndexValid &&
    password.trim().length > 0;

  const handleGenerate = useCallback(async () => {
    if (!canGenerate) return;

    setIsGenerating(true);
    setError(null);
    try {
      const shard: NeuralShard = {
        index: externalShardIndex,
        hex: externalShard.trim().replace(/^0x/i, ''),
      };

      await createMachineKey(
        machineName.trim(),
        undefined,
        keyScheme,
        shard,
        password
      );
      // Navigate back after successful creation
      navigateBack();
    } catch (err) {
      console.error('Failed to create machine key:', err);
      setError(err instanceof Error ? err.message : 'Failed to generate machine key');
    } finally {
      setIsGenerating(false);
    }
  }, [
    canGenerate,
    machineName,
    keyScheme,
    externalShard,
    password,
    createMachineKey,
    navigateBack,
  ]);

  const handleCancel = useCallback(() => {
    // Navigate back to Machine Keys panel
    navigateBack();
  }, [navigateBack]);

  return (
    <div className={styles.panelContainer}>
      <div className={styles.identitySection}>
        <div className={styles.heroIcon}>
          <Cpu size={48} strokeWidth={1} />
        </div>
        <Text size="md" className={styles.heroTitle}>
          Generate Machine Key
        </Text>
        <Text size="sm" className={styles.heroDescription}>
          Create a new machine key derived from your Neural Key. You&apos;ll need 1 Neural shard and
          your password.
        </Text>

        <div className={styles.addForm}>
          <Input
            value={machineName}
            onChange={(e) => setMachineName(e.target.value)}
            placeholder="Machine name (e.g., Work Laptop)"
            autoFocus
          />

          <div className={styles.schemeSection}>
            <Text size="xs" className={styles.schemeLabel}>
              Key Scheme
            </Text>
            <select
              value={keyScheme}
              onChange={(e) => setKeyScheme(e.target.value as KeyScheme)}
              className={styles.schemeSelect}
            >
              <option value="classical">Classical (Ed25519 + X25519)</option>
              <option value="pq_hybrid">Post-Quantum Hybrid (+ ML-DSA-65 + ML-KEM-768)</option>
            </select>
          </div>

          {keyScheme === 'pq_hybrid' && (
            <Card className={styles.infoCard}>
              <CardItem
                icon={<Shield size={14} />}
                title="Post-Quantum Protection"
                description="PQ keys are larger (~3KB vs 64 bytes) but provide long-term protection against quantum computers. Recommended for high-security applications."
                className={styles.infoCardItem}
              />
            </Card>
          )}

          {/* External Shard Section */}
          <div className={styles.shardsSection}>
            <div className={styles.shardsSectionHeader}>
              <Key size={14} />
              <Text size="xs" className={styles.shardsLabel}>
                External Shard (1 of 3 required)
              </Text>
            </div>

            <Card className={styles.infoCard}>
              <CardItem
                icon={<Key size={14} />}
                title="Shard + Password Key Derivation"
                description="Enter one of your external shards and your password. The other two shards are decrypted from keystore."
                className={styles.infoCardItem}
              />
            </Card>

            <div className={styles.shardInputs}>
              <div className={styles.shardInputGroup}>
                <label className={styles.shardLabel}>Shard Index (1-5)</label>
                <Input
                  value={String(externalShardIndex)}
                  onChange={(e) => {
                    const parsed = Number(e.target.value);
                    setExternalShardIndex(Number.isNaN(parsed) ? 0 : parsed);
                  }}
                  placeholder="1"
                  type="number"
                  min={1}
                  max={5}
                />
              </div>
              <div className={styles.shardInputGroup}>
                <label className={styles.shardLabel}>External Shard</label>
                <textarea
                  className={`${styles.shardInput} ${externalShard && !isValidHex(externalShard) ? styles.shardInputError : ''}`}
                  value={externalShard}
                  onChange={(e) => setExternalShard(e.target.value)}
                  placeholder="Paste hex shard (e.g., 01abc23def...)"
                  rows={2}
                />
              </div>
            </div>

            {!shardIndexValid && (
              <Text size="xs" className={styles.shardsHint}>
                Shard index must be between 1 and 5
              </Text>
            )}
            {!shardValid && externalShard && (
              <Text size="xs" className={styles.shardsHint}>
                Shard must be a valid hexadecimal string
              </Text>
            )}
          </div>

          <div className={styles.addForm}>
            <Input
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="Password used to encrypt shards"
              type="password"
            />
          </div>

          {error && (
            <Card className={styles.dangerCard}>
              <CardItem
                icon={<AlertTriangle size={14} />}
                title="Error"
                description={error}
                className={styles.dangerCardItem}
              />
            </Card>
          )}

          <div className={styles.addFormButtons}>
            <Button variant="ghost" size="md" onClick={handleCancel} disabled={isGenerating}>
              <X size={14} />
              Cancel
            </Button>
            <Button
              variant="primary"
              size="md"
              onClick={handleGenerate}
              disabled={isGenerating || !canGenerate}
            >
              {isGenerating ? (
                <>
                  <Loader size={14} className={styles.spinner} />
                  Generating...
                </>
              ) : (
                <>
                  <Check size={14} />
                  Generate
                </>
              )}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
