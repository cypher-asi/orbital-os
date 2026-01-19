import { TerminalApp } from '../TerminalApp/TerminalApp';
import styles from './AppRouter.module.css';

interface AppRouterProps {
  appId: string;
  windowId: number;
}

// Placeholder components for apps that aren't implemented yet
function PlaceholderApp({ appId }: { appId: string }) {
  return (
    <div className={styles.placeholder}>
      <div className={styles.placeholderIcon}>🚧</div>
      <div className={styles.placeholderTitle}>{appId}</div>
      <div className={styles.placeholderText}>This app is not yet implemented</div>
    </div>
  );
}

export function AppRouter({ appId, windowId }: AppRouterProps) {
  switch (appId) {
    case 'terminal':
      return <TerminalApp windowId={windowId} />;
    case 'settings':
      return <PlaceholderApp appId="Settings" />;
    case 'files':
      return <PlaceholderApp appId="Files" />;
    default:
      return <PlaceholderApp appId={appId} />;
  }
}
