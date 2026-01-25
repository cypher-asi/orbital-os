import { useState, useCallback } from 'react';
import { GroupCollapsible, Input, Button, Label, Text } from '@cypher-asi/zui';
import { Check, RotateCcw } from 'lucide-react';
import { DEFAULT_RPC_ENDPOINT } from '../../../../stores';
import styles from './NetworkPanel.module.css';

interface NetworkPanelProps {
  rpcEndpoint: string;
  onRpcEndpointChange: (endpoint: string) => void;
}

/**
 * Network Settings Panel
 * - ZERO-ID RPC endpoint configuration
 */
export function NetworkPanel({ rpcEndpoint, onRpcEndpointChange }: NetworkPanelProps) {
  const [editValue, setEditValue] = useState(rpcEndpoint);
  const [isDirty, setIsDirty] = useState(false);

  const handleInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      setEditValue(e.target.value);
      setIsDirty(e.target.value !== rpcEndpoint);
    },
    [rpcEndpoint]
  );

  const handleSave = useCallback(() => {
    onRpcEndpointChange(editValue);
    setIsDirty(false);
  }, [editValue, onRpcEndpointChange]);

  const handleReset = useCallback(() => {
    setEditValue(DEFAULT_RPC_ENDPOINT);
    onRpcEndpointChange(DEFAULT_RPC_ENDPOINT);
    setIsDirty(false);
  }, [onRpcEndpointChange]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'Enter' && isDirty) {
        handleSave();
      }
    },
    [isDirty, handleSave]
  );

  return (
    <div className={styles.panelContainer}>
      <GroupCollapsible title="Zero-ID RPC" defaultOpen className={styles.collapsibleSection}>
        <div className={styles.fieldContent}>
          <Text size="xs" variant="muted" className={styles.fieldLabel}>
            Endpoint
          </Text>

          <div className={styles.inputRow}>
            <Input
              value={editValue}
              onChange={handleInputChange}
              onKeyDown={handleKeyDown}
              placeholder={DEFAULT_RPC_ENDPOINT}
              className={styles.endpointInput}
            />
            {isDirty && (
              <Button variant="primary" size="sm" onClick={handleSave}>
                <Check size={14} />
              </Button>
            )}
            <Button variant="ghost" size="sm" onClick={handleReset} title="Reset to default">
              <RotateCcw size={14} />
            </Button>
          </div>

          {rpcEndpoint !== DEFAULT_RPC_ENDPOINT && (
            <Label size="xs" variant="warning">
              Modified
            </Label>
          )}

          <Text size="xs" variant="muted">
            Default: {DEFAULT_RPC_ENDPOINT}
          </Text>
        </div>
      </GroupCollapsible>
    </div>
  );
}

export { DEFAULT_RPC_ENDPOINT };
