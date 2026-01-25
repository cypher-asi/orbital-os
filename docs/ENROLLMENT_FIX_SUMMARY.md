# ZID Enrollment Fix Summary

## Problem

The identity enrollment was failing with **HTTP 422 (Unprocessable Entity)** because:
1. The payload structure didn't match ZID server expectations
2. The signature message format was incorrect
3. Timestamps were in milliseconds instead of seconds
4. Too many unnecessary fields in the machine key

## Root Cause

We were mixing up two different ZID operations:

### 1. Create Identity (POST /v1/identity) - WHAT WE NEED
- **First time setup** - creates a brand new identity
- No authentication required
- Sends: `identity_id`, `identity_signing_public_key`, `authorization_signature`, `machine_key`, `namespace_name`, `created_at`
- Creates the identity AND its first machine key together

### 2. Enroll Machine (POST /v1/machines/enroll) - NOT THIS
- Adds an additional device to existing identity
- Requires Bearer token authentication
- Just sends machine key details

## Current Implementation Status

✅ **Correct endpoint**: Sending to `POST /v1/identity`
✅ **Correct struct**: Using `CreateIdentityRequest` with all required fields
✅ **Field order**: `identity_id` is first field in struct
⚠️ **Naming confusion**: Called "enrollMachine" but actually does "createIdentity"

## Changes Made

### 1. Fixed Struct Definition
**File**: `crates/zos-identity/src/ipc.rs`

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreateIdentityRequest {
    pub identity_id: String,                      // ✅ FIRST FIELD
    pub identity_signing_public_key: String,
    pub authorization_signature: String,
    pub machine_key: ZidMachineKey,
    pub namespace_name: String,                   // ✅ Required (not optional)
    pub created_at: u64,                          // ✅ Unix seconds
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZidMachineKey {
    pub machine_id: String,
    pub signing_public_key: String,
    pub encryption_public_key: String,
    pub capabilities: Vec<String>,
    pub device_name: String,
    pub device_platform: String,
    // ✅ Removed: identity_id, namespace_id, epoch, timestamps, revoked, key_scheme, pq keys
}
```

### 2. Fixed Authorization Signature
**File**: `crates/zos-apps/src/identity/crypto.rs`

```rust
// OLD: Custom format with many fields
// NEW: ZID spec format
pub fn canonicalize_identity_creation_message(
    identity_id: &u128,
    machine_signing_public_key: &[u8; 32],
    created_at: u64,
) -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(b"create");                  // Literal "create"
    message.extend_from_slice(&identity_id.to_be_bytes()); // Big-endian UUID
    message.extend_from_slice(machine_signing_public_key); // 32 bytes
    message.extend_from_slice(&created_at.to_be_bytes());  // Big-endian timestamp
    message
}
```

### 3. Fixed Timestamp Format
**File**: `crates/zos-apps/src/bin/identity_service/handlers/session.rs`

```rust
let now_ms = syscall::get_wallclock();
let now_secs = now_ms / 1000;  // ✅ Convert to seconds
```

### 4. Simplified Machine Key
**File**: `crates/zos-apps/src/bin/identity_service/handlers/session.rs`

```rust
let machine_key = ZidMachineKey {
    machine_id: format_uuid(machine_id),
    signing_public_key: bytes_to_hex(&machine_keypair.signing_public_key),
    encryption_public_key: bytes_to_hex(&machine_keypair.encryption_public_key),
    capabilities: vec!["SIGN".into(), "ENCRYPT".into(), "VAULT_OPERATIONS".into()],
    device_name: "Browser".into(),
    device_platform: "web".into(),
};
```

### 5. Added Debug Logging
**File**: `crates/zos-apps/src/bin/identity_service/handlers/session.rs`

```rust
// Debug: Log the JSON payload being sent
if let Ok(json_str) = alloc::str::from_utf8(&enroll_body) {
    syscall::debug(&format!("IdentityService: Sending enrollment JSON: {}", json_str));
}
```

## Expected JSON Payload

The service now sends this exact format:

```json
{
  "identity_id": "550e8400-e29b-41d4-a716-446655440000",
  "identity_signing_public_key": "a1b2c3d4...",
  "authorization_signature": "d3a4b5c6...",
  "machine_key": {
    "machine_id": "660e8400-e29b-41d4-a716-446655440001",
    "signing_public_key": "f0e1d2c3...",
    "encryption_public_key": "01234567...",
    "capabilities": ["SIGN", "ENCRYPT", "VAULT_OPERATIONS"],
    "device_name": "Browser",
    "device_platform": "web"
  },
  "namespace_name": "personal",
  "created_at": 1737504000
}
```

## Verification

To verify the fix works:

1. **Check browser console** for debug log: `"IdentityService: Sending enrollment JSON: ..."`
2. **Check browser DevTools Network tab** to see the actual HTTP request body
3. **Verify all fields** match the spec above
4. **Check ZID server response** should be 200 OK with tokens

## Next Steps

If still getting 422 error:
1. Check the debug log output to see the actual JSON being sent
2. Verify the `identity_id` field is present in the JSON
3. Check if there are any serde serialization issues
4. Verify the ZID server is running and accessible

## Cryptographic Improvements (Latest Fix)

### Problem
The previous implementation used **mock cryptographic functions** that generated fake signatures using XOR operations. The ZID server performs real Ed25519 signature verification and rejected these mock signatures with:
```
INVALID_SIGNATURE: Cryptographic signature is invalid
```

### Solution
Integrated the `ed25519-compact` library to generate **real Ed25519 signatures**:

1. **Added dependency** in `crates/zos-apps/Cargo.toml`:
   ```toml
   ed25519-compact = { version = "2.1", default-features = false, features = ["random"] }
   ```

2. **Updated `IdentityKeypair` structure** to use real Ed25519 keypairs:
   ```rust
   pub struct IdentityKeypair {
       pub public_key: [u8; 32],
       pub secret_key: [u8; 64],  // Ed25519 secret key (32-byte seed + 32-byte public key)
       keypair: ed25519_compact::KeyPair,
   }
   ```

3. **Real key generation** in `derive_identity_signing_keypair()`:
   - Derives a 32-byte seed from neural key + identity ID
   - Generates real Ed25519 keypair using `KeyPair::from_seed()`
   - Returns proper Ed25519 public/secret keys

4. **Real signature generation** in `sign_message()`:
   - Uses `keypair.sk.sign(message, None)` for proper Ed25519 signing
   - Generates cryptographically valid signatures that the ZID server can verify

### Benefits
- ✅ Signatures are now cryptographically valid
- ✅ Compatible with ZID server's Ed25519 verification
- ✅ Uses industry-standard Ed25519 algorithm
- ✅ Works in no_std WASM environment
- ✅ Deterministic key derivation from neural key seed

## Files Modified

1. `crates/zos-identity/src/ipc.rs` - Fixed struct definitions
2. `crates/zos-apps/src/identity/crypto.rs` - **Added real Ed25519 crypto** (replaces mock functions)
3. `crates/zos-apps/src/bin/identity_service/handlers/session.rs` - Fixed enrollment logic + debug logging
4. `crates/zos-apps/Cargo.toml` - **Added ed25519-compact dependency**
5. `docs/zid-enrollment-payload.md` - Added documentation
6. `docs/identity-storage-paths.md` - Documented storage paths

## Build Status

✅ Compiles successfully
✅ All fields use snake_case
✅ identity_id is first field in CreateIdentityRequest
✅ Debug logging added to verify payload
✅ **Real Ed25519 signatures** - Added `ed25519-compact` library for proper cryptographic signatures
