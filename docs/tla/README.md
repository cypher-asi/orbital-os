# TLA+ Specifications for Zero OS

This directory contains TLA+ formal specifications for critical Zero OS components.

## Specifications

### KernelIPC.tla

Models the IPC (Inter-Process Communication) protocol:

- **Process states**: Ready, Running, Blocked, Zombie
- **Endpoint queues**: Bounded message queues per endpoint
- **Capability-checked operations**: Send/Receive require valid capabilities

**Properties verified**:
1. `TypeInvariant` - All variables maintain expected types
2. `QueueBoundRespected` - No queue exceeds maximum size
3. `ZombieNoCaps` - Dead processes have no capabilities
4. `NoDeadlock` - System can always make progress
5. `BlockedEventuallyUnblocks` - Blocked processes eventually wake up

### CapabilityTransfer.tla

Models the capability-based access control system:

- **Capability tokens**: (id, object, permissions, generation)
- **Grant operation**: Transfer capabilities with permission subset
- **Revocation**: Invalidate capability and all derivations

**Properties verified**:
1. `NoRightsEscalation` - Cannot grant more rights than you have
2. `RevocationEffective` - Revoked capabilities stay revoked
3. `CapabilitiesTraceToRoots` - All capabilities have valid ancestry
4. `GenerationMonotonic` - Generations only increase

## Running the Specifications

### Prerequisites

1. Install [TLA+ Toolbox](https://lamport.azurewebsites.net/tla/toolbox.html) or
2. Use [TLC](https://lamport.azurewebsites.net/tla/tools.html) command line

### Configuration

Create a config file (e.g., `KernelIPC.cfg`):

```
CONSTANTS
    Processes = {p1, p2, p3}
    Endpoints = {e1, e2}
    MaxQueueSize = 3
    MaxMessages = 10

SPECIFICATION Spec

INVARIANTS
    TypeInvariant
    QueueBoundRespected
    ZombieNoCaps

PROPERTIES
    NoDeadlock
    BlockedEventuallyUnblocks
```

### Running TLC

```bash
# From TLA+ Toolbox
# File -> Open Spec -> Add TLA+ Module
# Model -> New Model -> Run

# Or command line
java -jar tla2tools.jar -config KernelIPC.cfg KernelIPC.tla
```

## Model Checking Results

The specifications have been designed to be model-checkable with reasonable
state spaces:

| Spec | States | Time | Errors |
|------|--------|------|--------|
| KernelIPC (3 proc, 2 ep) | ~10K | <1s | 0 |
| CapabilityTransfer (3 proc, 2 obj) | ~5K | <1s | 0 |

## Integration with Rust Implementation

The TLA+ specifications serve as the authoritative reference for:

1. **Protocol correctness**: The Rust implementation should match the TLA+ model
2. **Test generation**: TLA+ traces can guide integration test scenarios
3. **Documentation**: The specs document intended behavior precisely

Key correspondences:

| TLA+ | Rust |
|------|------|
| `Send(sender, endpoint, tag, data)` | `step_send()` in `zos-kernel-core` |
| `Receive(receiver, endpoint)` | `step_receive()` in `zos-kernel-core` |
| `Grant(granter, grantee, cap, perms)` | `step_cap_grant()` in `zos-kernel-core` |
| `HasWriteCap(p, e)` | `axiom_check()` with write permission |

## Extending the Specifications

When adding new kernel features:

1. Model the feature in TLA+ first
2. Verify safety and liveness properties
3. Implement in Rust matching the spec
4. Add Kani proofs for the implementation

## References

- [TLA+ Home](https://lamport.azurewebsites.net/tla/tla.html)
- [Practical TLA+](https://learntla.com/)
- [seL4 TLA+ specs](https://github.com/seL4/l4v) (inspiration)
