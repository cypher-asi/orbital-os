import { useEffect, useRef } from 'react';
import { useWindowActions } from '../../desktop/hooks/useWindows';
import { useSupervisor } from '../../desktop/hooks/useSupervisor';
import { Panel } from '@cypher-asi/zui';
import { TerminalSquare, Settings, Folder, Power } from 'lucide-react';
import styles from './BeginMenu.module.css';

interface BeginMenuProps {
  onClose: () => void;
}

const MENU_ITEMS = [
  { id: 'terminal', label: 'Terminal', icon: <TerminalSquare size={14} /> },
  { id: 'settings', label: 'Settings', icon: <Settings size={14} /> },
  { id: 'files', label: 'Files', icon: <Folder size={14} /> },
  { id: 'shutdown', label: 'Shutdown', icon: <Power size={14} /> },
];

export function BeginMenu({ onClose }: BeginMenuProps) {
  const { launchApp } = useWindowActions();
  const supervisor = useSupervisor();
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(event.target as Node)) {
        onClose();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [onClose]);

  const handleSelect = (id: string) => {
    if (id === 'shutdown') {
      onClose();
      if (supervisor) {
        supervisor.send_input('shutdown');
      }
    } else {
      launchApp(id);
      onClose();
    }
  };

  return (
    <div ref={menuRef} className={styles.menuWrapper}>
      <Panel className={styles.menu} variant="glass" border="future">
        <div className={styles.menuTitle}>ZERO OS</div>
        {MENU_ITEMS.map((item) => (
          <button
            key={item.id}
            className={styles.menuItem}
            onClick={() => handleSelect(item.id)}
          >
            {item.icon}
            <span>{item.label}</span>
          </button>
        ))}
      </Panel>
    </div>
  );
}
