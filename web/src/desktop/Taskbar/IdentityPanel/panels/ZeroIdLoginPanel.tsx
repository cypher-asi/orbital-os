import { useState } from 'react';
import { Button, Label, Spinner, Menu } from '@cypher-asi/zui';
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
  RefreshCw,
  Cpu,
  Brain,
  Mail,
  Globe,
  RotateCcw,
  UserCheck,
  KeyRound,
  ChevronDown,
  ChevronRight,
} from 'lucide-react';
import { useZeroIdAuth } from '../../../hooks/useZeroIdAuth';
import { useCopyToClipboard } from '../../../hooks/useCopyToClipboard';
import { useMachineKeys } from '../../../hooks/useMachineKeys';
import { usePanelDrillOptional } from '../context';
import { formatLoginType, type LoginType } from '@/stores';
import styles from './ZeroIdLoginPanel.module.css';

/** Get the appropriate icon for a login type */
function getLoginTypeIcon(loginType: LoginType, size = 12) {
  switch (loginType) {
    case 'machine_key':
      return <Cpu size={size} />;
    case 'neural_key':
      return <Brain size={size} />;
    case 'email':
      return <Mail size={size} />;
    case 'oauth':
      return <Globe size={size} />;
    case 'webauthn':
      return <Fingerprint size={size} />;
    case 'recovery':
      return <RotateCcw size={size} />;
    default:
      return <Key size={size} />;
  }
}

interface ZeroIdLoginPanelProps {
  /** Callback to close the subpanel (e.g., after disconnect) - optional fallback */
  onClose?: () => void;
}

/**
 * ZeroIdLoginPanel - Shows connected session info
 *
 * This panel is only displayed when the user is connected to ZERO ID.
 * Login functionality is handled by the LoginModal.
 * Uses PanelDrill context for navigation when available.
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
  const [showTokenExpanded, setShowTokenExpanded] = useState(false);

  // PanelDrill navigation (optional - allows component to work both inside and outside drill context)
  const panelDrill = usePanelDrillOptional();

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
      // Navigate back using PanelDrill context if available, otherwise use fallback
      if (panelDrill) {
        panelDrill.navigateBack();
      } else {
        onClose?.();
      }
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

  // Helper to truncate access token for preview
  const truncateToken = (token: string) => {
    if (token.length <= 24) return token;
    return token.slice(0, 12) + '...' + token.slice(-8);
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
              <UserCheck size={12} />
              <span>Auth Type</span>
            </div>
            <div className={styles.infoValueWithBadge}>
              {remoteAuthState.loginType ? (
                <>
                  {getLoginTypeIcon(remoteAuthState.loginType)}
                  <span>{formatLoginType(remoteAuthState.loginType)}</span>
                </>
              ) : (
                <span className={styles.textMuted}>Unknown</span>
              )}
            </div>
          </div>

          <div className={styles.infoItem}>
            <div className={styles.infoLabel}>
              <Clock size={12} />
              <span>Expires</span>
            </div>
            <div className={styles.infoValueWithCopy}>
              <span className={expired ? styles.infoValueWarning : styles.infoValueAccent}>
                {getTimeRemaining()}
              </span>
              {remoteAuthState.refreshToken && (
                <Button
                  variant="ghost"
                  size="xs"
                  onClick={handleRefresh}
                  disabled={isAuthenticating}
                  className={styles.copyButton}
                  title="Refresh token"
                >
                  {isAuthenticating ? <Spinner size="small" /> : <RefreshCw size={12} />}
                </Button>
              )}
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
                    onClick={() => copy(authorizedMachine.signingPublicKey, 'auth-key')}
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

          {/* Access Token - expandable */}
          <div className={styles.infoItem}>
            <div 
              className={`${styles.infoLabel} ${styles.infoLabelClickable}`}
              onClick={() => setShowTokenExpanded(!showTokenExpanded)}
            >
              <KeyRound size={12} />
              <span>Access Token</span>
              <span className={styles.expandToggle}>
                {showTokenExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              </span>
            </div>
            {!showTokenExpanded ? (
              <div className={styles.infoValueWithCopy}>
                <code className={styles.infoValueCode}>{truncateToken(remoteAuthState.accessToken)}</code>
                <Button
                  variant={isCopied('token') ? 'primary' : 'ghost'}
                  size="xs"
                  onClick={() => copy(remoteAuthState.accessToken, 'token')}
                  className={styles.copyButton}
                >
                  {isCopied('token') ? <Check size={12} /> : <Copy size={12} />}
                </Button>
              </div>
            ) : (
              <div className={styles.tokenExpanded}>
                <code className={styles.tokenFull}>{remoteAuthState.accessToken}</code>
                <Button
                  variant={isCopied('token') ? 'primary' : 'ghost'}
                  size="xs"
                  onClick={() => copy(remoteAuthState.accessToken, 'token')}
                  className={styles.copyButtonToken}
                >
                  {isCopied('token') ? <Check size={12} /> : <Copy size={12} />}
                </Button>
              </div>
            )}
          </div>
        </div>

        {error && (
          <Label variant="error" className={styles.error}>
            {error}
          </Label>
        )}
      </div>

      {/* Footer - Disconnect Button pinned to bottom */}
      <div className={styles.footer}>
        <Menu
          items={[
            {
              id: 'disconnect',
              label: isDisconnecting ? 'Disconnecting...' : 'Disconnect',
              icon: isDisconnecting ? <Spinner size="small" /> : <LogOut size={14} />,
              disabled: isDisconnecting || isAuthenticating,
            },
          ]}
          onChange={(id) => id === 'disconnect' && handleDisconnect()}
          background="none"
          border="none"
        />
      </div>
    </div>
  );
}
