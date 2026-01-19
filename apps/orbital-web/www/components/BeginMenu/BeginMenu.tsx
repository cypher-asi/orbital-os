import { useEffect, useRef } from 'react';
import { useWindowActions } from '../../hooks/useWindows';
import { useSupervisor } from '../../hooks/useSupervisor';
import { Menu } from '@cypher-asi/zui';
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

    // Use mousedown to catch the click before it bubbles
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
      <Menu
        title="ZERO OS"
        items={MENU_ITEMS}
        onSelect={handleSelect}
        variant="glass"
        border="future"
        width={200}
      />
    </div>
  );
}
