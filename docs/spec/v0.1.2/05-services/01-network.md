# Network Service

> Network connectivity for applications via HTTP, WebSocket, and DNS.

## Overview

The Network Service provides network access for applications. It is:

1. **Policy-controlled**: URL access governed by network policy
2. **Rate-limited**: Prevents abuse through request limiting
3. **Cross-platform**: Fetch API on WASM, sockets on native
4. **Optional**: ZOS can run without network connectivity

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Network Service                                        │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Fetch Backend (WASM)                                 │ │
│  │                                                                         │ │
│  │  • HTTP GET/POST/PUT/DELETE                                            │ │
│  │  • Headers management                                                  │ │
│  │  • CORS handling                                                       │ │
│  │  • Response streaming                                                  │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Network Policy                                       │ │
│  │                                                                         │ │
│  │  Allow: *.api.example.com                                              │ │
│  │  Allow: cdn.example.com                                                │ │
│  │  Deny: *.malware.com                                                   │ │
│  │  Default: Deny for applications                                        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                    Rate Limiter                                         │ │
│  │                                                                         │ │
│  │  Per-process limits: 100 requests/minute                               │ │
│  │  Per-host limits: 10 requests/second                                   │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  Message Handlers:                                                           │
│  • MSG_NET_REQUEST  → make HTTP request                                     │
│  • MSG_WS_OPEN      → open WebSocket connection                             │
│  • MSG_WS_MESSAGE   → send/receive WebSocket message                        │
│  • MSG_WS_CLOSE     → close WebSocket connection                            │
│  • MSG_NET_POLICY   → query/update policy                                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Data Structures

### HTTP Request

```rust
use serde::{Serialize, Deserialize};

/// HTTP request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HttpRequest {
    /// HTTP method
    pub method: HttpMethod,
    
    /// URL to fetch
    pub url: String,
    
    /// Request headers
    pub headers: Vec<(String, String)>,
    
    /// Request body (for POST/PUT)
    pub body: Option<Vec<u8>>,
    
    /// Timeout in milliseconds
    pub timeout_ms: u32,
}

/// HTTP methods.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

/// HTTP response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HttpResponse {
    pub result: Result<HttpSuccess, NetworkError>,
}

/// Successful HTTP response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HttpSuccess {
    /// HTTP status code
    pub status: u16,
    
    /// Response headers
    pub headers: Vec<(String, String)>,
    
    /// Response body
    pub body: Vec<u8>,
}

/// Network errors.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NetworkError {
    /// URL not allowed by policy
    PolicyDenied,
    
    /// DNS resolution failed
    DnsError,
    
    /// Connection failed
    ConnectionFailed,
    
    /// Request timed out
    Timeout,
    
    /// Invalid URL
    InvalidUrl,
    
    /// CORS error (WASM)
    CorsError,
    
    /// Rate limit exceeded
    RateLimitExceeded,
    
    /// Other error
    Other(String),
}
```

### WebSocket

```rust
/// WebSocket open request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsOpenRequest {
    /// WebSocket URL (ws:// or wss://)
    pub url: String,
    
    /// Subprotocols
    pub protocols: Vec<String>,
}

/// WebSocket opened response.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsOpened {
    /// Connection ID
    pub conn_id: u64,
    
    /// Endpoint for receiving messages
    pub message_endpoint: CapSlot,
}

/// WebSocket message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsMessage {
    /// Connection ID
    pub conn_id: u64,
    
    /// Message data
    pub data: WsData,
}

/// WebSocket data types.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WsData {
    Text(String),
    Binary(Vec<u8>),
}

/// WebSocket close request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WsCloseRequest {
    /// Connection ID
    pub conn_id: u64,
    
    /// Close code
    pub code: Option<u16>,
    
    /// Close reason
    pub reason: Option<String>,
}
```

## Network Policy

### Policy Rules

```rust
/// Network policy rule.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkPolicyRule {
    /// Rule ID
    pub id: String,
    
    /// Process class this applies to
    pub applies_to: ProcessClass,
    
    /// URL pattern (glob)
    pub url_pattern: String,
    
    /// Whether to allow or deny
    pub allow: bool,
    
    /// Priority (higher = evaluated first)
    pub priority: u32,
}

/// Process classes for policy matching.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessClass {
    /// System services
    System,
    /// Runtime services
    Runtime,
    /// User applications
    Application,
    /// Specific process by name
    Named(String),
}
```

### Default Policy

```rust
/// Default network policy rules.
fn default_policy() -> Vec<NetworkPolicyRule> {
    vec![
        // System services can access anything
        NetworkPolicyRule {
            id: "system-allow-all".to_string(),
            applies_to: ProcessClass::System,
            url_pattern: "*".to_string(),
            allow: true,
            priority: 100,
        },
        
        // Runtime services can access anything
        NetworkPolicyRule {
            id: "runtime-allow-all".to_string(),
            applies_to: ProcessClass::Runtime,
            url_pattern: "*".to_string(),
            allow: true,
            priority: 90,
        },
        
        // Apps can access HTTPS only
        NetworkPolicyRule {
            id: "app-https-only".to_string(),
            applies_to: ProcessClass::Application,
            url_pattern: "https://*".to_string(),
            allow: true,
            priority: 50,
        },
        
        // Block known bad domains
        NetworkPolicyRule {
            id: "block-malware".to_string(),
            applies_to: ProcessClass::Application,
            url_pattern: "*.malware.example.com".to_string(),
            allow: false,
            priority: 80,
        },
        
        // Default deny for apps
        NetworkPolicyRule {
            id: "app-default-deny".to_string(),
            applies_to: ProcessClass::Application,
            url_pattern: "*".to_string(),
            allow: false,
            priority: 1,
        },
    ]
}
```

### Policy Checking

```rust
impl NetworkService {
    fn check_policy(&self, caller: ProcessId, url: &str) -> Result<(), NetworkError> {
        let class = self.classify_process(caller);
        
        // Parse URL to extract host
        let host = parse_url_host(url)
            .map_err(|_| NetworkError::InvalidUrl)?;
        
        // Get rules sorted by priority (descending)
        let mut rules: Vec<_> = self.policy.iter()
            .filter(|r| self.rule_applies(r, &class))
            .collect();
        rules.sort_by_key(|r| std::cmp::Reverse(r.priority));
        
        // First matching rule wins
        for rule in rules {
            if glob_match(&rule.url_pattern, url) || glob_match(&rule.url_pattern, &host) {
                if rule.allow {
                    return Ok(());
                } else {
                    return Err(NetworkError::PolicyDenied);
                }
            }
        }
        
        // No matching rule - deny by default
        Err(NetworkError::PolicyDenied)
    }
    
    fn rule_applies(&self, rule: &NetworkPolicyRule, class: &ProcessClass) -> bool {
        match &rule.applies_to {
            ProcessClass::System => *class == ProcessClass::System,
            ProcessClass::Runtime => *class == ProcessClass::Runtime,
            ProcessClass::Application => *class == ProcessClass::Application,
            ProcessClass::Named(name) => {
                if let ProcessClass::Named(c) = class {
                    c == name
                } else {
                    false
                }
            }
        }
    }
}
```

## Rate Limiting

```rust
/// Rate limiter for network requests.
struct RateLimiter {
    /// Process -> (count, window_start)
    per_process: BTreeMap<ProcessId, (u32, u64)>,
    
    /// Host -> (count, window_start)
    per_host: BTreeMap<String, (u32, u64)>,
}

impl RateLimiter {
    const PROCESS_LIMIT: u32 = 100;      // requests per minute
    const PROCESS_WINDOW: u64 = 60_000_000_000;  // 1 minute in nanos
    
    const HOST_LIMIT: u32 = 10;          // requests per second
    const HOST_WINDOW: u64 = 1_000_000_000;  // 1 second in nanos
    
    fn check_and_record(&mut self, pid: ProcessId, host: &str, now: u64) -> bool {
        // Check per-process limit
        if !self.check_limit(&mut self.per_process, pid, Self::PROCESS_LIMIT, Self::PROCESS_WINDOW, now) {
            return false;
        }
        
        // Check per-host limit
        if !self.check_limit_by_key(&mut self.per_host, host, Self::HOST_LIMIT, Self::HOST_WINDOW, now) {
            return false;
        }
        
        true
    }
    
    fn check_limit<K: Ord + Clone>(
        &self,
        map: &mut BTreeMap<K, (u32, u64)>,
        key: K,
        limit: u32,
        window: u64,
        now: u64,
    ) -> bool {
        let entry = map.entry(key).or_insert((0, now));
        
        // Reset window if expired
        if now - entry.1 > window {
            *entry = (0, now);
        }
        
        // Check limit
        if entry.0 >= limit {
            return false;
        }
        
        entry.0 += 1;
        true
    }
}
```

## IPC Protocol

### Message Types

```rust
pub mod net_msg {
    /// HTTP request.
    pub const MSG_NET_REQUEST: u32 = 0x9000;
    /// HTTP response.
    pub const MSG_NET_RESPONSE: u32 = 0x9001;
    
    /// WebSocket open.
    pub const MSG_WS_OPEN: u32 = 0x9010;
    /// WebSocket opened.
    pub const MSG_WS_OPENED: u32 = 0x9011;
    /// WebSocket message (bidirectional).
    pub const MSG_WS_MESSAGE: u32 = 0x9012;
    /// WebSocket close.
    pub const MSG_WS_CLOSE: u32 = 0x9013;
    /// WebSocket closed.
    pub const MSG_WS_CLOSED: u32 = 0x9014;
    /// WebSocket error.
    pub const MSG_WS_ERROR: u32 = 0x9015;
    
    /// Query network policy.
    pub const MSG_NET_POLICY_QUERY: u32 = 0x9020;
    /// Network policy response.
    pub const MSG_NET_POLICY_RESPONSE: u32 = 0x9021;
    /// Update network policy (admin only).
    pub const MSG_NET_POLICY_UPDATE: u32 = 0x9022;
}
```

## WASM Backend

### Fetch Implementation

```javascript
class NetworkBackend {
    async fetch(request) {
        const { method, url, headers, body, timeout_ms } = request;
        
        // Create abort controller for timeout
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), timeout_ms);
        
        try {
            const response = await fetch(url, {
                method,
                headers: new Headers(headers),
                body: body ? new Uint8Array(body) : undefined,
                signal: controller.signal,
            });
            
            clearTimeout(timeoutId);
            
            // Read response
            const responseBody = await response.arrayBuffer();
            const responseHeaders = [];
            response.headers.forEach((value, key) => {
                responseHeaders.push([key, value]);
            });
            
            return {
                status: response.status,
                headers: responseHeaders,
                body: new Uint8Array(responseBody),
            };
        } catch (error) {
            clearTimeout(timeoutId);
            
            if (error.name === 'AbortError') {
                throw { type: 'Timeout' };
            }
            throw { type: 'ConnectionFailed', message: error.message };
        }
    }
}
```

### WebSocket Implementation

```javascript
class WebSocketBackend {
    constructor() {
        this.connections = new Map();
        this.nextConnId = 1;
    }
    
    open(url, protocols, messageCallback, closeCallback) {
        const connId = this.nextConnId++;
        const ws = new WebSocket(url, protocols);
        
        ws.onopen = () => {
            // Connection established
        };
        
        ws.onmessage = (event) => {
            const data = event.data instanceof ArrayBuffer
                ? { type: 'Binary', data: new Uint8Array(event.data) }
                : { type: 'Text', data: event.data };
            messageCallback(connId, data);
        };
        
        ws.onclose = (event) => {
            this.connections.delete(connId);
            closeCallback(connId, event.code, event.reason);
        };
        
        ws.onerror = (error) => {
            // Handle error
        };
        
        this.connections.set(connId, ws);
        return connId;
    }
    
    send(connId, data) {
        const ws = this.connections.get(connId);
        if (!ws) throw new Error('Connection not found');
        
        if (data.type === 'Binary') {
            ws.send(data.data);
        } else {
            ws.send(data.data);
        }
    }
    
    close(connId, code, reason) {
        const ws = this.connections.get(connId);
        if (ws) {
            ws.close(code, reason);
            this.connections.delete(connId);
        }
    }
}
```

## Service Implementation

```rust
#![no_std]
extern crate alloc;
extern crate zero_process;

use zero_process::*;

#[no_mangle]
pub extern "C" fn _start() {
    debug("network: starting");
    
    // Load network policy
    let policy = load_policy_or_default();
    let rate_limiter = RateLimiter::new();
    
    let service_ep = create_endpoint();
    register_service("network", service_ep);
    send_ready();
    
    loop {
        let msg = receive_blocking(service_ep);
        match msg.tag {
            net_msg::MSG_NET_REQUEST => handle_http_request(msg, &policy, &mut rate_limiter),
            net_msg::MSG_WS_OPEN => handle_ws_open(msg, &policy),
            net_msg::MSG_WS_MESSAGE => handle_ws_message(msg),
            net_msg::MSG_WS_CLOSE => handle_ws_close(msg),
            net_msg::MSG_NET_POLICY_QUERY => handle_policy_query(msg),
            net_msg::MSG_NET_POLICY_UPDATE => handle_policy_update(msg),
            _ => debug("network: unknown message"),
        }
    }
}

fn handle_http_request(msg: ReceivedMessage, policy: &NetworkPolicy, limiter: &mut RateLimiter) {
    let request: HttpRequest = decode(&msg.data);
    let reply_ep = msg.cap_slots.get(0);
    
    // Check policy
    if let Err(e) = policy.check(msg.from, &request.url) {
        send_error(reply_ep, e);
        return;
    }
    
    // Check rate limit
    let host = parse_host(&request.url).unwrap_or_default();
    if !limiter.check_and_record(msg.from, &host, current_timestamp()) {
        send_error(reply_ep, NetworkError::RateLimitExceeded);
        return;
    }
    
    // Make request via backend
    match backend_fetch(&request) {
        Ok(response) => send_response(reply_ep, HttpResponse { result: Ok(response) }),
        Err(e) => send_response(reply_ep, HttpResponse { result: Err(e) }),
    }
}
```

## Native Backend (Future)

For native targets, the network service uses sockets:

```rust
// native_network.rs (future)

struct NativeNetwork {
    dns_resolver: DnsResolver,
    connection_pool: ConnectionPool,
}

impl NativeNetwork {
    async fn http_request(&self, request: HttpRequest) -> Result<HttpSuccess, NetworkError> {
        // 1. Parse URL
        let url = Url::parse(&request.url)
            .map_err(|_| NetworkError::InvalidUrl)?;
        
        // 2. DNS resolution
        let addr = self.dns_resolver.resolve(url.host())
            .await
            .map_err(|_| NetworkError::DnsError)?;
        
        // 3. TCP connection (with TLS for HTTPS)
        let stream = if url.scheme() == "https" {
            self.connection_pool.get_tls(addr, url.host()).await?
        } else {
            self.connection_pool.get_tcp(addr).await?
        };
        
        // 4. Send HTTP request
        let http_request = format_http_request(&request);
        stream.write_all(&http_request).await?;
        
        // 5. Read response
        let response = parse_http_response(&stream).await?;
        
        Ok(response)
    }
}
```

## Invariants

1. **Policy enforcement**: All requests checked against policy
2. **Rate limiting**: Requests within limits
3. **Connection tracking**: WebSocket connections properly tracked
4. **Timeout enforcement**: All requests have finite timeout

## Security Considerations

1. **URL validation**: Reject malformed URLs
2. **CORS handling**: Respect browser CORS policies (WASM)
3. **TLS enforcement**: HTTPS required for applications by default
4. **Rate limiting**: Prevent DoS attacks
5. **Policy isolation**: Apps cannot modify their own network policy

## Related Specifications

- [README.md](README.md) - Runtime services overview
- [../05-identity/04-permissions.md](../05-identity/04-permissions.md) - Capability-based access
- [../04-init/02-supervision.md](../04-init/02-supervision.md) - Service supervision
