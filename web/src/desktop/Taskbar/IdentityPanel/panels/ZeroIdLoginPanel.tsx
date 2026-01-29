import { useState } from 'react';
import { Button, Label, Spinner } from '@cypher-asi/zui';
import {
  Key,
  Clock,
  Server,
  Fingerprint,
  Shield,
  Laptop,
  Copy,
  Check,
  LogOut,
} from 'lucide-react';
import { useZeroIdAuth } from '../../../hooks/useZeroIdAuth';
import { useCopyToClipboard } from '../../../hooks/useCopyToClipboard';
import { useMachineKeys } from '../../../hooks/useMachineKeys';
import styles from './ZeroIdLoginPanel.module.css';

interface ZeroIdLoginPanelProps {
  /** Callback to close the subpanel (e.g., after disconnect) */
  onClose?: () => void;
}

/**
 * ZeroIdLoginPanel - Shows connected session info
 * 
 * This panel is only displayed when the user is connected to ZERO ID.
 * Login functionality is handled by the LoginModal.
 */
export function ZeroIdLoginPanel({ onClose }: ZeroIdLoginPanelProps) {
  const {
    remoteAuthState,
    isAuthenticating,
    error,
    disconnect,
    refreshToken,
    getTimeRemaining,
    isTokenExpired,
  } = useZeroIdAuth();

  const { copy, isCopied } = useCopyToClipboard();
  const [isDisconnecting, setIsDisconnecting] = useState(false);

  // Machine keys data (using hook triggers auto-refresh from keystore cache)
  const { state: machineKeysState } = useMachineKeys();
  const machines = machineKeysState.machines;
  // Look up the authorized machine for this session (by machineId from auth state)
  const authorizedMachine = machines.find(
    (m) => m.machineId === (remoteAuthState?.machineId ?? '')
  );

  const handleRefresh = async () => {
    try {
      await refreshToken();
    } catch {
      // Error is already set in the hook
    }
  };

  const handleDisconnect = async () => {
    setIsDisconnecting(true);
    try {
      await disconnect();
      // Close subpanel after successful disconnect
      onClose?.();
    } catch {
      // Error is already set in the hook
    } finally {
      setIsDisconnecting(false);
    }
  };

  // Helper to truncate session ID
  const truncateId = (id: string) => {
    if (id.length <= 12) return id;
    return id.slice(0, 6) + '...' + id.slice(-4);
  };

  // Helper to format server endpoint
  const formatServer = (endpoint: string) => {
    try {
      const url = new URL(endpoint);
      return url.host;
    } catch {
      return endpoint;
    }
  };

  // Only render when connected
  if (!remoteAuthState) {
    return null;
  }

  const expired = isTokenExpired();

  return (
    <div className={styles.panel}>
      {/* Scrollable Content Section */}
      <div className={styles.content}>
        {/* Session Info List */}
        <div className={styles.infoList}>
          <div className={styles.infoItem}>
            <div className={styles.infoLabel}>
              <Server size={12} />
              <span>Server</span>
            </div>
            <div className={styles.infoValueWithCopy}>
              <code className={styles.infoValueCode}>{formatServer(remoteAuthState.serverEndpoint)}</code>
              <Button
                variant={isCopied('server') ? 'primary' : 'ghost'}
                size="xs"
                onClick={() => copy(remoteAuthState.serverEndpoint, 'server')}
                className={styles.copyButton}
              >
                {isCopied('server') ? <Check size={12} /> : <Copy size={12} />}
              </Button>
            </div>
          </div>

          <div className={styles.infoItem}>
            <div className={styles.infoLabel}>
              <Fingerprint size={12} />
              <span>Session</span>
            </div>
            <div className={styles.infoValueWithCopy}>
              <code className={styles.infoValueCode}>{truncateId(remoteAuthState.sessionId)}</code>
              <Button
                variant={isCopied('session') ? 'primary' : 'ghost'}
                size="xs"
                onClick={() => copy(remoteAuthState.sessionId, 'session')}
                className={styles.copyButton}
              >
                {isCopied('session') ? <Check size={12} /> : <Copy size={12} />}
              </Button>
            </div>
          </div>

          <div className={styles.infoItem}>
            <div className={styles.infoLabel}>
              <Clock size={12} />
              <span>Expires</span>
            </div>
            <div className={expired ? styles.infoValueWarning : styles.infoValueAccent}>
              {getTimeRemaining()}
            </div>
          </div>

          <div className={styles.infoItem}>
            <div className={styles.infoLabel}>
              <Key size={12} />
              <span>Authorized Key</span>
            </div>
            <div className={styles.infoValueWithBadge}>
              {authorizedMachine ? (
                <>
                  <span>{authorizedMachine.machineName || 'Machine Key'}</span>
                  <Label variant="default" size="xs">
                    {authorizedMachine.keyScheme === 'pq_hybrid' ? 'PQ' : 'ED'}
                  </Label>
                  <Button
                    variant={isCopied('auth-key') ? 'primary' : 'ghost'}
                    size="xs"
                    onClick={() => copy(authorizedMachine.publicKey, 'auth-key')}
                    className={styles.copyButton}
                  >
                    {isCopied('auth-key') ? <Check size={12} /> : <Copy size={12} />}
                  </Button>
                </>
              ) : remoteAuthState.machineId ? (
                <>
                  <code className={styles.infoValueCode}>{truncateId(remoteAuthState.machineId)}</code>
                  <Button
                    variant={isCopied('auth-key') ? 'primary' : 'ghost'}
                    size="xs"
                    onClick={() => copy(remoteAuthState.machineId, 'auth-key')}
                    className={styles.copyButton}
                  >
                    {isCopied('auth-key') ? <Check size={12} /> : <Copy size={12} />}
                  </Button>
                </>
              ) : (
                <span className={styles.textMuted}>Unknown</span>
              )}
            </div>
          </div>

          <div className={styles.infoItem}>
            <div className={styles.infoLabel}>
              <Laptop size={12} />
              <span>Devices</span>
            </div>
            <div className={styles.infoValue}>{machines.length} linked</div>
          </div>

          <div className={styles.infoItem}>
            <div className={styles.infoLabel}>
              <Shield size={12} />
              <span>Scopes</span>
            </div>
            <div className={styles.scopesList}>
              {remoteAuthState.scopes.map((scope, i) => (
                <Label key={i} variant="default" size="xs">
                  {scope}
                </Label>
              ))}
            </div>
          </div>
        </div>

        {/* Actions */}
        {expired && remoteAuthState.refreshToken && (
          <div className={styles.actions}>
            <Button
              variant="secondary"
              onClick={handleRefresh}
              disabled={isAuthenticating}
              className={styles.actionButton}
            >
              {isAuthenticating ? <Spinner size="small" /> : <Key size={14} />}
              Refresh Token
            </Button>
          </div>
        )}

        {error && (
          <Label variant="error" className={styles.error}>
            {error}
          </Label>
        )}
      </div>

      {/* Footer - Disconnect Button pinned to bottom */}
      <div className={styles.footer}>
        <button
          className={styles.disconnectButton}
          onClick={handleDisconnect}
          disabled={isDisconnecting || isAuthenticating}
        >
          {isDisconnecting ? <Spinner size="small" /> : <LogOut size={14} />}
          <span>Disconnect</span>
        </button>
      </div>
    </div>
  );
}
