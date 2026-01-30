import { useCallback, type ReactNode } from 'react';
import { Menu, type MenuItem, Avatar, type PanelDrillItem } from '@cypher-asi/zui';
import {
  Info,
  Layers,
  User,
  Lock,
  LogOut,
  Clock,
  Brain,
  Cpu,
  Link,
  Mail,
  Globe,
  Fingerprint,
  RotateCcw,
  Key,
} from 'lucide-react';
import {
  useIdentityStore,
  selectCurrentUser,
  selectCurrentSession,
  formatUserId,
  getSessionTimeRemaining,
  formatLoginType,
  useSettingsStore,
} from '@/stores';
import { useZeroIdAuth } from '../../hooks/useZeroIdAuth';
import { useWindowActions } from '../../hooks/useWindows';
import { usePanelDrillOptional } from './context';
import { ZeroIdLoginPanel } from './panels/ZeroIdLoginPanel';
import styles from './IdentityPanel.module.css';

import type { LoginType } from '@/stores';

/** Get the appropriate icon for a login type */
function getLoginTypeIcon(loginType: LoginType, size = 10) {
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

interface IdentityPanelContentProps {
  onClose: () => void;
  onShowLoginModal: () => void;
  /** Optional push panel function for standalone use (fallback if not in PanelDrillProvider) */
  onPushPanel?: (item: PanelDrillItem) => void;
}

/**
 * IdentityPanelContent - The root panel content for IdentityPanel
 *
 * Contains: image section, profile section, menu section
 * Uses PanelDrill context for drill-down navigation when available.
 */
export function IdentityPanelContent({
  onClose,
  onShowLoginModal,
  onPushPanel,
}: IdentityPanelContentProps) {
  // Use Zustand store directly for better performance
  const identityStore = useIdentityStore();
  const currentUser = useIdentityStore(selectCurrentUser);
  const currentSession = useIdentityStore(selectCurrentSession);

  const { remoteAuthState, disconnect: disconnectZeroId } = useZeroIdAuth();
  const { launchOrFocusApp } = useWindowActions();
  const setPendingNavigation = useSettingsStore((state) => state.setPendingNavigation);

  // PanelDrill context (optional - use prop fallback if not in context)
  const panelDrill = usePanelDrillOptional();
  const pushPanel = panelDrill?.pushPanel ?? onPushPanel;

  // Compute display values
  const displayName = currentUser?.displayName ?? 'Not logged in';
  const displayUid = currentUser ? formatUserId(currentUser.id) : '---';
  const sessionInfo = currentSession ? getSessionTimeRemaining(currentSession) : 'No session';
  const loginTypeLabel = currentSession ? formatLoginType(currentSession.loginType) : null;

  // Check if logged into ZERO ID
  const isZeroIdConnected = !!remoteAuthState;

  // Open Settings app at Identity section (with optional sub-panel)
  const openIdentitySettings = useCallback(
    (subPanel?: string) => {
      // Set pending navigation via store (handles both cases - Settings open or closed)
      setPendingNavigation({
        area: 'identity',
        subPanel: subPanel as 'neural-key' | 'machine-keys' | 'linked-accounts' | undefined,
      });
      // Launch or focus the Settings app
      launchOrFocusApp('settings');
      // Close the identity panel
      onClose();
    },
    [setPendingNavigation, launchOrFocusApp, onClose]
  );

  // Handle menu selection - open subpanel overlay
  const handleSelect = useCallback(
    async (id: string) => {
      console.log('[identity-panel] Selected:', id);

      let subPanelContent: ReactNode = null;
      let subPanelLabel = '';

      switch (id) {
        case 'neural-key':
          // Open Settings > Identity > Neural Key
          openIdentitySettings('neural-key');
          return;

        case 'machine-keys':
          // Open Settings > Identity > Machine Keys
          openIdentitySettings('machine-keys');
          return;

        case 'linked-accounts':
          // Open Settings > Identity > Linked Accounts
          openIdentitySettings('linked-accounts');
          return;

        case 'login-zero-id':
          if (isZeroIdConnected) {
            // Show connected status in subpanel using PanelDrill
            subPanelLabel = 'Session';
            subPanelContent = <ZeroIdLoginPanel key="login-zero-id" />;
          } else {
            // Show centered login modal when not connected
            onShowLoginModal();
            return; // Don't open subpanel
          }
          break;

        case 'logout':
          try {
            // Disconnect from ZERO ID if connected (remote session only)
            if (isZeroIdConnected) {
              await disconnectZeroId();
              console.log('[identity-panel] ZERO ID disconnect successful');
            }
            // Logout from local identity
            await identityStore.logout();
            console.log('[identity-panel] Local logout successful');
          } catch (error) {
            console.error('[identity-panel] Logout failed:', error);
          }
          onClose();
          return;

        default:
          console.log('[identity-panel] Unhandled menu item:', id);
          return;
      }

      if (subPanelContent && pushPanel) {
        // Use pushPanel to add the subpanel
        pushPanel({ id, label: subPanelLabel, content: subPanelContent });
      }
    },
    [
      identityStore,
      onClose,
      isZeroIdConnected,
      disconnectZeroId,
      openIdentitySettings,
      pushPanel,
      onShowLoginModal,
    ]
  );

  // Dynamic nav items based on ZERO ID connection state
  const navItems: MenuItem[] = [
    // Identity settings shortcuts (open Settings > Identity)
    { id: 'neural-key', label: 'Neural Key', icon: <Brain size={14} /> },
    { id: 'machine-keys', label: 'Machine Keys', icon: <Cpu size={14} /> },
    { id: 'linked-accounts', label: 'Linked Accounts', icon: <Link size={14} /> },
    { id: 'vault', label: 'Vault', icon: <Lock size={14} />, disabled: true },
    { id: 'information', label: 'Information', icon: <Info size={14} />, disabled: true },
    { type: 'separator' },
    {
      id: 'login-zero-id',
      label: isZeroIdConnected ? 'Connected' : 'Login',
      icon: isZeroIdConnected ? (
        <div className={styles.connectedIndicator}>
          <User size={14} />
        </div>
      ) : (
        <User size={14} />
      ),
    },
    { type: 'separator' },
    { id: 'logout', label: 'Logout', icon: <LogOut size={14} /> },
  ];

  return (
    <div className={styles.contentWrapper}>
      {/* Section 1: Horizontal Image */}
      <div className={styles.imageSection}>
        <div className={styles.imagePlaceholder}>
          <Layers size={32} strokeWidth={1} />
        </div>
      </div>

      {/* Section 2: Profile Data */}
      <div className={styles.profileSection}>
        <Avatar name={displayName} icon size="lg" />
        <div className={styles.userInfo}>
          <span className={styles.userName}>{displayName}</span>
          <span className={styles.userUid}>{displayUid}</span>
          {currentSession && (
            <>
              <span className={styles.sessionInfo}>
                {getLoginTypeIcon(currentSession.loginType)} {loginTypeLabel}
              </span>
              <span className={styles.sessionInfo}>
                <Clock size={10} /> {sessionInfo}
              </span>
            </>
          )}
        </div>
      </div>

      {/* Section 3: Menu */}
      <div className={styles.menuSection}>
        <Menu items={navItems} onChange={handleSelect} background="none" border="none" />
      </div>
    </div>
  );
}
