# ZID Enrollment Payload Specification

## Overview

This document describes the exact payload format sent to the ZID server during identity enrollment.

## HTTP Request

```
POST /v1/identity
Content-Type: application/json
```

## Payload Structure (snake_case)

```json
{
  "identity_id": "<uuid>",
  "identity_signing_public_key": "<hex>",
  "authorization_signature": "<hex>",
  "machine_key": {
    "machine_id": "<uuid>",
    "signing_public_key": "<hex>",
    "encryption_public_key": "<hex>",
    "capabilities": ["SIGN", "ENCRYPT", "VAULT_OPERATIONS"],
    "device_name": "Browser",
    "device_platform": "web"
  },
  "namespace_name": "personal",
  "created_at": <unix_seconds>
}
```

## Field Descriptions

### Root Level

| Field | Type | Description |
|-------|------|-------------|
| `identity_id` | String (UUID) | Locally generated identity UUID, formatted with hyphens |
| `identity_signing_public_key` | String (hex) | Ed25519 public key for identity-level signing (64 chars) |
| `authorization_signature` | String (hex) | Ed25519 signature proving ownership (128 chars) |
| `machine_key` | Object | First machine key for this identity |
| `namespace_name` | String | Namespace name (e.g., "personal") |
| `created_at` | Number | Unix timestamp in **seconds** (not milliseconds) |

### Machine Key Object

| Field | Type | Description |
|-------|------|-------------|
| `machine_id` | String (UUID) | Locally generated machine UUID, formatted with hyphens |
| `signing_public_key` | String (hex) | Ed25519 public key for machine signing (64 chars) |
| `encryption_public_key` | String (hex) | X25519 public key for machine encryption (64 chars) |
| `capabilities` | Array[String] | Capabilities: `["SIGN", "ENCRYPT", "VAULT_OPERATIONS"]` |
| `device_name` | String | Human-readable device name (e.g., "Browser") |
| `device_platform` | String | Platform identifier (e.g., "web", "wasm32") |

## Authorization Signature

The `authorization_signature` proves ownership of the identity private key.

### Signature Message Format

```
"create" + identity_id.bytes + machine_signing_public_key.bytes + created_at.bytes
```

**Important:**
- Use big-endian byte order for UUID and timestamp
- Concatenate raw bytes (no delimiters)
- Sign with identity private key using Ed25519

### Example Message Construction

```rust
let mut message = Vec::new();
message.extend_from_slice(b"create");                     // Literal string
message.extend_from_slice(&identity_id.to_be_bytes());    // 16 bytes, big-endian
message.extend_from_slice(&machine_signing_public_key);   // 32 bytes
message.extend_from_slice(&created_at.to_be_bytes());     // 8 bytes, big-endian
let signature = ed25519_sign(&identity_private_key, &message);
```

## Implementation Notes

### Timestamp Format
- **Use seconds, not milliseconds**
- Convert: `wallclock_ms / 1000`
- Example: `1737504000` (not `1737504000000`)

### UUID Format
- Use hyphenated format: `550e8400-e29b-41d4-a716-446655440000`
- Generated with `format!("{:08x}-{:04x}-{:04x}-{:04x}-{:012x}")`

### Hex Encoding
- Public keys: 64 hex characters (32 bytes)
- Signature: 128 hex characters (64 bytes)
- Use lowercase hex

### Capabilities
- Must include at least: `["SIGN", "ENCRYPT", "VAULT_OPERATIONS"]`
- Exact string matches required

## Example Payload

```json
{
  "identity_id": "550e8400-e29b-41d4-a716-446655440000",
  "identity_signing_public_key": "a1b2c3d4e5f6789012345678901234567890abcdef1234567890abcdef123456",
  "authorization_signature": "d3a4b5c6e7f89012345678901234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12345678",
  "machine_key": {
    "machine_id": "660e8400-e29b-41d4-a716-446655440001",
    "signing_public_key": "f0e1d2c3b4a59687776655443322110fedcba9876543210fedcba9876543210",
    "encryption_public_key": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "capabilities": ["SIGN", "ENCRYPT", "VAULT_OPERATIONS"],
    "device_name": "Browser",
    "device_platform": "web"
  },
  "namespace_name": "personal",
  "created_at": 1737504000
}
```

## Implementation Files

- **Types**: `crates/zos-identity/src/ipc.rs`
  - `CreateIdentityRequest`
  - `ZidMachineKey`

- **Crypto**: `crates/zos-apps/src/identity/crypto.rs`
  - `canonicalize_identity_creation_message()`
  - `sign_message()`
  - `format_uuid()`
  - `bytes_to_hex()`

- **Handler**: `crates/zos-apps/src/bin/identity_service/handlers/session.rs`
  - `continue_zid_enroll_after_read()`

## Changes from Previous Version

### Fixed Issues

1. ✅ **Timestamp format**: Changed from milliseconds to seconds
2. ✅ **Signature message**: Simplified to match ZID spec
3. ✅ **Field names**: All snake_case (identity_id, not identityId)
4. ✅ **Machine key**: Removed unnecessary fields
5. ✅ **Namespace**: Required string field (not optional)

### Simplified Fields

Removed from `ZidMachineKey`:
- `identity_id` (not needed in nested object)
- `namespace_id` (handled at root level)
- `epoch`, `created_at`, `expires_at`, `last_used_at`
- `revoked`, `revoked_at`
- `key_scheme`
- `pq_signing_public_key`, `pq_encryption_public_key`

## Testing

To verify the payload:
1. Run enrollment from browser
2. Check network request in DevTools
3. Verify JSON matches this spec exactly
4. Check that signature signs the correct message

## Expected Response

On success (HTTP 200):
```json
{
  "access_token": "eyJ...",
  "refresh_token": "ref_...",
  "session_id": "sess_...",
  "expires_in": 3600
}
```

On error (HTTP 422):
```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Invalid field",
    "field": "identity_id"
  }
}
```
