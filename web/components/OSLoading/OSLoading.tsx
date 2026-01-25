import { FC } from 'react';
import styles from './OSLoading.module.css';

export interface OSLoadingProps {
  progress: number;
  status: string;
  error?: string;
}

export const OSLoading: FC<OSLoadingProps> = ({ progress, status, error }) => {
  return (
    <div className={styles.overlay}>
      <div className={styles.container}>
        <div className={styles.progressSection}>
          <div className={styles.percentage}>{Math.round(progress)}%</div>
          <div className={styles.progressBar}>
            <div className={styles.progressTrack}>
              <div className={styles.progressFill} style={{ width: `${progress}%` }} />
            </div>
          </div>
          <div className={error ? styles.statusError : styles.status}>{error || status}</div>
        </div>
      </div>
    </div>
  );
};
