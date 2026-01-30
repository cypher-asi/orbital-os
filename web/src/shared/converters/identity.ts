/**
 * Identity Type Converters
 *
 * Convert between service layer types (snake_case) and UI types (camelCase).
 *
 * Service layer types are defined in: client-services/identity/types.ts
 * UI types are defined in: shared/types/identity.ts
 *
 * This module provides the bridge between the two layers.
 */

import type {
  MachineKeyCapabilities as UIMachineKeyCapabilities,
  MachineKeyRecord as UIMachineKeyRecord,
  MachineKeyCapability,
  CredentialType as UICredentialType,
  LinkedCredential as UILinkedCredential,
  NeuralKeyGenerated as UINeuralKeyGenerated,
  NeuralShard as UINeuralShard,
  KeyScheme,
} from '../types/identity';

import type {
  MachineKeyCapabilities as ServiceMachineKeyCapabilities,
  LegacyMachineKeyCapabilities as ServiceLegacyMachineKeyCapabilities,
  MachineKeyRecord as ServiceMachineKeyRecord,
  CredentialType as ServiceCredentialType,
  LinkedCredential as ServiceLinkedCredential,
  NeuralKeyGenerated as ServiceNeuralKeyGenerated,
  NeuralShard as ServiceNeuralShard,
} from '@/client-services/identity/types';

import {
  isLegacyCapabilities,
  convertLegacyCapabilities,
} from '@/client-services/identity/types';

/** Service capabilities type that accepts both modern and legacy formats */
type ServiceCapabilities = ServiceMachineKeyCapabilities | ServiceLegacyMachineKeyCapabilities;

import { bytesToHex, u128ToUuid } from '@/client-services/identityUtils';

// =============================================================================
// Machine Key Converters
// =============================================================================

/**
 * Convert service capabilities to UI format.
 * Handles both modern (string array) and legacy (boolean struct) formats.
 */
export function convertCapabilities(
  caps: ServiceCapabilities
): UIMachineKeyCapabilities {
  // Handle legacy format (boolean struct)
  if (isLegacyCapabilities(caps)) {
    const converted = convertLegacyCapabilities(caps);
    return {
      capabilities: converted.capabilities as MachineKeyCapability[],
      expiresAt: converted.expires_at,
    };
  }

  // Modern format (string array)
  return {
    capabilities: caps.capabilities as MachineKeyCapability[],
    expiresAt: caps.expires_at,
  };
}

/**
 * Convert UI capabilities to service format.
 * Defaults to ['AUTHENTICATE', 'ENCRYPT'] if none provided.
 */
export function convertCapabilitiesForService(
  caps?: MachineKeyCapability[]
): ServiceMachineKeyCapabilities {
  const defaultCaps: MachineKeyCapability[] = ['AUTHENTICATE', 'ENCRYPT'];
  const capabilities = caps && caps.length > 0 ? caps : defaultCaps;

  return {
    capabilities,
    expires_at: null,
  };
}

/**
 * Convert service machine record to UI format.
 *
 * @param record - Service layer machine record (snake_case)
 * @param currentMachineId - Current machine ID for isCurrentDevice check
 * @returns UI format machine record (camelCase)
 */
export function convertMachineRecord(
  record: ServiceMachineKeyRecord,
  currentMachineId?: string
): UIMachineKeyRecord {
  // machine_id comes as a number from JSON, convert to UUID format (matches ZID server)
  const machineIdUuid = u128ToUuid(record.machine_id);
  const authorizedByUuid = u128ToUuid(record.authorized_by);

  const normalizeKeyScheme = (scheme?: string): KeyScheme => {
    switch (scheme) {
      case 'pq_hybrid':
      case 'PqHybrid':
        return 'pq_hybrid';
      case 'classical':
      case 'Classical':
      default:
        return 'classical';
    }
  };

  return {
    machineId: machineIdUuid,
    signingPublicKey: bytesToHex(record.signing_public_key),
    encryptionPublicKey: bytesToHex(record.encryption_public_key),
    authorizedAt: record.authorized_at,
    authorizedBy: authorizedByUuid,
    capabilities: convertCapabilities(record.capabilities),
    machineName: record.machine_name,
    lastSeenAt: record.last_seen_at,
    isCurrentDevice: machineIdUuid === currentMachineId,
    epoch: record.epoch ?? 1, // Use service value, fallback to 1 for backward compatibility
    keyScheme: normalizeKeyScheme(record.key_scheme),
    pqSigningPublicKey: record.pq_signing_public_key
      ? bytesToHex(record.pq_signing_public_key)
      : undefined,
    pqEncryptionPublicKey: record.pq_encryption_public_key
      ? bytesToHex(record.pq_encryption_public_key)
      : undefined,
  };
}

// =============================================================================
// Credential Converters
// =============================================================================

/**
 * Convert service credential type to UI type (PascalCase -> lowercase).
 */
export function convertCredentialType(type: ServiceCredentialType): UICredentialType {
  switch (type) {
    case 'Email':
      return 'email';
    case 'Phone':
      return 'phone';
    case 'OAuth':
      return 'oauth';
    case 'WebAuthn':
      return 'webauthn';
    default:
      return 'email';
  }
}

/**
 * Convert UI credential type to service type (lowercase -> PascalCase).
 */
export function convertCredentialTypeForService(type: UICredentialType): ServiceCredentialType {
  switch (type) {
    case 'email':
      return 'Email';
    case 'phone':
      return 'Phone';
    case 'oauth':
      return 'OAuth';
    case 'webauthn':
      return 'WebAuthn';
    default:
      return 'Email';
  }
}

/**
 * Convert service credential to UI format.
 */
export function convertCredential(cred: ServiceLinkedCredential): UILinkedCredential {
  return {
    type: convertCredentialType(cred.credential_type),
    identifier: cred.value,
    verified: cred.verified,
    linkedAt: cred.linked_at,
    verifiedAt: cred.verified_at,
    isPrimary: cred.is_primary,
  };
}

// =============================================================================
// Neural Key Converters
// =============================================================================

/**
 * Convert service Neural Key response to UI format.
 */
export function convertNeuralKeyGenerated(
  service: ServiceNeuralKeyGenerated
): UINeuralKeyGenerated {
  return {
    userId: service.user_id,
    publicIdentifiers: {
      identitySigningPubKey: service.public_identifiers.identity_signing_pub_key,
      machineSigningPubKey: service.public_identifiers.machine_signing_pub_key,
      machineEncryptionPubKey: service.public_identifiers.machine_encryption_pub_key,
    },
    shards: service.shards.map((s) => ({ index: s.index, hex: s.hex })),
    createdAt: service.created_at,
  };
}

/**
 * Convert UI shard format to service format.
 * (They're actually the same, but this makes the conversion explicit.)
 */
export function convertShardsForService(shards: UINeuralShard[]): ServiceNeuralShard[] {
  return shards.map((s) => ({ index: s.index, hex: s.hex }));
}
