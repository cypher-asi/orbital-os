import { useState, useCallback, useMemo } from 'react';
import { Button, Card, CardItem, Input, Text } from '@cypher-asi/zui';
import { Cpu, Check, X, AlertTriangle, Loader, Shield, Key } from 'lucide-react';
import {
  useMachineKeys,
  type KeyScheme,
  type NeuralShard,
} from '../../../../desktop/hooks/useMachineKeys';
import { useSettingsStore } from '../../../../stores/settingsStore';
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
 * Requires 3 Neural shards for key derivation.
 */
export function GenerateMachineKeyPanel() {
  const { createMachineKey } = useMachineKeys();
  const { navigateBack } = usePanelDrill();
  const { defaultKeyScheme } = useSettingsStore();
  const [machineName, setMachineName] = useState('');
  const [keyScheme, setKeyScheme] = useState<KeyScheme>(defaultKeyScheme);
  const [isGenerating, setIsGenerating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // State for 3 Neural shards
  const [shard1, setShard1] = useState('');
  const [shard2, setShard2] = useState('');
  const [shard3, setShard3] = useState('');

  // Validate shards
  const shardsValid = useMemo(() => {
    const shards = [shard1, shard2, shard3];
    return shards.every((s) => s.trim().length > 0 && isValidHex(s));
  }, [shard1, shard2, shard3]);

  const canGenerate = machineName.trim().length > 0 && shardsValid;

  const handleGenerate = useCallback(async () => {
    if (!canGenerate) return;

    setIsGenerating(true);
    setError(null);
    try {
      // Build Neural shards array
      const shards: NeuralShard[] = [
        { index: 1, hex: shard1.trim().replace(/^0x/i, '') },
        { index: 2, hex: shard2.trim().replace(/^0x/i, '') },
        { index: 3, hex: shard3.trim().replace(/^0x/i, '') },
      ];

      await createMachineKey(machineName.trim(), undefined, keyScheme, shards);
      // Navigate back after successful creation
      navigateBack();
    } catch (err) {
      console.error('Failed to create machine key:', err);
      setError(err instanceof Error ? err.message : 'Failed to generate machine key');
    } finally {
      setIsGenerating(false);
    }
  }, [canGenerate, machineName, keyScheme, shard1, shard2, shard3, createMachineKey, navigateBack]);

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
          Create a new machine key derived from your Neural Key. You&apos;ll need 3 of your 5 Neural
          shards.
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

          {keyScheme === 'PqHybrid' && (
            <Card className={styles.infoCard}>
              <CardItem
                icon={<Shield size={14} />}
                title="Post-Quantum Protection"
                description="PQ keys are larger (~3KB vs 64 bytes) but provide long-term protection against quantum computers. Recommended for high-security applications."
                className={styles.infoCardItem}
              />
            </Card>
          )}

          {/* Neural Shards Section */}
          <div className={styles.shardsSection}>
            <div className={styles.shardsSectionHeader}>
              <Key size={14} />
              <Text size="xs" className={styles.shardsLabel}>
                Neural Shards (3 of 5 required)
              </Text>
            </div>

            <Card className={styles.infoCard}>
              <CardItem
                icon={<Key size={14} />}
                title="Shard-Based Key Derivation"
                description="Enter any 3 of your 5 Neural shards. Your machine key will be deterministically derived from your Neural Key."
                className={styles.infoCardItem}
              />
            </Card>

            <div className={styles.shardInputs}>
              <div className={styles.shardInputGroup}>
                <label className={styles.shardLabel}>Shard 1</label>
                <textarea
                  className={`${styles.shardInput} ${shard1 && !isValidHex(shard1) ? styles.shardInputError : ''}`}
                  value={shard1}
                  onChange={(e) => setShard1(e.target.value)}
                  placeholder="Paste hex shard (e.g., 01abc23def...)"
                  rows={2}
                />
              </div>

              <div className={styles.shardInputGroup}>
                <label className={styles.shardLabel}>Shard 2</label>
                <textarea
                  className={`${styles.shardInput} ${shard2 && !isValidHex(shard2) ? styles.shardInputError : ''}`}
                  value={shard2}
                  onChange={(e) => setShard2(e.target.value)}
                  placeholder="Paste hex shard (e.g., 01abc23def...)"
                  rows={2}
                />
              </div>

              <div className={styles.shardInputGroup}>
                <label className={styles.shardLabel}>Shard 3</label>
                <textarea
                  className={`${styles.shardInput} ${shard3 && !isValidHex(shard3) ? styles.shardInputError : ''}`}
                  value={shard3}
                  onChange={(e) => setShard3(e.target.value)}
                  placeholder="Paste hex shard (e.g., 01abc23def...)"
                  rows={2}
                />
              </div>
            </div>

            {!shardsValid && (shard1 || shard2 || shard3) && (
              <Text size="xs" className={styles.shardsHint}>
                All 3 shards must be valid hexadecimal strings
              </Text>
            )}
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
