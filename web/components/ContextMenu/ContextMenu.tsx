import { useEffect, useRef, useState } from 'react';
import styles from './ContextMenu.module.css';

export interface MenuItem {
  id: string;
  label: string;
  icon?: string;
  disabled?: boolean;
  checked?: boolean;
  submenu?: MenuItem[];
  onClick?: () => void;
}

export interface ContextMenuProps {
  x: number;
  y: number;
  items: MenuItem[];
  onClose: () => void;
}

export function ContextMenu({ x, y, items, onClose }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  // Close on click outside or escape
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      // Only close if clicking outside the menu
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };

    // Use a small delay to let the menu render first, then add listener
    // Use click (not mousedown) to allow menu items to handle clicks first
    const timeoutId = setTimeout(() => {
      document.addEventListener('click', handleClickOutside);
      document.addEventListener('keydown', handleKeyDown);
    }, 10);

    return () => {
      clearTimeout(timeoutId);
      document.removeEventListener('click', handleClickOutside);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [onClose]);

  // Adjust position to keep menu on screen
  useEffect(() => {
    if (!menuRef.current) return;

    const menu = menuRef.current;
    const rect = menu.getBoundingClientRect();
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    let adjustedX = x;
    let adjustedY = y;

    // Adjust horizontal position
    if (x + rect.width > viewportWidth) {
      adjustedX = viewportWidth - rect.width - 8;
    }

    // Adjust vertical position
    if (y + rect.height > viewportHeight) {
      adjustedY = viewportHeight - rect.height - 8;
    }

    menu.style.left = `${Math.max(8, adjustedX)}px`;
    menu.style.top = `${Math.max(8, adjustedY)}px`;
  }, [x, y]);

  const handleItemClick = (item: MenuItem) => {
    if (item.disabled) return;
    item.onClick?.();
    onClose();
  };

  return (
    <div
      ref={menuRef}
      className={styles.contextMenu}
      style={{ left: x, top: y }}
      onContextMenu={(e) => e.preventDefault()}
      onPointerDown={(e) => e.stopPropagation()}
      onMouseDown={(e) => e.stopPropagation()}
    >
      {items.map((item, index) => {
        if (item.id === 'separator') {
          return <div key={`sep-${index}`} className={styles.separator} />;
        }

        return (
          <div
            key={item.id}
            className={`${styles.menuItem} ${item.disabled ? styles.disabled : ''} ${
              item.checked ? styles.checked : ''
            }`}
            onClick={() => handleItemClick(item)}
          >
            {item.checked && <span className={styles.checkmark}>✓</span>}
            <span className={styles.label}>{item.label}</span>
          </div>
        );
      })}
    </div>
  );
}

// Submenu for nested items (e.g., background selection)
export interface SubmenuProps {
  label: string;
  items: MenuItem[];
  parentRef?: React.RefObject<HTMLDivElement>;
}

export function ContextMenuWithSubmenu({
  x,
  y,
  items,
  onClose,
}: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      }
    };

    document.addEventListener('mousedown', handleClickOutside, true);
    document.addEventListener('keydown', handleKeyDown);

    return () => {
      document.removeEventListener('mousedown', handleClickOutside, true);
      document.removeEventListener('keydown', handleKeyDown);
    };
  }, [onClose]);

  return (
    <div
      ref={menuRef}
      className={styles.contextMenu}
      style={{ left: x, top: y }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item, index) => {
        if (item.id === 'separator') {
          return <div key={`sep-${index}`} className={styles.separator} />;
        }

        if (item.submenu) {
          return (
            <SubmenuItem
              key={item.id}
              item={item}
              onClose={onClose}
            />
          );
        }

        return (
          <div
            key={item.id}
            className={`${styles.menuItem} ${item.disabled ? styles.disabled : ''} ${
              item.checked ? styles.checked : ''
            }`}
            onClick={() => {
              if (!item.disabled) {
                item.onClick?.();
                onClose();
              }
            }}
          >
            {item.checked && <span className={styles.checkmark}>✓</span>}
            <span className={styles.label}>{item.label}</span>
          </div>
        );
      })}
    </div>
  );
}

function SubmenuItem({ item, onClose }: { item: MenuItem; onClose: () => void }) {
  const itemRef = useRef<HTMLDivElement>(null);
  const [showSubmenu, setShowSubmenu] = useState(false);
  const [submenuPosition, setSubmenuPosition] = useState({ x: 0, y: 0 });

  const handleMouseEnter = () => {
    if (!itemRef.current || !item.submenu) return;

    const rect = itemRef.current.getBoundingClientRect();
    setSubmenuPosition({
      x: rect.right,
      y: rect.top,
    });
    setShowSubmenu(true);
  };

  return (
    <div
      ref={itemRef}
      className={`${styles.menuItem} ${styles.hasSubmenu}`}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={() => setShowSubmenu(false)}
    >
      <span className={styles.label}>{item.label}</span>
      <span className={styles.submenuArrow}>▶</span>

      {showSubmenu && item.submenu && (
        <div
          className={styles.submenu}
          style={{ left: submenuPosition.x, top: submenuPosition.y }}
        >
          {item.submenu.map((subitem) => (
            <div
              key={subitem.id}
              className={`${styles.menuItem} ${subitem.disabled ? styles.disabled : ''} ${
                subitem.checked ? styles.checked : ''
              }`}
              onClick={() => {
                if (!subitem.disabled) {
                  subitem.onClick?.();
                  onClose();
                }
              }}
            >
              {subitem.checked && <span className={styles.checkmark}>✓</span>}
              <span className={styles.label}>{subitem.label}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

