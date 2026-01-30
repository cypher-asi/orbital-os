import { useEffect, useRef, useState, useCallback } from 'react';
import { PanelDrill, type PanelDrillItem } from '@cypher-asi/zui';
import { PanelDrillProvider } from './context';
import { IdentityPanelContent } from './IdentityPanelContent';
import { LoginModal } from './modals';
import styles from './IdentityPanel.module.css';

interface IdentityPanelProps {
  onClose: () => void;
  containerRef?: React.RefObject<HTMLDivElement>;
}

export function IdentityPanel({ onClose, containerRef }: IdentityPanelProps) {
  const panelRef = useRef<HTMLDivElement>(null);

  // Modal state for centered login
  const [showLoginModal, setShowLoginModal] = useState(false);

  // Initialize stack with root item (content will be populated in useEffect)
  const [stack, setStack] = useState<PanelDrillItem[]>(() => [
    { id: 'identity', label: 'Identity', content: null },
  ]);

  // Navigate back one level in the stack
  const navigateBack = useCallback(() => {
    setStack((prev) => {
      if (prev.length <= 1) return prev;
      return prev.slice(0, -1);
    });
  }, []);

  // Push a new panel onto the stack
  const pushPanel = useCallback((item: PanelDrillItem) => {
    setStack((prev) => [...prev, item]);
  }, []);

  // Handle breadcrumb navigation
  const handleNavigate = useCallback((_id: string, index: number) => {
    setStack((prev) => prev.slice(0, index + 1));
  }, []);

  // Populate root panel content after mount (following SettingsApp pattern)
  useEffect(() => {
    setStack((prev) => {
      if (prev.length === 1 && prev[0].content === null) {
        return [
          {
            ...prev[0],
            content: (
              <IdentityPanelContent
                onClose={onClose}
                onShowLoginModal={() => setShowLoginModal(true)}
                onPushPanel={pushPanel}
              />
            ),
          },
        ];
      }
      return prev;
    });
  }, [onClose, pushPanel]);

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

  return (
    <div ref={panelRef} className={styles.panelWrapper}>
      <PanelDrillProvider onNavigateBack={navigateBack} onPushPanel={pushPanel}>
        <PanelDrill
          stack={stack}
          onNavigate={handleNavigate}
          showBreadcrumb={true}
          className={styles.panel}
          variant="glass"
          border="future"
        />
      </PanelDrillProvider>

      {/* Centered Login Modal - Shows when not connected */}
      {showLoginModal && <LoginModal onClose={() => setShowLoginModal(false)} />}
    </div>
  );
}
