import { useEffect, useRef, useState, useMemo, useCallback, type ReactNode } from 'react';
import {
  Panel,
  PanelDrill,
  type PanelDrillItem,
  Menu,
  type MenuItem,
  Avatar,
} from '@cypher-asi/zui';
import { Info, Layers, User, Lock, LogOut, Clock, Brain, Cpu, Link } from 'lucide-react';
import {
  useIdentityStore,
  selectCurrentUser,
  selectCurrentSession,
  formatUserId,
  getSessionTimeRemaining,
  useSettingsStore,
} from '@/stores';
import { useZeroIdAuth } from '../../hooks/useZeroIdAuth';
import { useWindowActions } from '../../hooks/useWindows';
import { ZeroIdLoginPanel } from './panels/ZeroIdLoginPanel';
import { LoginModal } from './modals';
import styles from './IdentityPanel.module.css';

interface IdentityPanelProps {
  onClose: () => void;
  containerRef?: React.RefObject<HTMLDivElement>;
}

/** Format a ZERO ID user key for display */
function formatUserKey(key: string): string {
  // Already formatted like UID-XXXX-XXXX-XXXX, just truncate
  if (key.length > 16) {
    return key.slice(0, 16) + '...';
  }
  return key;
}

export function IdentityPanel({ onClose, containerRef }: IdentityPanelProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  // Use Zustand store directly for better performance
  const identityStore = useIdentityStore();
  const currentUser = useIdentityStore(selectCurrentUser);
  const currentSession = useIdentityStore(selectCurrentSession);

  const { remoteAuthState, disconnect: disconnectZeroId } = useZeroIdAuth();
  const { launchOrFocusApp } = useWindowActions();
  const setPendingNavigation = useSettingsStore((state) => state.setPendingNavigation);

  // Stack state for drill-down navigation (subpanel overlay)
  const [stack, setStack] = useState<PanelDrillItem[]>([]);
  const isSubpanelOpen = stack.length > 0;

  // Modal state for centered login
  const [showLoginModal, setShowLoginModal] = useState(false);

  // Compute display values
  const displayName = currentUser?.displayName ?? 'Not logged in';
  const displayUid = currentUser ? formatUserId(currentUser.id) : '---';
  const sessionInfo = currentSession ? getSessionTimeRemaining(currentSession) : 'No session';

  // Check if logged into ZERO ID
  const isZeroIdConnected = !!remoteAuthState;
  const zeroIdUserKey = remoteAuthState?.userKey;

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
            // Show connected status in subpanel
            subPanelLabel = 'ZERO ID';
            subPanelContent = (
              <ZeroIdLoginPanel key="login-zero-id" onClose={() => setStack([])} />
            );
          } else {
            // Show centered login modal when not connected
            setShowLoginModal(true);
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

      if (subPanelContent) {
        // Set the subpanel stack with root item for breadcrumb navigation
        setStack([
          { id: 'identity', label: 'Identity', content: null },
          { id, label: subPanelLabel, content: subPanelContent },
        ]);
      }
    },
    [identityStore, onClose, isZeroIdConnected, disconnectZeroId, openIdentitySettings]
  );

  // Handle breadcrumb navigation within subpanel
  const handleNavigate = useCallback((_id: string, index: number) => {
    if (index === 0) {
      // Navigating to root "Identity" - close the subpanel
      setStack([]);
    } else {
      // Trim stack to this point (for nested navigation in future)
      setStack((prev) => prev.slice(0, index + 1));
    }
  }, []);

  // Click outside handler
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node;
      // Ignore clicks inside the panel
      if (panelRef.current && panelRef.current.contains(target)) {
        return;
      }
      // Ignore clicks on the container (includes the toggle button)
      if (containerRef?.current && containerRef.current.contains(target)) {
        return;
      }
      onClose();
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [onClose, containerRef]);

  // Dynamic nav items based on ZERO ID connection state
  const navItems: MenuItem[] = useMemo(
    () => [
      // Identity settings shortcuts (open Settings > Identity)
      { id: 'neural-key', label: 'Neural Key', icon: <Brain size={14} /> },
      { id: 'machine-keys', label: 'Machine Keys', icon: <Cpu size={14} /> },
      { id: 'linked-accounts', label: 'Linked Accounts', icon: <Link size={14} /> },
      { id: 'vault', label: 'Vault', icon: <Lock size={14} />, disabled: true },
      { id: 'information', label: 'Information', icon: <Info size={14} />, disabled: true },
      { type: 'separator' },
      {
        id: 'login-zero-id',
        label: isZeroIdConnected ? `Connected ${formatUserKey(zeroIdUserKey || '')}` : 'Login',
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
    ],
    [isZeroIdConnected, zeroIdUserKey]
  );

  return (
    <div ref={panelRef} className={styles.panelWrapper}>
      {/* Main Panel - Always Present */}
      <Panel className={styles.panel} variant="glass" border="future">
        {/* Section 1: Title */}
        <div className={styles.titleSection}>
          <h2 className={styles.title}>IDENTITY</h2>
        </div>

        {/* Section 2: Horizontal Image */}
        <div className={styles.imageSection}>
          <div className={styles.imagePlaceholder}>
            <Layers size={32} strokeWidth={1} />
          </div>
        </div>

        {/* Section 3: Profile Data */}
        <div className={styles.profileSection}>
          <Avatar name={displayName} icon size="lg" />
          <div className={styles.userInfo}>
            <span className={styles.userName}>{displayName}</span>
            <span className={styles.userUid}>{displayUid}</span>
            {currentSession && (
              <span className={styles.sessionInfo}>
                <Clock size={10} /> {sessionInfo}
              </span>
            )}
          </div>
        </div>

        {/* Section 4: Menu */}
        <div className={styles.menuSection}>
          <Menu
            items={navItems}
            onChange={handleSelect}
            background="none"
            border="none"
          />
        </div>
      </Panel>

      {/* Subpanel Overlay - Slides in over main panel */}
      {isSubpanelOpen && (
        <div className={styles.subpanelOverlay}>
          <PanelDrill
            stack={stack}
            onNavigate={handleNavigate}
            className={styles.subpanel}
            background="none"
            border="none"
          />
        </div>
      )}

      {/* Centered Login Modal - Shows when not connected */}
      {showLoginModal && <LoginModal onClose={() => setShowLoginModal(false)} />}
    </div>
  );
}
