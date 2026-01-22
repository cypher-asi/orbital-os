# Zero-ID Rust Integration Guide

This guide demonstrates how to integrate Zero-ID into a Rust server application. It covers cryptographic operations, identity management, authentication methods, and session handling.

## Dependencies

Add these to your `Cargo.toml`:

```toml
[dependencies]
zid-crypto = { path = "../crates/zid-crypto" }
zid-identity-core = { path = "../crates/zid-identity-core" }
zid-methods = { path = "../crates/zid-methods" }
zid-sessions = { path = "../crates/zid-sessions" }
zid-policy = { path = "../crates/zid-policy" }
zid-storage = { path = "../crates/zid-storage" }

uuid = { version = "1", features = ["v4"] }
tokio = { version = "1", features = ["full"] }
```

---

## 1. Neural Key Generation & Shamir Shares

Neural Keys are generated **client-side only** and never transmitted. The server only sees public keys derived from them.

```rust
use zid_crypto::{NeuralKey, split_neural_key, combine_shards, NeuralShard};

// Generate a new Neural Key (32 bytes of entropy)
fn generate_neural_key() -> Result<NeuralKey, zid_crypto::CryptoError> {
    let neural_key = NeuralKey::generate()?;
    
    // Validate entropy (optional but recommended)
    neural_key.validate_entropy()?;
    
    Ok(neural_key)
}

// Split into 5 shares (3-of-5 threshold scheme)
fn create_backup_shares(neural_key: &NeuralKey) -> Result<[NeuralShard; 5], zid_crypto::CryptoError> {
    let shards = split_neural_key(neural_key)?;
    
    // Each shard can be exported to hex for storage
    for (i, shard) in shards.iter().enumerate() {
        let hex = shard.to_hex();
        println!("Shard {}: {} (index: {})", i + 1, hex, shard.index);
    }
    
    Ok(shards)
}

// Reconstruct from any 3+ shares
fn recover_neural_key(shards: &[NeuralShard]) -> Result<NeuralKey, zid_crypto::CryptoError> {
    // Requires at least 3 shards
    let neural_key = combine_shards(shards)?;
    Ok(neural_key)
}

// Parse shard from hex string (e.g., from user input)
fn parse_shard(hex_str: &str) -> Result<NeuralShard, zid_crypto::CryptoError> {
    NeuralShard::from_hex(hex_str)
}
```

---

## 2. Machine Key Derivation

Machine Keys are deterministically derived from the Neural Key. Two schemes are supported:

- **Classical**: Ed25519 (signing) + X25519 (encryption)
- **PqHybrid**: Classical + ML-DSA-65 (PQ signing) + ML-KEM-768 (PQ encryption)

```rust
use zid_crypto::{
    NeuralKey, KeyScheme, MachineKeyCapabilities, MachineKeyPair,
    derive_machine_keypair, derive_machine_keypair_with_scheme,
};
use uuid::Uuid;

// Derive a Classical scheme machine key
fn derive_classical_machine_key(
    neural_key: &NeuralKey,
    identity_id: Uuid,
    machine_id: Uuid,
    epoch: u64,
) -> Result<MachineKeyPair, zid_crypto::CryptoError> {
    let capabilities = MachineKeyCapabilities::FULL_DEVICE;
    
    derive_machine_keypair(
        neural_key,
        &identity_id,
        &machine_id,
        epoch,
        capabilities,
    )
}

// Derive a PQ-Hybrid scheme machine key (post-quantum protection)
fn derive_pq_hybrid_machine_key(
    neural_key: &NeuralKey,
    identity_id: Uuid,
    machine_id: Uuid,
    epoch: u64,
) -> Result<MachineKeyPair, zid_crypto::CryptoError> {
    let capabilities = MachineKeyCapabilities::FULL_DEVICE;
    
    derive_machine_keypair_with_scheme(
        neural_key,
        &identity_id,
        &machine_id,
        epoch,
        capabilities,
        KeyScheme::PqHybrid,
    )
}

// Extract public keys for server enrollment
fn get_public_keys(keypair: &MachineKeyPair) {
    // Classical keys (always present)
    let signing_pk: [u8; 32] = keypair.signing_public_key();
    let encryption_pk: [u8; 32] = keypair.encryption_public_key();
    
    // PQ keys (only in PqHybrid mode)
    if keypair.has_post_quantum_keys() {
        let pq_signing_pk: [u8; 1952] = keypair.pq_signing_public_key().unwrap();
        let pq_encryption_pk: [u8; 1184] = keypair.pq_encryption_public_key().unwrap();
    }
    
    println!("Scheme: {:?}", keypair.scheme());
    println!("Capabilities: {:?}", keypair.capabilities());
}

// Available capability presets
fn capability_examples() {
    // Full device - all operations
    let _ = MachineKeyCapabilities::FULL_DEVICE;
    
    // Service machine - no MLS messaging
    let _ = MachineKeyCapabilities::SERVICE_MACHINE;
    
    // Limited device - no vault access
    let _ = MachineKeyCapabilities::LIMITED_DEVICE;
    
    // Custom combination
    let custom = MachineKeyCapabilities::AUTHENTICATE 
        | MachineKeyCapabilities::SIGN;
}
```

---

## 3. Server Setup

Before using identity and authentication features, initialize the services:

```rust
use std::sync::Arc;
use zid_storage::RocksDbStorage;
use zid_policy::PolicyEngineImpl;
use zid_identity_core::{IdentityCoreService, EventPublisher};
use zid_methods::{AuthMethodsService, OAuthConfigs, OAuthProviderConfig};
use zid_sessions::SessionService;
use async_trait::async_trait;

// Event publisher implementation (required for Identity Core)
struct MyEventPublisher;

#[async_trait]
impl EventPublisher for MyEventPublisher {
    async fn publish(&self, event: zid_identity_core::RevocationEvent) -> zid_identity_core::Result<()> {
        // Handle revocation events (e.g., push to webhook, SSE, etc.)
        println!("Revocation event: {:?}", event);
        Ok(())
    }
}

async fn setup_services() -> Result<Services, Box<dyn std::error::Error>> {
    // 1. Initialize storage
    let storage = Arc::new(RocksDbStorage::open("./data/zid")?);
    
    // 2. Initialize policy engine
    let policy = Arc::new(PolicyEngineImpl::new(Arc::clone(&storage)));
    
    // 3. Initialize event publisher
    let events = Arc::new(MyEventPublisher);
    
    // 4. Initialize Identity Core
    let identity_core = Arc::new(IdentityCoreService::new(
        Arc::clone(&policy),
        Arc::clone(&events),
        Arc::clone(&storage),
    ));
    
    // 5. Generate or load service master key (32 bytes)
    // IMPORTANT: Store this securely, it protects MFA secrets
    let service_master_key: [u8; 32] = load_or_generate_master_key();
    
    // 6. Initialize Auth Methods (with optional OAuth configs)
    let oauth_configs = OAuthConfigs {
        google: Some(OAuthProviderConfig {
            client_id: std::env::var("GOOGLE_CLIENT_ID")?,
            client_secret: std::env::var("GOOGLE_CLIENT_SECRET")?,
            redirect_uri: "https://your-app.com/oauth/google/callback".to_string(),
        }),
        x: None,
        epic_games: None,
    };
    
    let auth_methods = Arc::new(AuthMethodsService::with_oauth_configs(
        Arc::clone(&identity_core),
        Arc::clone(&policy),
        Arc::clone(&storage),
        service_master_key,
        oauth_configs,
    ));
    
    // 7. Initialize Session Service
    let session_events = Arc::new(zid_sessions::NoOpEventPublisher);
    let sessions = Arc::new(SessionService::new(
        Arc::clone(&storage),
        Arc::clone(&session_events),
        service_master_key,
    ));
    
    Ok(Services {
        storage,
        policy,
        identity_core,
        auth_methods,
        sessions,
    })
}

struct Services {
    storage: Arc<RocksDbStorage>,
    policy: Arc<PolicyEngineImpl<RocksDbStorage>>,
    identity_core: Arc<IdentityCoreService<PolicyEngineImpl<RocksDbStorage>, MyEventPublisher, RocksDbStorage>>,
    auth_methods: Arc<AuthMethodsService</* ... */>>,
    sessions: Arc<SessionService</* ... */>>,
}

fn load_or_generate_master_key() -> [u8; 32] {
    // In production: load from HSM, KMS, or secure storage
    // For development only:
    let mut key = [0u8; 32];
    use rand::RngCore;
    rand::thread_rng().fill_bytes(&mut key);
    key
}
```

---

## 4. Identity Creation

Create a new identity with the first machine enrolled:

```rust
use zid_identity_core::{IdentityCore, CreateIdentityRequest, MachineKey};
use zid_crypto::{
    NeuralKey, MachineKeyCapabilities, KeyScheme,
    derive_identity_signing_keypair, derive_machine_keypair_with_scheme,
    canonicalize_identity_creation_message, sign_message, current_timestamp,
};
use uuid::Uuid;

async fn create_identity(
    identity_core: &impl IdentityCore,
    neural_key: &NeuralKey,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let identity_id = Uuid::new_v4();
    let machine_id = Uuid::new_v4();
    let now = current_timestamp();
    
    // 1. Derive identity signing keypair from Neural Key
    let (identity_signing_public_key, identity_keypair) = 
        derive_identity_signing_keypair(neural_key, &identity_id)?;
    
    // 2. Derive first machine key
    let machine_keypair = derive_machine_keypair_with_scheme(
        neural_key,
        &identity_id,
        &machine_id,
        1, // epoch
        MachineKeyCapabilities::FULL_DEVICE,
        KeyScheme::Classical,
    )?;
    
    // 3. Build MachineKey struct for enrollment
    let machine_key = MachineKey {
        machine_id,
        identity_id,
        namespace_id: identity_id, // Default namespace = identity
        signing_public_key: machine_keypair.signing_public_key(),
        encryption_public_key: machine_keypair.encryption_public_key(),
        capabilities: MachineKeyCapabilities::FULL_DEVICE,
        epoch: 1,
        created_at: now,
        expires_at: None,
        last_used_at: None,
        device_name: "Primary Device".to_string(),
        device_platform: "linux".to_string(),
        revoked: false,
        revoked_at: None,
        key_scheme: KeyScheme::Classical,
        pq_signing_public_key: None,
        pq_encryption_public_key: None,
    };
    
    // 4. Create authorization signature
    let message = canonicalize_identity_creation_message(
        &identity_id,
        &identity_signing_public_key,
        &machine_id,
        &machine_key.signing_public_key,
        &machine_key.encryption_public_key,
        now,
    );
    let signature = sign_message(&identity_keypair, &message);
    
    // 5. Submit creation request
    let request = CreateIdentityRequest {
        identity_id,
        identity_signing_public_key,
        machine_key,
        authorization_signature: signature.to_vec(),
        namespace_name: Some("Personal".to_string()),
        created_at: now,
    };
    
    let identity = identity_core.create_identity(request).await?;
    
    println!("Created identity: {}", identity.identity_id);
    Ok(identity.identity_id)
}
```

---

## 5. List Machine Keys

```rust
use zid_identity_core::{IdentityCore, MachineKey};
use uuid::Uuid;

async fn list_machines(
    identity_core: &impl IdentityCore,
    identity_id: Uuid,
    namespace_id: Uuid,
) -> Result<Vec<MachineKey>, Box<dyn std::error::Error>> {
    let machines = identity_core.list_machines(identity_id, namespace_id).await?;
    
    for machine in &machines {
        println!("Machine: {}", machine.machine_id);
        println!("  Device: {} ({})", machine.device_name, machine.device_platform);
        println!("  Scheme: {:?}", machine.key_scheme);
        println!("  Capabilities: {:?}", machine.capabilities);
        println!("  Revoked: {}", machine.revoked);
        println!("  Created: {}", machine.created_at);
    }
    
    Ok(machines)
}
```

---

## 6. Connect Email Credential

```rust
use zid_methods::AuthMethods;
use uuid::Uuid;

async fn attach_email(
    auth_methods: &impl AuthMethods,
    identity_id: Uuid,
    email: String,
    password: String,
) -> Result<(), Box<dyn std::error::Error>> {
    auth_methods
        .attach_email_credential(identity_id, email, password)
        .await?;
    
    println!("Email credential attached to identity {}", identity_id);
    Ok(())
}
```

---

## 7. Connect OAuth Provider

```rust
use zid_methods::{AuthMethods, OAuthProvider, OAuthCompleteRequest};
use uuid::Uuid;

// Step 1: Initiate OAuth flow (returns URL to redirect user to)
async fn start_oauth_link(
    auth_methods: &impl AuthMethods,
    identity_id: Uuid,
    provider: OAuthProvider,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = auth_methods
        .oauth_initiate(identity_id, provider)
        .await?;
    
    println!("State: {}", response.state);
    println!("Redirect user to: {}", response.authorization_url);
    
    Ok(response.authorization_url)
}

// Step 2: Complete OAuth flow (after callback with code)
async fn complete_oauth_link(
    auth_methods: &impl AuthMethods,
    identity_id: Uuid,
    code: String,
    state: String,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let request = OAuthCompleteRequest { code, state };
    
    let link_id = auth_methods
        .oauth_complete(identity_id, request)
        .await?;
    
    println!("OAuth link created: {}", link_id);
    Ok(link_id)
}

// Remove OAuth link
async fn remove_oauth_link(
    auth_methods: &impl AuthMethods,
    identity_id: Uuid,
    provider: OAuthProvider,
) -> Result<(), Box<dyn std::error::Error>> {
    auth_methods
        .revoke_oauth_link(identity_id, provider)
        .await?;
    
    println!("OAuth link revoked for {:?}", provider);
    Ok(())
}
```

---

## 8. Authentication: Machine Key Challenge-Response

The primary authentication method using cryptographic challenge-response:

```rust
use zid_methods::{AuthMethods, ChallengeRequest, ChallengeResponse};
use zid_crypto::{sign_challenge, MachineKeyPair, EntityType};
use uuid::Uuid;

async fn authenticate_with_machine_key(
    auth_methods: &impl AuthMethods,
    machine_keypair: &MachineKeyPair,
    machine_id: Uuid,
    ip_address: String,
    user_agent: String,
) -> Result<AuthResult, Box<dyn std::error::Error>> {
    // 1. Request a challenge
    let challenge_request = ChallengeRequest {
        machine_id,
        entity_type: EntityType::Device,
    };
    
    let challenge = auth_methods
        .create_challenge(challenge_request)
        .await?;
    
    println!("Challenge received: {}", challenge.challenge_id);
    
    // 2. Sign the challenge with machine key
    let signature = sign_challenge(
        machine_keypair.signing_key_pair(),
        &challenge.challenge_id,
        &challenge.nonce,
        challenge.timestamp,
        challenge.expires_at,
        &challenge.entity_type,
    )?;
    
    // 3. Submit response
    let response = ChallengeResponse {
        challenge_id: challenge.challenge_id,
        machine_id,
        signature: signature.to_vec(),
        mfa_code: None, // Include if MFA is enabled
    };
    
    let auth_result = auth_methods
        .authenticate_machine(response, ip_address, user_agent)
        .await?;
    
    println!("Auth result: identity={}, mfa_required={}", 
        auth_result.identity_id, auth_result.mfa_required);
    
    Ok(auth_result)
}

use zid_methods::AuthResult;
```

---

## 9. Authentication: Email + Password

```rust
use zid_methods::{AuthMethods, EmailAuthRequest, AuthResult};

async fn authenticate_with_email(
    auth_methods: &impl AuthMethods,
    email: String,
    password: String,
    machine_id: Option<Uuid>, // None = use virtual machine
    ip_address: String,
    user_agent: String,
) -> Result<AuthResult, Box<dyn std::error::Error>> {
    let request = EmailAuthRequest {
        email,
        password,
        machine_id,
        mfa_code: None, // Include if MFA is enabled
    };
    
    let auth_result = auth_methods
        .authenticate_email(request, ip_address, user_agent)
        .await?;
    
    Ok(auth_result)
}
```

---

## 10. Authentication: OAuth

```rust
use zid_methods::{AuthMethods, OAuthProvider, OAuthCompleteRequest, AuthResult};

// Step 1: Get login URL
async fn start_oauth_login(
    auth_methods: &impl AuthMethods,
    provider: OAuthProvider,
) -> Result<String, Box<dyn std::error::Error>> {
    let response = auth_methods
        .oauth_initiate_login(provider)
        .await?;
    
    Ok(response.authorization_url)
}

// Step 2: Complete login after callback
async fn complete_oauth_login(
    auth_methods: &impl AuthMethods,
    code: String,
    state: String,
    ip_address: String,
    user_agent: String,
) -> Result<AuthResult, Box<dyn std::error::Error>> {
    let request = OAuthCompleteRequest { code, state };
    
    let auth_result = auth_methods
        .authenticate_oauth(request, ip_address, user_agent)
        .await?;
    
    Ok(auth_result)
}
```

---

## 11. Session Management

After successful authentication, create a session and issue tokens:

```rust
use zid_sessions::{SessionManager, SessionTokens, TokenIntrospection};
use uuid::Uuid;

async fn create_user_session(
    sessions: &impl SessionManager,
    identity_id: Uuid,
    machine_id: Uuid,
    namespace_id: Uuid,
    mfa_verified: bool,
) -> Result<SessionTokens, Box<dyn std::error::Error>> {
    let capabilities = vec![
        "AUTHENTICATE".to_string(),
        "SIGN".to_string(),
    ];
    
    let scope = vec![
        "identity:read".to_string(),
        "identity:write".to_string(),
    ];
    
    let tokens = sessions
        .create_session(
            identity_id,
            machine_id,
            namespace_id,
            mfa_verified,
            capabilities,
            scope,
        )
        .await?;
    
    println!("Session created: {}", tokens.session_id);
    println!("Access token expires in: {} seconds", tokens.expires_in);
    
    Ok(tokens)
}

async fn refresh_tokens(
    sessions: &impl SessionManager,
    refresh_token: String,
    session_id: Uuid,
    machine_id: Uuid,
) -> Result<SessionTokens, Box<dyn std::error::Error>> {
    let tokens = sessions
        .refresh_session(refresh_token, session_id, machine_id)
        .await?;
    
    Ok(tokens)
}

async fn validate_token(
    sessions: &impl SessionManager,
    token: String,
    expected_audience: Option<String>,
) -> Result<TokenIntrospection, Box<dyn std::error::Error>> {
    let introspection = sessions
        .introspect_token(token, expected_audience)
        .await?;
    
    if introspection.active {
        println!("Token valid for identity: {:?}", introspection.identity_id);
        println!("Scope: {:?}", introspection.scope);
    } else {
        println!("Token is invalid or expired");
    }
    
    Ok(introspection)
}

async fn revoke_user_session(
    sessions: &impl SessionManager,
    session_id: Uuid,
) -> Result<(), Box<dyn std::error::Error>> {
    sessions.revoke_session(session_id).await?;
    println!("Session {} revoked", session_id);
    Ok(())
}
```

---

## 12. MFA Setup and Verification

```rust
use zid_methods::{AuthMethods, MfaSetup};
use uuid::Uuid;

async fn setup_mfa(
    auth_methods: &impl AuthMethods,
    identity_id: Uuid,
) -> Result<MfaSetup, Box<dyn std::error::Error>> {
    let setup = auth_methods.setup_mfa(identity_id).await?;
    
    // Display to user
    println!("TOTP Secret: {}", setup.secret);
    println!("QR Code URI: {}", setup.provisioning_uri);
    println!("Backup codes: {:?}", setup.backup_codes);
    
    Ok(setup)
}

async fn enable_mfa(
    auth_methods: &impl AuthMethods,
    identity_id: Uuid,
    verification_code: String,
) -> Result<(), Box<dyn std::error::Error>> {
    auth_methods
        .enable_mfa(identity_id, verification_code)
        .await?;
    
    println!("MFA enabled for identity {}", identity_id);
    Ok(())
}

async fn verify_mfa_code(
    auth_methods: &impl AuthMethods,
    identity_id: Uuid,
    code: String,
) -> Result<bool, Box<dyn std::error::Error>> {
    let valid = auth_methods.verify_mfa(identity_id, code).await?;
    Ok(valid)
}
```

---

## Complete Example: Registration Flow

```rust
use uuid::Uuid;

async fn full_registration_flow() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup services
    let services = setup_services().await?;
    
    // 2. Client generates Neural Key (client-side only!)
    let neural_key = NeuralKey::generate()?;
    
    // 3. Client creates backup shares
    let shards = split_neural_key(&neural_key)?;
    // Store shards securely with trusted parties
    
    // 4. Create identity
    let identity_id = create_identity(&*services.identity_core, &neural_key).await?;
    
    // 5. Attach email credential for recovery
    services.auth_methods
        .attach_email_credential(
            identity_id,
            "user@example.com".to_string(),
            "secure_password_123".to_string(),
        )
        .await?;
    
    // 6. Setup MFA
    let mfa_setup = services.auth_methods.setup_mfa(identity_id).await?;
    // User configures authenticator app...
    
    // 7. Enable MFA with first code
    services.auth_methods
        .enable_mfa(identity_id, "123456".to_string())
        .await?;
    
    println!("Registration complete for identity {}", identity_id);
    Ok(())
}
```

---

## Complete Example: Login Flow

```rust
async fn full_login_flow(
    services: &Services,
    machine_keypair: &MachineKeyPair,
    machine_id: Uuid,
) -> Result<SessionTokens, Box<dyn std::error::Error>> {
    // 1. Authenticate with machine key
    let auth_result = authenticate_with_machine_key(
        &*services.auth_methods,
        machine_keypair,
        machine_id,
        "192.168.1.1".to_string(),
        "MyApp/1.0".to_string(),
    ).await?;
    
    // 2. Handle MFA if required
    let mfa_verified = if auth_result.mfa_required {
        // Prompt user for MFA code
        let code = "123456".to_string(); // From user input
        services.auth_methods
            .verify_mfa(auth_result.identity_id, code)
            .await?
    } else {
        false
    };
    
    // 3. Create session
    let tokens = services.sessions
        .create_session(
            auth_result.identity_id,
            auth_result.machine_id,
            auth_result.namespace_id,
            mfa_verified,
            auth_result.capabilities,
            vec!["identity:read".to_string()],
        )
        .await?;
    
    println!("Login successful!");
    println!("Access token: {}...", &tokens.access_token[..20]);
    
    Ok(tokens)
}
```

---

## Error Handling

All services return `Result` types with specific error enums:

```rust
use zid_crypto::CryptoError;
use zid_identity_core::IdentityCoreError;
use zid_methods::AuthMethodsError;
use zid_sessions::SessionError;

fn handle_auth_error(err: AuthMethodsError) {
    match err {
        AuthMethodsError::InvalidCredentials => {
            println!("Invalid email or password");
        }
        AuthMethodsError::MfaRequired => {
            println!("MFA verification required");
        }
        AuthMethodsError::MfaInvalid => {
            println!("Invalid MFA code");
        }
        AuthMethodsError::RateLimited { retry_after } => {
            println!("Too many attempts, retry after {} seconds", retry_after);
        }
        AuthMethodsError::IdentityFrozen => {
            println!("Account is frozen");
        }
        _ => {
            println!("Authentication error: {:?}", err);
        }
    }
}
```

---

## Security Considerations

1. **Neural Key**: Never transmit or store whole. Only public keys go to server.
2. **Shamir Shares**: Distribute to trusted parties. Never store together.
3. **Service Master Key**: Use HSM/KMS in production. Protects MFA secrets.
4. **Machine Keys**: Deterministic from Neural Key. Re-derivable if lost.
5. **Rate Limiting**: Policy engine enforces limits. Handle `RateLimited` errors.
6. **Token Rotation**: Refresh tokens have families. Reuse detection triggers revocation.
