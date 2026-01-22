# Service Supervision

> Init supervises services, monitoring health and restarting failed processes.

## Overview

Supervision provides:

1. **Health Monitoring**: Detect crashed or unresponsive services
2. **Automatic Restart**: Restart services per policy
3. **Dependency Management**: Restart dependents when service fails
4. **Rate Limiting**: Prevent restart storms

## Supervision Model

```
┌─────────────────────────────────────────────────────────────────────┐
│                         Init Supervisor                              │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                     Service Registry                           │ │
│  │                                                               │ │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐        │ │
│  │  │ terminal │ │ storage  │ │ network  │ │  app1    │        │ │
│  │  │          │ │          │ │          │ │          │        │ │
│  │  │ PID: 2   │ │ PID: 3   │ │ PID: 4   │ │ PID: 5   │        │ │
│  │  │ Running  │ │ Running  │ │ Running  │ │ Stopped  │        │ │
│  │  │ Restarts │ │ Restarts │ │ Restarts │ │ Restarts │        │ │
│  │  │ : 0      │ │ : 1      │ │ : 3      │ │ : 0      │        │ │
│  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘        │ │
│  └───────────────────────────────────────────────────────────────┘ │
│                                                                     │
│  ┌───────────────────────────────────────────────────────────────┐ │
│  │                    Event Loop                                  │ │
│  │                                                               │ │
│  │  1. Receive messages (spawn/stop/status requests)             │ │
│  │  2. Check for child exit events                               │ │
│  │  3. Process restart timers                                     │ │
│  │  4. Health check unresponsive services                         │ │
│  └───────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

## Service States

```
                     ┌───────────────────┐
                     │                   │
                     ▼                   │
              ┌──────────┐               │
   ┌─────────▶│ Starting │───────────────┤
   │          └────┬─────┘               │
   │               │ ready               │
   │               ▼                     │
   │          ┌──────────┐               │
   │          │ Running  │───────────────┤
   │          └────┬─────┘               │
   │               │ crash/exit          │ restart timer
   │               ▼                     │
   │          ┌──────────┐               │
   │          │ Stopped  │───────────────┘
   │          └────┬─────┘
   │               │ (restart policy)
   │               │
   │ restart  ┌────▼─────┐
   └──────────│ Backoff  │
              └──────────┘
```

### State Machine

```rust
/// Service state.
#[derive(Clone, Debug)]
pub enum ServiceState {
    /// Service is starting up
    Starting {
        started_at: u64,
        timeout_at: u64,
    },
    /// Service is running
    Running {
        pid: ProcessId,
        started_at: u64,
    },
    /// Service has stopped
    Stopped {
        exit_code: Option<i32>,
        stopped_at: u64,
    },
    /// Waiting for restart (backoff)
    Backoff {
        retry_at: u64,
        attempt: u32,
    },
    /// Service disabled (manual stop)
    Disabled,
}
```

## Restart Policies

```rust
/// Restart policy for a service.
#[derive(Clone, Copy, Debug)]
pub enum RestartPolicy {
    /// Never restart automatically
    Never,
    
    /// Restart only on non-zero exit code
    OnFailure,
    
    /// Always restart (even on clean exit)
    Always,
    
    /// Restart with exponential backoff
    WithBackoff {
        /// Initial delay in milliseconds
        initial_delay_ms: u32,
        /// Maximum delay
        max_delay_ms: u32,
        /// Maximum attempts before giving up
        max_attempts: u32,
    },
}

impl RestartPolicy {
    /// Should the service be restarted?
    pub fn should_restart(&self, exit_code: Option<i32>, attempt: u32) -> bool {
        match self {
            RestartPolicy::Never => false,
            RestartPolicy::OnFailure => exit_code.map_or(true, |c| c != 0),
            RestartPolicy::Always => true,
            RestartPolicy::WithBackoff { max_attempts, .. } => {
                attempt < *max_attempts
            }
        }
    }
    
    /// Calculate backoff delay.
    pub fn backoff_delay(&self, attempt: u32) -> u64 {
        match self {
            RestartPolicy::WithBackoff { initial_delay_ms, max_delay_ms, .. } => {
                let delay = (*initial_delay_ms as u64) * (1 << attempt.min(10));
                delay.min(*max_delay_ms as u64)
            }
            _ => 0,
        }
    }
}
```

## Service Registry

```rust
/// Tracked service.
pub struct Service {
    /// Service name
    pub name: String,
    /// Binary path
    pub binary: String,
    /// Current state
    pub state: ServiceState,
    /// Restart policy
    pub restart_policy: RestartPolicy,
    /// Dependencies (names)
    pub depends_on: Vec<String>,
    /// Dependents (services that depend on us)
    pub dependents: Vec<String>,
    /// Restart count (for monitoring)
    pub restart_count: u32,
    /// Capabilities to grant
    pub capabilities: Vec<CapabilityGrant>,
}

/// Service registry.
pub struct ServiceRegistry {
    /// Services by name
    services: BTreeMap<String, Service>,
    /// PID to service name mapping
    pid_to_name: BTreeMap<ProcessId, String>,
    /// Pending restart timers
    restart_timers: BinaryHeap<RestartTimer>,
}

struct RestartTimer {
    service_name: String,
    restart_at: u64,
}
```

## Event Handling

### Child Exit

When a service exits:

```rust
fn handle_child_exit(&mut self, pid: ProcessId, exit_code: i32) {
    let name = match self.pid_to_name.remove(&pid) {
        Some(n) => n,
        None => return,  // Unknown process
    };
    
    let service = self.services.get_mut(&name).unwrap();
    
    debug(&format!(
        "supervisor: {} (pid {}) exited with code {}",
        name, pid.0, exit_code
    ));
    
    // Update state
    service.state = ServiceState::Stopped {
        exit_code: Some(exit_code),
        stopped_at: now(),
    };
    
    // Check restart policy
    if service.restart_policy.should_restart(
        Some(exit_code),
        service.restart_count
    ) {
        let delay = service.restart_policy.backoff_delay(service.restart_count);
        service.state = ServiceState::Backoff {
            retry_at: now() + delay,
            attempt: service.restart_count,
        };
        service.restart_count += 1;
        
        self.restart_timers.push(RestartTimer {
            service_name: name.clone(),
            restart_at: now() + delay,
        });
        
        debug(&format!(
            "supervisor: will restart {} in {}ms (attempt {})",
            name, delay, service.restart_count
        ));
    }
    
    // Notify dependents
    for dependent in &service.dependents.clone() {
        self.handle_dependency_stopped(dependent);
    }
}
```

### Restart Timer

Process pending restarts:

```rust
fn process_restart_timers(&mut self) {
    let now = now();
    
    while let Some(timer) = self.restart_timers.peek() {
        if timer.restart_at > now {
            break;  // No more ready timers
        }
        
        let timer = self.restart_timers.pop().unwrap();
        let service = self.services.get_mut(&timer.service_name).unwrap();
        
        // Only restart if still in Backoff state
        if let ServiceState::Backoff { .. } = service.state {
            self.start_service(&timer.service_name);
        }
    }
}
```

### Health Check

Optional periodic health checking:

```rust
/// Health check request.
pub const MSG_HEALTH_CHECK: u32 = 0x1001;
/// Health check response.
pub const MSG_HEALTH_OK: u32 = 0x1002;

fn check_service_health(&mut self, name: &str) -> bool {
    let service = self.services.get(name).unwrap();
    
    if let ServiceState::Running { pid, .. } = service.state {
        // Send health check
        if send(service_endpoint, MSG_HEALTH_CHECK, &[]).is_err() {
            return false;
        }
        
        // Wait for response with timeout
        let deadline = now() + HEALTH_CHECK_TIMEOUT;
        while now() < deadline {
            if let Some(msg) = receive(my_endpoint) {
                if msg.from == pid && msg.tag == MSG_HEALTH_OK {
                    return true;
                }
            }
            yield_now();
        }
        
        // Timeout - service unresponsive
        return false;
    }
    
    false
}
```

## Dependency Management

When a service fails, dependents may need to be restarted:

```rust
fn handle_dependency_stopped(&mut self, dependent_name: &str) {
    let dependent = self.services.get_mut(dependent_name).unwrap();
    
    // Check if any dependency is down
    let deps_ok = dependent.depends_on.iter().all(|dep| {
        self.services.get(dep)
            .map(|s| matches!(s.state, ServiceState::Running { .. }))
            .unwrap_or(false)
    });
    
    if !deps_ok {
        // Stop dependent until dependencies are back
        if let ServiceState::Running { pid, .. } = dependent.state {
            debug(&format!(
                "supervisor: stopping {} (dependency down)",
                dependent_name
            ));
            // Signal graceful stop
            send_stop_signal(pid);
        }
    }
}

fn handle_dependency_started(&mut self, started_name: &str) {
    // Check if any dependents can now be started
    for (name, service) in &mut self.services {
        if service.depends_on.contains(&started_name.to_string()) {
            let deps_ok = service.depends_on.iter().all(|dep| {
                self.services.get(dep)
                    .map(|s| matches!(s.state, ServiceState::Running { .. }))
                    .unwrap_or(false)
            });
            
            if deps_ok && matches!(service.state, ServiceState::Stopped { .. }) {
                self.start_service(name);
            }
        }
    }
}
```

## Supervision API

Init exposes an IPC API for service management:

```rust
/// Supervision message tags.
pub mod tags {
    /// Start a service
    pub const SVC_START: u32 = 0x2000;
    /// Stop a service
    pub const SVC_STOP: u32 = 0x2001;
    /// Restart a service
    pub const SVC_RESTART: u32 = 0x2002;
    /// Get service status
    pub const SVC_STATUS: u32 = 0x2003;
    /// List all services
    pub const SVC_LIST: u32 = 0x2004;
    /// Response with status
    pub const SVC_RESPONSE: u32 = 0x2080;
}

/// Service status response.
#[derive(Clone, Debug)]
pub struct ServiceStatus {
    pub name: String,
    pub state: String,  // "running", "stopped", "starting", "backoff"
    pub pid: Option<u32>,
    pub restart_count: u32,
    pub uptime_secs: Option<u64>,
}
```

## Rate Limiting

Prevent restart storms:

```rust
/// Maximum restarts within the rate limit window.
const MAX_RESTARTS_PER_WINDOW: u32 = 5;
/// Rate limit window duration (milliseconds).
const RATE_LIMIT_WINDOW_MS: u64 = 60_000;

impl ServiceRegistry {
    fn check_rate_limit(&self, name: &str) -> bool {
        let service = self.services.get(name).unwrap();
        
        // Count restarts in window
        let window_start = now().saturating_sub(RATE_LIMIT_WINDOW_MS);
        let recent_restarts = service.restart_count;  // Simplified
        
        if recent_restarts >= MAX_RESTARTS_PER_WINDOW {
            debug(&format!(
                "supervisor: {} hit restart rate limit, disabling",
                name
            ));
            return false;
        }
        
        true
    }
}
```

## WASM Considerations

On WASM, supervision is simpler:

- No SIGCHLD - must poll process status via kernel
- No fork - spawn is asynchronous
- Single-threaded supervisor loop
- Cooperative scheduling means health checks are less critical

```rust
fn supervision_loop() {
    loop {
        // 1. Handle pending messages
        while let Some(msg) = receive(my_endpoint) {
            handle_message(msg);
        }
        
        // 2. Check for dead children (poll kernel)
        check_dead_children();
        
        // 3. Process restart timers
        process_restart_timers();
        
        // 4. Yield to other processes
        yield_now();
    }
}
```
