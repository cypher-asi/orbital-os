import {
  ShieldCheck,
  RefreshCw,
  Loader,
  CheckCircle,
  XCircle,
  AlertCircle,
  Wifi,
} from 'lucide-react';
import styles from './NeuralKeyPanel.module.css';

interface SetupStatusSectionProps {
  hasMachineKey: boolean;
  isEnrolled: boolean;
  sessionStatus: 'none' | 'expired' | 'active';
  enrollmentRetrying: boolean;
  sessionRetrying: boolean;
  enrollmentError: string | null;
  sessionError: string | null;
  isAuthenticating: boolean;
  getTimeRemaining: () => string;
  onRetryEnrollment: () => void;
  onRetrySession: () => void;
}

/**
 * Setup Status Section
 * 
 * Displays the status of:
 * - Machine Key (created / not found)
 * - ZID Enrollment (verified / not connected)
 * - Session (active with time remaining / expired / not connected)
 */
export function SetupStatusSection({
  hasMachineKey,
  isEnrolled,
  sessionStatus,
  enrollmentRetrying,
  sessionRetrying,
  enrollmentError,
  sessionError,
  isAuthenticating,
  getTimeRemaining,
  onRetryEnrollment,
  onRetrySession,
}: SetupStatusSectionProps) {
  return (
    <div className={styles.setupStatusSection}>
      <div className={styles.setupStatusHeader}>
        <ShieldCheck size={14} />
        <span className={styles.setupStatusTitle}>Setup Status</span>
      </div>

      {/* Machine Key Status */}
      <div className={styles.setupStatusRow}>
        <div className={`${styles.setupStatusIcon} ${hasMachineKey ? styles.setupStatusIconSuccess : styles.setupStatusIconError}`}>
          {hasMachineKey ? <CheckCircle size={16} /> : <XCircle size={16} />}
        </div>
        <span className={styles.setupStatusLabel}>Machine Key</span>
        <div className={styles.setupStatusValue}>
          {hasMachineKey ? 'Created' : 'Not Found'}
        </div>
      </div>

      {/* ZID Enrollment Status */}
      <div className={styles.setupStatusRow}>
        <div className={`${styles.setupStatusIcon} ${isEnrolled ? styles.setupStatusIconSuccess : styles.setupStatusIconError}`}>
          {isEnrolled ? <CheckCircle size={16} /> : <XCircle size={16} />}
        </div>
        <span className={styles.setupStatusLabel}>ZERO ID</span>
        <div className={styles.setupStatusValue}>
          {enrollmentRetrying ? (
            <>
              <Loader size={12} className={styles.spinner} />
              Connecting...
            </>
          ) : isEnrolled ? (
            'Verified'
          ) : (
            <>
              {enrollmentError || 'Not Connected'}
              {hasMachineKey && (
                <button
                  className={styles.setupStatusRetryBtn}
                  onClick={onRetryEnrollment}
                  disabled={enrollmentRetrying || isAuthenticating}
                >
                  <RefreshCw size={12} />
                  Connect
                </button>
              )}
            </>
          )}
        </div>
      </div>

      {/* Session Status */}
      <div className={styles.setupStatusRow}>
        <div className={`${styles.setupStatusIcon} ${
          sessionStatus === 'active' 
            ? styles.setupStatusIconSuccess 
            : sessionStatus === 'expired' 
              ? styles.setupStatusIconWarning 
              : styles.setupStatusIconPending
        }`}>
          {sessionStatus === 'active' ? (
            <Wifi size={16} />
          ) : sessionStatus === 'expired' ? (
            <AlertCircle size={16} />
          ) : (
            <Wifi size={16} />
          )}
        </div>
        <span className={styles.setupStatusLabel}>Session</span>
        <div className={styles.setupStatusValue}>
          {sessionRetrying ? (
            <>
              <Loader size={12} className={styles.spinner} />
              Connecting...
            </>
          ) : sessionStatus === 'active' ? (
            `Active (${getTimeRemaining()})`
          ) : sessionStatus === 'expired' ? (
            <>
              Expired
              <button
                className={styles.setupStatusRetryBtn}
                onClick={onRetrySession}
                disabled={sessionRetrying || isAuthenticating}
              >
                <RefreshCw size={12} />
                Refresh
              </button>
            </>
          ) : (
            <>
              {sessionError || 'Not Connected'}
              {isEnrolled && (
                <button
                  className={styles.setupStatusRetryBtn}
                  onClick={onRetrySession}
                  disabled={sessionRetrying || isAuthenticating}
                >
                  <RefreshCw size={12} />
                  Connect
                </button>
              )}
            </>
          )}
        </div>
      </div>

    </div>
  );
}
