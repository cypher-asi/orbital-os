import { useState, useCallback, useMemo } from 'react';
import { Panel, Button, Text, Label } from '@cypher-asi/zui';
import styles from './PermissionDialog.module.css';

// =============================================================================
// Types (matching 03-security.md spec)
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
 * Capability request from an app's manifest
 */
export interface CapabilityRequest {
  /** Type of kernel object being requested */
  objectType: ObjectType;
  /** Permissions needed on this object */
  permissions: Permissions;
  /** Human-readable reason (shown to user in permission dialog) */
  reason: string;
  /** Whether this permission is required for the app to function */
  required: boolean;
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
  /** Description */
  description: string;
  /** Requested capabilities */
  capabilities: CapabilityRequest[];
  /** Whether this is a factory (trusted) app */
  isFactory?: boolean;
}

// =============================================================================
// Component Props
// =============================================================================

export interface PermissionDialogProps {
  /** App requesting capabilities */
  app: AppManifest;
  /** Called when user approves (with list of approved capabilities) */
  onApprove: (approved: CapabilityRequest[]) => void;
  /** Called when user denies all capabilities */
  onDeny: () => void;
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

function getObjectTypeIcon(type: ObjectType): string {
  switch (type) {
    case 'Endpoint':
      return '↔';
    case 'Console':
      return '⌨';
    case 'Storage':
      return '💾';
    case 'Network':
      return '🌐';
    case 'Process':
      return '⚙';
    case 'Memory':
      return '🧠';
    default:
      return '?';
  }
}

// =============================================================================
// PermissionDialog Component
// =============================================================================

/**
 * Capability request dialog for third-party apps.
 * 
 * Shows requested capabilities from the app manifest and allows users to
 * selectively approve or deny capabilities. Required capabilities are
 * automatically selected and cannot be deselected.
 */
export function PermissionDialog({
  app,
  onApprove,
  onDeny,
}: PermissionDialogProps) {
  // Initialize selected state: required capabilities are always selected
  const initialSelected = useMemo(() => {
    const set = new Set<number>();
    app.capabilities.forEach((cap, i) => {
      if (cap.required) {
        set.add(i);
      }
    });
    return set;
  }, [app.capabilities]);

  const [selected, setSelected] = useState<Set<number>>(initialSelected);

  // Toggle a capability selection
  const toggleCapability = useCallback(
    (index: number) => {
      const cap = app.capabilities[index];
      // Can't toggle required capabilities
      if (cap.required) return;

      setSelected((prev) => {
        const next = new Set(prev);
        if (next.has(index)) {
          next.delete(index);
        } else {
          next.add(index);
        }
        return next;
      });
    },
    [app.capabilities]
  );

  // Handle approve - pass only selected capabilities
  const handleApprove = useCallback(() => {
    const approved = app.capabilities.filter((_, i) => selected.has(i));
    onApprove(approved);
  }, [app.capabilities, selected, onApprove]);

  // Check if any required capability is missing (shouldn't happen with proper init)
  const hasRequiredCapabilities = app.capabilities
    .filter((c) => c.required)
    .every((_, i) => selected.has(i));

  // Check if this is a potentially dangerous capability request
  const hasSensitivePermissions = app.capabilities.some(
    (cap) =>
      cap.objectType === 'Network' ||
      cap.objectType === 'Process' ||
      (cap.objectType === 'Storage' && cap.permissions.write)
  );

  return (
    <div className={styles.overlay} onClick={onDeny}>
      <Panel
        variant="glass"
        className={styles.dialog}
        onClick={(e: React.MouseEvent) => e.stopPropagation()}
      >
        {/* Header */}
        <div className={styles.header}>
          <div className={styles.title}>
            <div className={styles.icon}>🔐</div>
            <Text as="span" size="sm" className={styles.appName}>
              {app.name}
            </Text>
            <Label size="xs" variant="default">
              v{app.version}
            </Label>
          </div>
          <Text as="div" size="sm" className={styles.subtitle}>
            This app is requesting the following capabilities:
          </Text>
        </div>

        {/* Content */}
        <div className={styles.content}>
          {/* Warning for sensitive capabilities */}
          {hasSensitivePermissions && (
            <div className={styles.warning}>
              <span className={styles.warningIcon}>⚠</span>
              <Text as="span" size="xs" className={styles.warningText}>
                This app requests sensitive capabilities. Only approve if you
                trust the source.
              </Text>
            </div>
          )}

          {app.capabilities.length === 0 ? (
            <div className={styles.emptyState}>
              <Text as="p" size="sm" variant="muted">
                This app doesn't require any special capabilities.
              </Text>
            </div>
          ) : (
            <div className={styles.permissionList}>
              {app.capabilities.map((cap, index) => (
                <div
                  key={index}
                  className={`${styles.permissionItem} ${
                    cap.required ? styles.permissionItemDisabled : ''
                  }`}
                >
                  <input
                    type="checkbox"
                    className={styles.checkbox}
                    checked={selected.has(index)}
                    disabled={cap.required}
                    onChange={() => toggleCapability(index)}
                    aria-label={`${formatObjectType(cap.objectType)} permission`}
                  />
                  <div className={styles.permissionInfo}>
                    <div className={styles.permissionHeader}>
                      <span className={styles.permissionType}>
                        {getObjectTypeIcon(cap.objectType)}{' '}
                        {formatObjectType(cap.objectType)}
                      </span>
                      <span className={styles.permissionPerms}>
                        {formatPermissions(cap.permissions)}
                      </span>
                      {cap.required && (
                        <span className={styles.requiredBadge}>Required</span>
                      )}
                    </div>
                    <Text
                      as="p"
                      size="xs"
                      variant="muted"
                      className={styles.permissionReason}
                    >
                      {cap.reason}
                    </Text>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className={styles.footer}>
          <Button variant="ghost" size="md" onClick={onDeny}>
            Deny All
          </Button>
          <Button
            variant="primary"
            size="md"
            onClick={handleApprove}
            disabled={!hasRequiredCapabilities}
          >
            {selected.size === 0
              ? 'Allow'
              : `Allow Selected (${selected.size})`}
          </Button>
        </div>
      </Panel>
    </div>
  );
}

