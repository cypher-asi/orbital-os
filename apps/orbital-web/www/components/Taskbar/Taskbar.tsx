import { useState, useEffect, useCallback } from 'react';
import { useWindows, useWindowActions } from '../../hooks/useWindows';
import { useWorkspaces, useWorkspaceActions } from '../../hooks/useWorkspaces';
import { BeginMenu } from '../BeginMenu/BeginMenu';
import { IdentityPanel } from '../IdentityPanel';
import { Button } from '@cypher-asi/zui';
import { TerminalSquare, AppWindow, Circle, Plus, KeyRound, CreditCard } from 'lucide-react';
import styles from './Taskbar.module.css';

// Get the appropriate icon for a window based on its title
function getWindowIcon(title: string) {
  const lowerTitle = title.toLowerCase();
  if (lowerTitle.includes('terminal') || lowerTitle.includes('shell') || lowerTitle.includes('bash')) {
    return <TerminalSquare size={14} />;
  }
  // Default icon for other apps
  return <AppWindow size={14} />;
}

export function Taskbar() {
  const [beginMenuOpen, setBeginMenuOpen] = useState(false);
  const [identityPanelOpen, setIdentityPanelOpen] = useState(false);
  const windows = useWindows();
  const workspaces = useWorkspaces();
  const { focusWindow, panToWindow, restoreWindow } = useWindowActions();
  const { createWorkspace, switchWorkspace } = useWorkspaceActions();

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

  const handleAddWorkspace = () => {
    const count = workspaces.length;
    createWorkspace(`Workspace ${count + 1}`);
  };

  return (
    <div className={styles.taskbar}>
      {/* Begin Button - Left */}
      <div className={styles.beginSection}>
        <Button
          variant={beginMenuOpen ? 'glass' : 'transparent'}
          size="sm"
          rounded="none"
          textCase="uppercase"
          icon={<Circle size={14} />}
          onClick={() => setBeginMenuOpen(!beginMenuOpen)}
        >
          Begin
        </Button>

        {beginMenuOpen && <BeginMenu onClose={() => setBeginMenuOpen(false)} />}
      </div>

      {/* Active Windows - Center */}
      <div className={styles.windowsSection}>
        {windows.map((win) => (
          <Button
            key={win.id}
            variant={win.focused ? 'glass' : 'transparent'}
            size="sm"
            rounded="none"
            textCase="uppercase"
            icon={getWindowIcon(win.title)}
            className={`${styles.windowItem} ${win.state === 'minimized' ? styles.minimized : ''}`}
            onClick={(e) => handleWindowClick(e, win.id, win.state, win.focused)}
            title={win.title}
          >
            <span className={styles.windowTitle}>{win.title}</span>
          </Button>
        ))}
      </div>

      {/* Workspace Indicators - Right */}
      <div className={styles.workspacesSection}>
        {workspaces.map((ws, i) => (
          <Button
            key={ws.id}
            variant={ws.active ? 'glass' : 'transparent'}
            size="sm"
            rounded="none"
            iconOnly
            className={styles.workspaceBtn}
            onClick={() => switchWorkspace(i)}
            title={ws.name}
          >
            {i + 1}
          </Button>
        ))}
        <Button
          variant="transparent"
          size="sm"
          rounded="none"
          iconOnly
          className={styles.workspaceAdd}
          onClick={handleAddWorkspace}
          title="Add workspace"
        >
          <Plus size={14} />
        </Button>
        <Button
          variant="transparent"
          size="sm"
          rounded="none"
          iconOnly
          className={styles.walletBtn}
          onClick={() => console.log('[taskbar] Wallet clicked')}
          title="Wallet"
        >
          <CreditCard size={14} />
        </Button>
        <div className={styles.neuralKeyWrapper}>
          <Button
            variant={identityPanelOpen ? 'glass' : 'transparent'}
            size="sm"
            rounded="none"
            iconOnly
            className={styles.neuralKey}
            onClick={() => setIdentityPanelOpen(!identityPanelOpen)}
            title="Neural Link - Identity & Security"
          >
            <KeyRound size={14} />
          </Button>

          {identityPanelOpen && <IdentityPanel onClose={() => setIdentityPanelOpen(false)} />}
        </div>
      </div>
    </div>
  );
}
