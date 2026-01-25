# Identity Service Storage Paths

This document defines the canonical VFS storage paths used by the Identity Service.

## Directory Structure

```
/home/{user_id}/
  .zos/
    identity/
      public_keys.json          # LocalKeyStore - public identity/machine keys
      private_keys.enc          # EncryptedPrivateKeys - encrypted private keys
      zid_session.json          # ZidSession - ZID server session tokens
      machine/
        {machine_id}.json       # MachineKeyRecord - per-machine key metadata
    credentials/
      credentials.json          # CredentialStore - linked credentials (email, etc)
    sessions/
      {session_id}.json         # Session - local authentication sessions
    tokens/
      {family_id}.json          # TokenFamily - token family metadata
    config/
      preferences.json          # UserPreferences - user settings
```

## Path Functions

All paths should be accessed via the canonical `storage_path()` methods:

### Identity Keys

```rust
use zos_identity::keystore::{LocalKeyStore, EncryptedPrivateKeys, MachineKeyRecord};

// Public keys (identity + machine signing/encryption)
let path = LocalKeyStore::storage_path(user_id);
// Returns: "/home/{user_id:032x}/.zos/identity/public_keys.json"

// Encrypted private keys
let path = EncryptedPrivateKeys::storage_path(user_id);
// Returns: "/home/{user_id:032x}/.zos/identity/private_keys.enc"

// Individual machine key
let path = MachineKeyRecord::storage_path(user_id, machine_id);
// Returns: "/home/{user_id:032x}/.zos/identity/machine/{machine_id:032x}.json"
```

### ZID Session

```rust
use zos_identity::ipc::ZidSession;

// ZID server session (access tokens, refresh tokens)
let path = ZidSession::storage_path(user_id);
// Returns: "/home/{user_id:032x}/.zos/identity/zid_session.json"
```

### Credentials

```rust
use zos_identity::keystore::CredentialStore;

// Linked credentials (email, phone, etc)
let path = CredentialStore::storage_path(user_id);
// Returns: "/home/{user_id:032x}/.zos/credentials/credentials.json"
```

### Sessions and Tokens

```rust
use zos_identity::session::{Session, TokenFamily};

// Local session
let path = session.storage_path(); // or Session::storage_path(user_id, session_id)
// Returns: "/home/{user_id:032x}/.zos/sessions/{session_id:032x}.json"

// Token family
let path = TokenFamily::storage_path(user_id, family_id);
// Returns: "/home/{user_id:032x}/.zos/tokens/{family_id:032x}.json"
```

### User Preferences

```rust
use zos_identity::types::UserPreferences;

// User preferences
let path = UserPreferences::storage_path(user_id);
// Returns: "/home/{user_id:032x}/.zos/config/preferences.json"
```

## Path Format

All user IDs and machine IDs are formatted as 32-character zero-padded hexadecimal strings using `{:032x}` format specifier.

Example:
- User ID: `0x00000000000000000000000000000001`
- Path: `/home/00000000000000000000000000000001/.zos/identity/public_keys.json`

## Enrollment Flow Storage

During ZID enrollment, the following files are created/updated:

1. **Read existing machine key** (used as seed):
   - Path: `/home/{user_id}/.zos/identity/machine/{machine_id}.json`
   - Contains: `MachineKeyRecord` with public keys

2. **Store ZID session** (after successful enrollment):
   - Path: `/home/{user_id}/.zos/identity/zid_session.json`
   - Contains: `ZidSession` with access tokens, refresh tokens

3. **Future: Store derived keys** (optional, for production):
   - Identity keys: `/home/{user_id}/.zos/identity/public_keys.json`
   - Machine key: `/home/{user_id}/.zos/identity/machine/{new_machine_id}.json`

## Best Practices

1. **Always use canonical path methods** - Never hardcode paths as strings
2. **Check parent directory exists** before writing files
3. **Write both content and inode** for each file (VFS requires both)
4. **Use proper error handling** for storage operations
5. **Format user/machine IDs** consistently with `{:032x}`

## Implementation Status

✅ All storage paths are implemented and verified
✅ `ZidSession::storage_path()` added for consistency
✅ Enrollment handler uses canonical paths
✅ Session storage tested and working
