import { useState } from 'react';
import { Button, Card, CardItem, Text, Input, Label, Spinner, ButtonCollapsible } from '@cypher-asi/zui';
import { User, LogIn, LogOut, Key, Clock, Copy, Check, Shield } from 'lucide-react';
import { useZeroIdAuth } from '../../../desktop/hooks/useZeroIdAuth';
import { useCopyToClipboard } from '../../../desktop/hooks/useCopyToClipboard';
import styles from './ZeroIdLoginPanel.module.css';

export function ZeroIdLoginPanel() {
  const {
    remoteAuthState,
    isAuthenticating,
    error,
    loginWithEmail,
    loginWithMachineKey,
    logout,
    refreshToken,
    getTimeRemaining,
    isTokenExpired,
  } = useZeroIdAuth();

  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [localError, setLocalError] = useState<string | null>(null);
  const [showTokens, setShowTokens] = useState(false);
  const { copy, isCopied } = useCopyToClipboard();

  const handleLogin = async () => {
    setLocalError(null);

    if (!email || !password) {
      setLocalError('Email and password are required');
      return;
    }

    try {
      await loginWithEmail(email, password);
      setEmail('');
      setPassword('');
    } catch (err) {
      // Error is already set in the hook
    }
  };

  const handleMachineKeyLogin = async () => {
    setLocalError(null);
    try {
      await loginWithMachineKey();
    } catch (err) {
      // Error is already set in the hook
    }
  };

  const handleLogout = async () => {
    try {
      await logout();
    } catch (err) {
      // Error is already set in the hook
    }
  };

  const handleRefresh = async () => {
    try {
      await refreshToken();
    } catch (err) {
      // Error is already set in the hook
    }
  };

  const truncateToken = (token: string) => {
    if (token.length <= 20) return token;
    return token.slice(0, 10) + '...' + token.slice(-6);
  };

  // Logged In State
  if (remoteAuthState) {
    const expired = isTokenExpired();
    
    return (
      <div className={styles.panel}>
        <div className={styles.header}>
          <User size={20} />
          <Text variant="heading">ZERO ID</Text>
          <Label variant={expired ? 'warning' : 'success'} className={styles.statusLabel}>
            {expired ? 'Expired' : 'Connected'}
          </Label>
        </div>

        <div className={styles.content}>
          <Card className={styles.userCard}>
            <CardItem className={styles.userItem}>
              <div className={styles.userAvatar}>
                <Shield size={20} />
              </div>
              <div className={styles.userInfo}>
                <div className={styles.userKey}>{remoteAuthState.userKey}</div>
                <div className={styles.expiresInfo}>
                  <Clock size={10} />
                  <span>Expires: {getTimeRemaining()}</span>
                </div>
              </div>
            </CardItem>
          </Card>

          <ButtonCollapsible
            label="Session Tokens"
            expanded={showTokens}
            onToggle={() => setShowTokens(!showTokens)}
          >
            <div className={styles.tokensSection}>
              <div className={styles.tokenItem}>
                <Label variant="default">Access Token</Label>
                <div className={styles.tokenValue}>
                  <code>{truncateToken(remoteAuthState.accessToken)}</code>
                  <Button
                    variant="ghost"
                    onClick={() => copy(remoteAuthState.accessToken, 'access')}
                    className={styles.copyButton}
                  >
                    {isCopied('access') ? (
                      <Check size={12} className={styles.checkIcon} />
                    ) : (
                      <Copy size={12} />
                    )}
                  </Button>
                </div>
              </div>

              {remoteAuthState.refreshToken && (
                <div className={styles.tokenItem}>
                  <Label variant="default">Refresh Token</Label>
                  <div className={styles.tokenValue}>
                    <code>{truncateToken(remoteAuthState.refreshToken)}</code>
                    <Button
                      variant="ghost"
                      onClick={() => copy(remoteAuthState.refreshToken!, 'refresh')}
                      className={styles.copyButton}
                    >
                      {isCopied('refresh') ? (
                        <Check size={12} className={styles.checkIcon} />
                      ) : (
                        <Copy size={12} />
                      )}
                    </Button>
                  </div>
                </div>
              )}

              <div className={styles.tokenItem}>
                <Label variant="default">Scopes</Label>
                <div className={styles.scopesList}>
                  {remoteAuthState.scopes.map((scope, i) => (
                    <Label key={i} variant="default" className={styles.scopeBadge}>
                      {scope}
                    </Label>
                  ))}
                </div>
              </div>
            </div>
          </ButtonCollapsible>

          <div className={styles.actions}>
            {expired && remoteAuthState.refreshToken && (
              <Button
                variant="secondary"
                onClick={handleRefresh}
                disabled={isAuthenticating}
                className={styles.refreshButton}
              >
                {isAuthenticating ? <Spinner size="small" /> : <Key size={14} />}
                Refresh Token
              </Button>
            )}
            <Button
              variant="ghost"
              onClick={handleLogout}
              disabled={isAuthenticating}
              className={styles.logoutButton}
            >
              <LogOut size={14} />
              Logout
            </Button>
          </div>

          {error && (
            <Label variant="error" className={styles.error}>
              {error}
            </Label>
          )}
        </div>
      </div>
    );
  }

  // Login State
  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <User size={20} />
        <Text variant="heading">Login w/ ZERO ID</Text>
      </div>

      <div className={styles.content}>
        <div className={styles.formGroup}>
          <Label variant="default">Email</Label>
          <Input
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="you@example.com"
            disabled={isAuthenticating}
          />
        </div>

        <div className={styles.formGroup}>
          <Label variant="default">Password</Label>
          <Input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="Enter password"
            disabled={isAuthenticating}
            onKeyDown={(e) => e.key === 'Enter' && handleLogin()}
          />
        </div>

        <Button
          variant="primary"
          onClick={handleLogin}
          disabled={isAuthenticating || !email || !password}
          className={styles.loginButton}
        >
          {isAuthenticating ? <Spinner size="small" /> : <LogIn size={14} />}
          Login
        </Button>

        <div className={styles.divider}>
          <span>or</span>
        </div>

        <Button
          variant="secondary"
          onClick={handleMachineKeyLogin}
          disabled={isAuthenticating}
          className={styles.machineKeyButton}
        >
          <Key size={14} />
          Login with Machine Key
        </Button>

        {(localError || error) && (
          <Label variant="error" className={styles.error}>
            {localError || error}
          </Label>
        )}
      </div>
    </div>
  );
}
