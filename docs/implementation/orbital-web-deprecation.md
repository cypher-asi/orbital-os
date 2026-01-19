# orbital-web Deprecation Plan

**Status:** Planned  
**Target:** Replace with `orbital-desktop` crate

---

## Overview

The `orbital-web` crate is being deprecated in favor of a new `orbital-desktop` crate. The existing implementation will be retained as a backup while the new crate is developed.

---

## Rationale

The current `orbital-web` implementation has grown organically and has several architectural limitations:

1. **Tight coupling** between compositor, window management, and React integration
2. **Code organization** does not follow strict size limits (60-line functions, 500-line files)
3. **Limited testability** due to deep WebGPU/DOM dependencies throughout
4. **Missing features** for a proper desktop compositor (Void mode, infinite canvas precision)

The new `orbital-desktop` crate addresses these with:

- Clean module boundaries with explicit trait abstractions
- Strict code quality constraints enforced by design
- Testable core logic (math, layout, transitions) without browser dependencies
- Full "Void" overview mode with animated transitions
- Infinite canvas support with f64 precision and origin rebasing

---

## Migration Strategy

### Phase 1: Backup and Parallel Development

1. **Rename** `crates/orbital-web/` to `crates/orbital-web-legacy/`
2. **Create** new `crates/orbital-desktop/` with clean architecture
3. **Update** workspace `Cargo.toml` to include both crates
4. **Keep** web frontend using legacy crate during development

### Phase 2: Feature Parity

Implement core features in `orbital-desktop`:

| Feature | orbital-web | orbital-desktop |
|---------|-------------|-----------------|
| Desktop management | Partial | Full (create/remove/switch) |
| Void overview | No | Yes |
| Infinite canvas | Basic | Full (f64 precision, origin rebasing) |
| Window management | Yes | Yes (improved API) |
| React surfaces | Yes | Yes (cleaner bridge) |
| Transitions | Basic | Full (easing, configurable) |
| Persistence | No | Yes (snapshot/restore) |

### Phase 3: Integration

1. **Update** web frontend to use `orbital-desktop`
2. **Migrate** existing window/desktop state
3. **Test** all existing functionality
4. **Remove** legacy imports

### Phase 4: Cleanup

1. **Archive** `orbital-web-legacy` (move to `archive/` or separate branch)
2. **Remove** from workspace
3. **Update** documentation

---

## File Mapping

### Legacy Structure (orbital-web)

```
crates/orbital-web/
├── src/
│   ├── lib.rs
│   ├── background.rs
│   └── desktop/
│       ├── mod.rs        (1959 lines - too large)
│       ├── desktops.rs   (871 lines - too large)
│       ├── windows.rs    (641 lines - too large)
│       ├── input.rs
│       ├── transition.rs
│       └── types.rs
```

### New Structure (orbital-desktop)

```
crates/orbital-desktop/
├── src/
│   ├── lib.rs                 (public API facade)
│   ├── compositor/
│   │   ├── mod.rs             (coordination)
│   │   └── state.rs           (compositor state)
│   ├── scene/
│   │   ├── desktop.rs         (desktop scene)
│   │   └── void.rs            (void overview scene)
│   ├── window/
│   │   ├── manager.rs         (window lifecycle)
│   │   ├── chrome.rs          (frame rendering)
│   │   └── layout.rs          (constraints, snap)
│   ├── input/
│   │   ├── router.rs          (hit testing, routing)
│   │   ├── events.rs          (event types)
│   │   └── gestures.rs        (touch gestures)
│   ├── render/
│   │   ├── mod.rs             (render graph)
│   │   ├── pipeline.rs        (WebGPU pipelines)
│   │   └── passes/
│   │       ├── background.rs
│   │       ├── window.rs
│   │       └── effects.rs
│   ├── ui/
│   │   ├── bridge.rs          (DOM mount management)
│   │   └── mount.rs           (mount lifecycle)
│   ├── transition/
│   │   ├── system.rs          (transition engine)
│   │   └── easing.rs          (easing functions)
│   ├── math/
│   │   ├── camera.rs          (viewport, projection)
│   │   ├── geom.rs            (rect, vec2, size)
│   │   └── precision.rs       (origin rebasing)
│   └── persistence/
│       └── snapshot.rs        (state serialization)
```

---

## Backward Compatibility

During migration:

- Legacy crate remains functional
- No breaking changes to web frontend initially
- New crate provides similar (but improved) public API
- Gradual migration of consumers

---

## Timeline

| Phase | Description | Status |
|-------|-------------|--------|
| 1 | Backup and parallel setup | Pending |
| 2 | Feature parity in orbital-desktop | Pending |
| 3 | Web frontend migration | Pending |
| 4 | Legacy cleanup | Pending |

---

## References

- [Desktop Specification](../spec/08-desktop/README.md)
- [orbital-desktop crate](../../crates/orbital-desktop/)
