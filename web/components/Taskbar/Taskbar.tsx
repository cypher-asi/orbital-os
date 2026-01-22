import { useState, useEffect, useCallback, useRef } from 'react';
import { Button } from '@cypher-asi/zui';
import { useWindows, useWindowActions } from '../../desktop/hooks/useWindows';
import { useDesktops, useDesktopActions } from '../../desktop/hooks/useDesktops';
import { BeginMenu } from '../BeginMenu/BeginMenu';
import { IdentityPanel } from '../IdentityPanel';
import { TerminalSquare, AppWindow, Circle, Plus, KeyRound, CreditCard } from 'lucide-react';
import styles from './Taskbar.module.css';

// Get the appropriate icon for a window based on its title
function getWindowIcon(title: string) {
  const lowerTitle = title.toLowerCase();
  if (lowerTitle.includes('terminal') || lowerTitle.includes('shell') || lowerTitle.includes('bash')) {
    return <TerminalSquare size={16} />;
  }
  // Default icon for other apps
  return <AppWindow size={16} />;
}

export function Taskbar() {
  const [beginMenuOpen, setBeginMenuOpen] = useState(false);
  const [identityPanelOpen, setIdentityPanelOpen] = useState(false);
  const beginSectionRef = useRef<HTMLDivElement>(null);
  const windows = useWindows();
  const desktops = useDesktops();
  const { focusWindow, panToWindow, restoreWindow } = useWindowActions();
  const { createDesktop, switchDesktop } = useDesktopActions();

  // Toggle begin menu with 'z' key when not in an input field
  const toggleBeginMenu = useCallback(() => {
    setBeginMenuOpen(prev => !prev);
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Only respond to 'z' key without modifiers
      if (e.key !== 'z' || e.ctrlKey || e.shiftKey || e.altKey || e.metaKey) {
        return;
      }

      // Ignore if focus is in an input field
      const target = e.target as HTMLElement;
      const tagName = target.tagName.toLowerCase();
      if (tagName === 'input' || tagName === 'textarea' || target.isContentEditable) {
        return;
      }

      e.preventDefault();
      toggleBeginMenu();
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [toggleBeginMenu]);

  const handleWindowClick = (e: React.MouseEvent, windowId: number, state: string, focused: boolean) => {
    e.stopPropagation(); // Prevent event from bubbling to Desktop
    if (state === 'minimized') {
      restoreWindow(windowId);
      // Always pan to minimized windows when restoring
      panToWindow(windowId);
    } else if (!focused) {
      // Only pan to unfocused windows - clicking an already-focused window
      // should not move the viewport (preserves user's current view)
      focusWindow(windowId);
      panToWindow(windowId);
    }
    // If already focused and not minimized, do nothing - user already sees this window
  };

  const handleAddDesktop = () => {
    const count = desktops.length;
    createDesktop(`Desktop ${count + 1}`);
  };

  return (
    <div className={styles.taskbar}>
      {/* Begin Button - Left */}
      <div ref={beginSectionRef} className={styles.beginSection}>
        <Button
          variant={beginMenuOpen ? 'glass' : 'transparent'}
          rounded="none"
          iconOnly
          className={`${styles.beginBtn} ${beginMenuOpen ? styles.beginBtnActive : ''}`}
          onClick={() => setBeginMenuOpen(!beginMenuOpen)}
          title="Begin Menu (Press Z)"
          selected={beginMenuOpen}
          selectedBgColor="transparent"
        >
          <span className={styles.beginIcon}>
            <Circle size={16} className={styles.beginCircle} />
            <span className={styles.beginSlash}>/</span>
          </span>
        </Button>

        {beginMenuOpen && <BeginMenu onClose={() => setBeginMenuOpen(false)} containerRef={beginSectionRef} />}
      </div>

      {/* Active Windows - Center */}
      <div className={styles.windowsSection}>
        {windows.map((win) => (
          <Button
            key={win.id}
            variant={win.focused ? 'glass' : 'transparent'}
            rounded="none"
            textCase="uppercase"
            icon={getWindowIcon(win.title)}
            className={`${styles.windowItem} ${win.state === 'minimized' ? styles.minimized : ''}`}
            onClick={(e) => handleWindowClick(e, win.id, win.state, win.focused)}
            title={win.title}
            selected={win.focused}
            selectedBgColor="transparent"
          >
            <span className={styles.windowTitle}>{win.title}</span>
          </Button>
        ))}
      </div>

      {/* Desktop Indicators - Right */}
      <div className={styles.workspacesSection}>
        {desktops.map((d, i) => (
          <Button
            key={d.id}
            variant={d.active ? 'glass' : 'transparent'}
            rounded="none"
            iconOnly
            className={styles.workspaceBtn}
            onClick={() => switchDesktop(i)}
            title={d.name}
            selected={d.active}
            selectedBgColor="transparent"
          >
            {i + 1}
          </Button>
        ))}
        <Button
          variant="transparent"
          rounded="none"
          iconOnly
          className={styles.workspaceAdd}
          onClick={handleAddDesktop}
          title="Add desktop"
          selected={false}
          selectedBgColor="transparent"
        >
          <Plus size={16} />
        </Button>
        <Button
          variant="transparent"
          rounded="none"
          iconOnly
          className={styles.walletBtn}
          onClick={() => console.log('[taskbar] Wallet clicked')}
          title="Wallet"
          selected={false}
          selectedBgColor="transparent"
        >
          <CreditCard size={16} />
        </Button>
        <div className={styles.neuralKeyWrapper}>
          <Button
            variant={identityPanelOpen ? 'glass' : 'transparent'}
            rounded="none"
            iconOnly
            className={styles.neuralKey}
            onClick={() => setIdentityPanelOpen(!identityPanelOpen)}
            title="Neural Link - Identity & Security"
            selected={identityPanelOpen}
            selectedBgColor="transparent"
          >
            <KeyRound size={16} />
          </Button>

          {identityPanelOpen && <IdentityPanel onClose={() => setIdentityPanelOpen(false)} />}
        </div>
      </div>
    </div>
  );
}
