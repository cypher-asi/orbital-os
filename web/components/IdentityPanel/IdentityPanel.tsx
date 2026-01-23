import { useEffect, useRef } from 'react';
import { Panel, Menu, type MenuItem } from '@cypher-asi/zui';
import { Brain, Cpu, Info, Layers, User, Users, Lock, LogOut, Clock } from 'lucide-react';
import { useIdentity, formatUserId, getSessionTimeRemaining } from '../../desktop/hooks/useIdentity';
import styles from './IdentityPanel.module.css';

interface IdentityPanelProps {
  onClose: () => void;
}

const NAV_ITEMS: MenuItem[] = [
  { id: 'neural-key', label: 'Neural Key', icon: <Brain size={14} /> },
  { id: 'machine-keys', label: 'Machine Keys', icon: <Cpu size={14} /> },
  { id: 'linked-accounts', label: 'Linked Accounts', icon: <Users size={14} /> },
  { id: 'vault', label: 'Vault', icon: <Lock size={14} /> },
  { id: 'information', label: 'Information', icon: <Info size={14} /> },
  { type: 'separator' },
  { id: 'login-zero-id', label: 'Login w/ ZERO ID', icon: <User size={14} /> },
  { type: 'separator' },
  { id: 'logout', label: 'Logout', icon: <LogOut size={14} /> },
];

// Simple avatar component
function Avatar({ name }: { size?: string; status?: string; name: string }) {
  return (
    <div className={styles.avatar}>
      <User size={20} />
    </div>
  );
}


export function IdentityPanel({ onClose }: IdentityPanelProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const identity = useIdentity();

  // Get current user info from identity service
  const currentUser = identity?.state.currentUser;
  const currentSession = identity?.state.currentSession;

  // Compute display values
  const displayName = currentUser?.displayName ?? 'Not logged in';
  const displayUid = currentUser ? formatUserId(currentUser.id) : '---';
  const sessionInfo = currentSession ? getSessionTimeRemaining(currentSession) : 'No session';

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (panelRef.current && !panelRef.current.contains(event.target as Node)) {
        onClose();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [onClose]);

  const handleSelect = async (id: string) => {
    console.log('[identity-panel] Selected:', id);
    if (id === 'logout' && identity) {
      try {
        await identity.logout();
        console.log('[identity-panel] Logout successful');
      } catch (error) {
        console.error('[identity-panel] Logout failed:', error);
      }
      onClose();
    }
  };

  return (
    <div ref={panelRef} className={styles.panelWrapper}>
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
          <Avatar name={displayName} />
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
          <Menu items={NAV_ITEMS} onChange={handleSelect} />
        </div>
      </Panel>
    </div>
  );
}
