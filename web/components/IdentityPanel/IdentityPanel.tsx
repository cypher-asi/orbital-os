import { useEffect, useRef, type ReactNode } from 'react';
import { Panel } from '@cypher-asi/zui';
import { Brain, Cpu, Info, Layers, User } from 'lucide-react';
import styles from './IdentityPanel.module.css';

interface IdentityPanelProps {
  onClose: () => void;
}

// Mock user data
const MOCK_USER = {
  name: 'CYPHER_01',
  uid: 'UID-7A3F-9B2E-4D1C-8E5F',
};

const NAV_ITEMS = [
  { id: 'neural-key', label: 'Neural Key', icon: <Brain size={14} /> },
  { id: 'machine-keys', label: 'Machine Keys', icon: <Cpu size={14} /> },
  { id: 'information', label: 'Information', icon: <Info size={14} /> },
];

// Simple avatar component
function Avatar({ name }: { size?: string; status?: string; name: string }) {
  return (
    <div className={styles.avatar}>
      <User size={20} />
    </div>
  );
}

// Simple menu component  
function Menu({ items, onSelect }: { items: Array<{id: string; label: string; icon: ReactNode}>; onSelect: (id: string) => void; variant?: string }) {
  return (
    <div className={styles.menu}>
      {items.map((item) => (
        <button key={item.id} className={styles.menuItem} onClick={() => onSelect(item.id)}>
          {item.icon}
          <span>{item.label}</span>
        </button>
      ))}
    </div>
  );
}

export function IdentityPanel({ onClose }: IdentityPanelProps) {
  const panelRef = useRef<HTMLDivElement>(null);

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

  const handleSelect = (id: string) => {
    console.log('[identity-panel] Selected:', id);
  };

  return (
    <div ref={panelRef} className={styles.panelWrapper}>
      <Panel className={styles.panel} variant="glass" border="future">
        {/* Section 1: Title */}
        <div className={styles.titleSection}>
          <h2 className={styles.title}>NEURAL LINK</h2>
        </div>

        {/* Section 2: Horizontal Image */}
        <div className={styles.imageSection}>
          <div className={styles.imagePlaceholder}>
            <Layers size={32} strokeWidth={1} />
          </div>
        </div>

        {/* Section 3: Profile Data */}
        <div className={styles.profileSection}>
          <Avatar name={MOCK_USER.name} />
          <div className={styles.userInfo}>
            <span className={styles.userName}>{MOCK_USER.name}</span>
            <span className={styles.userUid}>{MOCK_USER.uid}</span>
          </div>
        </div>

        {/* Section 4: Menu */}
        <div className={styles.menuSection}>
          <Menu items={NAV_ITEMS} onSelect={handleSelect} />
        </div>
      </Panel>
    </div>
  );
}
