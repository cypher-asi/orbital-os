# Desktop Environment (`orbital-desktop`)

**Layer:** 08  
**Status:** Specification  
**Crate:** `orbital-desktop`

---

## Overview

The Desktop Environment provides a **compositor and window manager** for the WASM (browser) target using WebGPU. It implements:

- Multiple **Desktops** (create/remove/switch)
- A **Void** overview mode (see all desktops, transition between them)
- **Infinite canvas** per desktop (pan + infinite zoom)
- **Windows** positioned in desktop world-space
- Optional **React presentation surfaces** inside windows via DOM overlay mounts

---

## Goals

Provide a drop-in Rust crate that can be embedded by the web OS as a subsystem. The crate owns:

- Compositor initialization and frame lifecycle
- Input handling and routing
- Desktop and window management
- Void mode and transitions
- DOM mount management for React surfaces
- Persistence hooks (host provides storage)

**Non-goals:** Full process model, security sandboxing, marketplace, or full UI toolkit.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    PRESENTATION LAYER (React)                    │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
│  │  App UI     │  │  Settings   │  │  Terminal   │  ... (React) │
│  │  Components │  │  Panel      │  │  Emulator   │              │
│  └─────────────┘  └─────────────┘  └─────────────┘              │
├─────────────────────────────────────────────────────────────────┤
│                      UI BRIDGE (DOM Mounts)                      │
│  - Creates/destroys mount elements per window                    │
│  - Aligns mounts to window content rect each frame              │
│  - Manages z-index stacking order                                │
├─────────────────────────────────────────────────────────────────┤
│                    COMPOSITOR (WebGPU)                           │
│  ┌─────────────────────────────────────────────────────────────┐│
│  │  Infinite Canvas  │  Window Chrome  │  Void Mode  │ Effects ││
│  │  (pan/zoom)       │  (frames)       │  (portals)  │ (blur)  ││
│  └─────────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────────┘
```

| Layer | Technology | Responsibility |
|-------|------------|----------------|
| **Compositor** | WebGPU | Infinite canvas, window compositing, void mode, visual effects |
| **UI Bridge** | DOM | Mount element lifecycle, alignment, z-ordering |
| **Presentation** | React | Actual UI content inside windows |

---

## Core Concepts

| Term | Definition |
|------|------------|
| **Desktop** | Infinite 2D world containing windows; each has its own camera |
| **Void** | Meta-scene showing all desktops as portals/thumbnails |
| **Window** | Movable/resizable rect in desktop world-space with z-order |
| **React Surface** | DOM mount element aligned to window's content rect |
| **Chrome** | Window frame/titlebar/shadow rendered by compositor |
| **Content Rect** | Inner area where app UI renders (GPU or React) |

---

## Key Design Decisions

### Ephemeral Window State

The desktop uses **local state** for UI state - **NOT Axiom**:

| State Type | Storage | Rationale |
|------------|---------|-----------|
| Window positions | Local (in-memory) | Ephemeral, user preference |
| Z-order / stacking | Local (in-memory) | Changes constantly |
| Focus state | Local (in-memory) | Ephemeral |
| Open windows | Local (in-memory) | Session-based |
| Window size | Local (in-memory) | User preference |

**What goes through Axiom:**
- Application launches (process creation)
- File operations triggered by apps
- System configuration changes

**What does NOT go through Axiom:**
- Window move/resize
- Focus changes
- Minimize/maximize
- Desktop arrangement

### Infinite Precision

World coordinates and camera center stored in `f64` on CPU. Origin rebasing each frame ensures GPU sees near-zero `f32` coords for stable behavior at extreme zoom levels.

---

## Embedding Model

Host provides:
- A `<canvas>` for WebGPU rendering
- A `<div>` root for DOM overlay (React mounts)
- A per-frame tick (typically `requestAnimationFrame`)

Crate provides:
- Initialization and rendering
- Input handling APIs (pointer/wheel/keyboard)
- Desktop/window management
- Void mode + transitions
- DOM mount management for React surfaces
- Persistence hooks (host chooses storage)

---

## Specification Files

| File | Description |
|------|-------------|
| [01-api.md](01-api.md) | Public API types and methods |
| [02-compositor.md](02-compositor.md) | Compositor structure and lifecycle |
| [03-desktops.md](03-desktops.md) | Desktop and Void management |
| [04-windows.md](04-windows.md) | Window management and chrome |
| [05-rendering.md](05-rendering.md) | WebGPU rendering requirements |
| [06-input.md](06-input.md) | Input handling and routing |
| [07-react-surfaces.md](07-react-surfaces.md) | React DOM integration |
| [08-transitions.md](08-transitions.md) | Transitions and animations |
| [09-persistence.md](09-persistence.md) | State persistence |
| [10-configuration.md](10-configuration.md) | Configuration and feature flags |
| [11-engineering.md](11-engineering.md) | Code quality constraints |
| [12-acceptance.md](12-acceptance.md) | Acceptance tests |
| [13-backgrounds.md](13-backgrounds.md) | Interactive procedural backgrounds |

---

*[Back to Spec Index](../README.md)*
