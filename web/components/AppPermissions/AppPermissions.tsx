import { useCallback } from 'react';
import { Card, CardItem, Button, Text, Label } from '@cypher-asi/zui';
import {
  ArrowLeftRight,
  Keyboard,
  HardDrive,
  Globe,
  Cog,
  Cpu,
  Shield,
  ShieldOff,
} from 'lucide-react';
import styles from './AppPermissions.module.css';

// =============================================================================
// Types
// =============================================================================

/**
 * Types of kernel objects that can be accessed via capabilities
 */
export type ObjectType =
  | 'Endpoint'
  | 'Console'
  | 'Storage'
  | 'Network'
  | 'Process'
  | 'Memory';

/**
 * Permission bits for capabilities
 */
export interface Permissions {
  read: boolean;
  write: boolean;
  grant: boolean;
}

/**
 * Information about a granted capability
 */
export interface CapabilityInfo {
  /** Capability slot in the process's CSpace */
  slot: number;
  /** Object type */
  objectType: ObjectType;
  /** Permissions */
  permissions: Permissions;
}

/**
 * App manifest information
 */
export interface AppManifest {
  /** Unique app identifier (e.g., "com.example.myapp") */
  id: string;
  /** Display name */
  name: string;
  /** Version string */
  version: string;
  /** Whether this is a factory (trusted) app */
  isFactory?: boolean;
}

// =============================================================================
// Component Props
// =============================================================================

export interface AppPermissionsProps {
  /** App manifest */
  app: AppManifest;
  /** Currently granted capabilities */
  grantedCaps: CapabilityInfo[];
  /** Called when user wants to revoke a capability */
  onRevoke: (objectType: ObjectType) => void;
  /** Called when user wants to revoke all capabilities */
  onRevokeAll?: () => void;
  /** Loading state */
  isLoading?: boolean;
}

// =============================================================================
// Helper Functions
// =============================================================================

function formatObjectType(type: ObjectType): string {
  switch (type) {
    case 'Endpoint':
      return 'IPC Endpoint';
    case 'Console':
      return 'Console Access';
    case 'Storage':
      return 'Storage';
    case 'Network':
      return 'Network';
    case 'Process':
      return 'Process Management';
    case 'Memory':
      return 'Memory Access';
    default:
      return type;
  }
}

function formatPermissions(perms: Permissions): string {
  const parts: string[] = [];
  if (perms.read) parts.push('R');
  if (perms.write) parts.push('W');
  if (perms.grant) parts.push('G');
  return parts.join('/') || 'none';
}

function getObjectTypeIcon(type: ObjectType): React.ReactNode {
  const iconProps = { size: 16, strokeWidth: 1.5 };
  switch (type) {
    case 'Endpoint':
      return <ArrowLeftRight {...iconProps} />;
    case 'Console':
      return <Keyboard {...iconProps} />;
    case 'Storage':
      return <HardDrive {...iconProps} />;
    case 'Network':
      return <Globe {...iconProps} />;
    case 'Process':
      return <Cog {...iconProps} />;
    case 'Memory':
      return <Cpu {...iconProps} />;
    default:
      return null;
  }
}

// =============================================================================
// AppPermissions Component
// =============================================================================

/**
 * Settings component for viewing and managing app capabilities.
 *
 * Displays the list of granted capabilities for an app and allows
 * the user to revoke individual capabilities or all capabilities at once.
 */
export function AppPermissions({
  app,
  grantedCaps,
  onRevoke,
  onRevokeAll,
  isLoading = false,
}: AppPermissionsProps) {
  const handleRevoke = useCallback(
    (objectType: ObjectType) => {
      onRevoke(objectType);
    },
    [onRevoke]
  );

  if (isLoading) {
    return (
      <div className={styles.container}>
        <div className={styles.loading}>
          <div className={styles.spinner} />
          <Text as="span" size="xs" variant="muted">
            Loading capabilities...
          </Text>
        </div>
      </div>
    );
  }

  return (
    <div className={styles.container}>
      {/* Permissions List */}
      {grantedCaps.length === 0 ? (
        <div className={styles.emptyState}>
          <div className={styles.emptyIcon}>
            <ShieldOff size={48} strokeWidth={1} />
          </div>
          <Text as="p" size="sm" variant="muted">
            No capabilities granted
          </Text>
          <Text as="p" size="xs" variant="muted">
            This app has not been granted any capabilities.
          </Text>
        </div>
      ) : (
        <>
          <div className={styles.sectionHeader}>
            <Shield size={14} strokeWidth={1.5} />
            <Text as="span" size="xs" weight="semibold" className={styles.sectionTitle}>
              Granted Capabilities
            </Text>
            <Text as="span" size="xs" variant="muted" className={styles.sectionCount}>
              {grantedCaps.length}
            </Text>
          </div>
          <Card className={styles.capabilitiesCard}>
            {grantedCaps.map((cap) => (
              <CardItem
                key={`${cap.objectType}-${cap.slot}`}
                icon={getObjectTypeIcon(cap.objectType)}
                title={
                  <span className={styles.capTitle}>
                    {formatObjectType(cap.objectType)}
                    <Label size="xs" variant="default" className={styles.permsBadge}>
                      {formatPermissions(cap.permissions)}
                    </Label>
                  </span>
                }
                description={`Capability slot ${cap.slot}`}
                className={styles.capabilityItem}
              >
                <Button
                  variant="danger"
                  size="sm"
                  onClick={() => handleRevoke(cap.objectType)}
                  className={styles.revokeButton}
                >
                  Revoke
                </Button>
              </CardItem>
            ))}
          </Card>

          {/* Danger Zone */}
          {onRevokeAll && grantedCaps.length > 1 && (
            <div className={styles.dangerZone}>
              <Text as="div" size="sm" className={styles.dangerTitle}>
                Danger Zone
              </Text>
              <Text as="p" size="xs" className={styles.dangerText}>
                Revoking all capabilities may cause the app to stop working correctly.
              </Text>
              <Button variant="danger" size="sm" onClick={onRevokeAll}>
                Revoke All Capabilities
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  );
}

