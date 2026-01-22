import { useEffect, useRef } from 'react';
import { useWindowActions } from '../../desktop/hooks/useWindows';
import { useSupervisor } from '../../desktop/hooks/useSupervisor';
import { Menu, type MenuItem } from '@cypher-asi/zui';
import styles from './BeginMenu.module.css';

interface BeginMenuProps {
  onClose: () => void;
  containerRef?: React.RefObject<HTMLDivElement>;
}

// Programs submenu items (alphabetically sorted)
const PROGRAM_ITEMS = [
  { id: 'calculator', label: 'Calculator' },
  { id: 'clock', label: 'Clock' },
];

// Settings submenu items
const SETTINGS_ITEMS = [
  { id: 'permissions', label: 'Permissions' },
];

// Main menu structure
const MENU_ITEMS: MenuItem[] = [
  {
    id: 'programs',
    label: 'Programs',
    children: PROGRAM_ITEMS,
  },
  { id: 'terminal', label: 'Terminal' },
  { id: 'files', label: 'Files' },
  {
    id: 'settings',
    label: 'Settings',
    children: SETTINGS_ITEMS,
  },
  { type: 'separator' },
  { id: 'shutdown', label: 'Shutdown' },
];

export function BeginMenu({ onClose, containerRef }: BeginMenuProps) {
  const { launchApp, launchTerminal } = useWindowActions();
  const supervisor = useSupervisor();
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      const target = event.target as Node;
      // Ignore clicks inside the menu
      if (menuRef.current && menuRef.current.contains(target)) {
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

  const handleSelect = async (id: string) => {
    // Skip parent menu items (submenus)
    if (id === 'programs' || id === 'settings') return;

    if (id === 'shutdown') {
      onClose();
      if (supervisor) {
        supervisor.send_input('shutdown');
      }
    } else if (id === 'terminal') {
      // Terminal uses special spawn-and-link flow
      onClose();
      await launchTerminal();
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
        onChange={handleSelect}
        variant="glass"
        border="future"
        rounded="md"
        width={200}
      />
    </div>
  );
}
