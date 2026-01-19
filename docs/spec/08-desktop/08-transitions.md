# Transitions

**Component:** 08-desktop/08-transitions  
**Status:** Specification

---

## Overview

Transitions provide smooth animations between states: entering/exiting Void mode, switching desktops, and camera movements. All transitions are time-based with configurable easing.

---

## Transition System

```rust
/// Transition system
pub struct TransitionSystem {
    /// Active transitions
    active: Vec<ActiveTransition>,
}

/// An active transition
struct ActiveTransition {
    /// Transition definition
    transition: Transition,
    
    /// Start time
    started_at: f32,
    
    /// Total duration
    duration_ms: f32,
    
    /// Easing function
    easing: Easing,
}

/// Transition definitions
pub enum Transition {
    /// Enter Void mode (desktop -> void)
    EnterVoid {
        from_desktop: DesktopId,
        from_camera: Camera,
        to_camera: Camera,
    },
    
    /// Exit Void mode (void -> desktop)
    ExitVoid {
        to_desktop: DesktopId,
        from_camera: Camera,
        to_camera: Camera,
    },
    
    /// Desktop switch via Void
    DesktopSwitch {
        from_desktop: DesktopId,
        to_desktop: DesktopId,
        phase: SwitchPhase,
    },
    
    /// Camera animation within a desktop
    CameraMove {
        from: Camera,
        to: Camera,
    },
}

/// Desktop switch phases
enum SwitchPhase {
    ZoomOut,
    Pan,
    ZoomIn,
}
```

---

## Transition Specification

```rust
/// Transition configuration
pub struct TransitionSpec {
    /// Duration in milliseconds
    pub duration_ms: f32,
    
    /// Easing function
    pub easing: Easing,
    
    /// Transition style
    pub style: TransitionStyle,
}

impl Default for TransitionSpec {
    fn default() -> Self {
        Self {
            duration_ms: 300.0,
            easing: Easing::EaseOutCubic,
            style: TransitionStyle::Smooth,
        }
    }
}

/// Transition visual style
pub enum TransitionStyle {
    /// Smooth interpolation
    Smooth,
    
    /// Quick snap
    Snap,
    
    /// Bounce effect
    Bounce,
}
```

---

## Easing Functions

```rust
/// Easing function
#[derive(Clone, Copy, Debug)]
pub enum Easing {
    Linear,
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,
    EaseOutBack,
    EaseOutElastic,
}

impl Easing {
    /// Apply easing to normalized time [0, 1] -> [0, 1]
    pub fn apply(&self, t: f32) -> f32 {
        match self {
            Easing::Linear => t,
            Easing::EaseInQuad => t * t,
            Easing::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOutQuad => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Easing::EaseInCubic => t * t * t,
            Easing::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                }
            }
            Easing::EaseOutBack => {
                let c1 = 1.70158;
                let c3 = c1 + 1.0;
                1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
            }
            Easing::EaseOutElastic => {
                if t == 0.0 || t == 1.0 {
                    t
                } else {
                    let c4 = (2.0 * std::f32::consts::PI) / 3.0;
                    2.0_f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
                }
            }
        }
    }
}
```

---

## Transition Lifecycle

### Starting Transitions

```rust
impl TransitionSystem {
    pub fn new() -> Self {
        Self { active: Vec::new() }
    }
    
    /// Start a new transition
    pub fn start(&mut self, transition: Transition, spec: TransitionSpec) {
        // Cancel conflicting transitions
        self.cancel_conflicting(&transition);
        
        self.active.push(ActiveTransition {
            transition,
            started_at: 0.0, // Will be set on first update
            duration_ms: spec.duration_ms,
            easing: spec.easing,
        });
    }
    
    fn cancel_conflicting(&mut self, new: &Transition) {
        self.active.retain(|active| {
            !Self::conflicts(&active.transition, new)
        });
    }
    
    fn conflicts(a: &Transition, b: &Transition) -> bool {
        use Transition::*;
        matches!(
            (a, b),
            (EnterVoid { .. }, EnterVoid { .. })
                | (ExitVoid { .. }, ExitVoid { .. })
                | (EnterVoid { .. }, ExitVoid { .. })
                | (ExitVoid { .. }, EnterVoid { .. })
                | (CameraMove { .. }, CameraMove { .. })
        )
    }
}
```

### Updating Transitions

```rust
impl TransitionSystem {
    /// Update transitions, returns completed transition kinds
    pub fn update(&mut self, dt_ms: f32, time_ms: f32) -> Vec<TransitionKind> {
        let mut completed = Vec::new();
        
        self.active.retain_mut(|active| {
            // Set start time on first update
            if active.started_at == 0.0 {
                active.started_at = time_ms;
            }
            
            let elapsed = time_ms - active.started_at;
            let progress = (elapsed / active.duration_ms).min(1.0);
            
            if progress >= 1.0 {
                completed.push(active.transition.kind());
                false
            } else {
                true
            }
        });
        
        completed
    }
    
    /// Get current transition progress for a kind
    pub fn progress(&self, kind: TransitionKind, time_ms: f32) -> Option<f32> {
        self.active.iter()
            .find(|t| t.transition.kind() == kind)
            .map(|t| {
                let elapsed = time_ms - t.started_at;
                let linear = (elapsed / t.duration_ms).clamp(0.0, 1.0);
                t.easing.apply(linear)
            })
    }
    
    /// Check if any transition is active
    pub fn is_active(&self) -> bool {
        !self.active.is_empty()
    }
}

impl Transition {
    fn kind(&self) -> TransitionKind {
        match self {
            Transition::EnterVoid { .. } => TransitionKind::EnterVoid,
            Transition::ExitVoid { .. } => TransitionKind::ExitVoid,
            Transition::DesktopSwitch { .. } => TransitionKind::DesktopSwitch,
            Transition::CameraMove { .. } => TransitionKind::CameraMove,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionKind {
    EnterVoid,
    ExitVoid,
    DesktopSwitch,
    CameraMove,
}
```

---

## Applying Transition Effects

```rust
impl Compositor {
    fn apply_transition_effects(&mut self, time_ms: f32) {
        for active in &self.transitions.active {
            let progress = {
                let elapsed = time_ms - active.started_at;
                let linear = (elapsed / active.duration_ms).clamp(0.0, 1.0);
                active.easing.apply(linear)
            };
            
            match &active.transition {
                Transition::EnterVoid { from_camera, to_camera, .. } => {
                    self.state.void_camera = Camera::lerp(from_camera, to_camera, progress);
                }
                
                Transition::ExitVoid { to_desktop, from_camera, to_camera } => {
                    if let Some(desktop) = self.state.desktops.get_mut(to_desktop) {
                        desktop.camera = Camera::lerp(from_camera, to_camera, progress);
                    }
                }
                
                Transition::CameraMove { from, to } => {
                    let desktop = self.state.active_desktop_mut();
                    desktop.camera = Camera::lerp(from, to, progress);
                }
                
                Transition::DesktopSwitch { .. } => {
                    // Multi-phase transition - handled separately
                }
            }
        }
    }
}
```

---

## Camera Interpolation

```rust
impl Camera {
    /// Linear interpolation between cameras
    pub fn lerp(from: &Camera, to: &Camera, t: f32) -> Camera {
        Camera {
            center: (
                lerp_f64(from.center.0, to.center.0, t),
                lerp_f64(from.center.1, to.center.1, t),
            ),
            zoom: lerp_f32(from.zoom, to.zoom, t),
        }
    }
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_f64(a: f64, b: f64, t: f32) -> f64 {
    a + (b - a) * t as f64
}
```

---

## Void Transition Details

### Enter Void

```rust
impl Compositor {
    pub fn enter_void(&mut self) {
        if self.state.mode == Mode::Void {
            return;
        }
        
        let from_desktop = self.state.active_desktop;
        let from_camera = self.state.active_desktop().camera.clone();
        
        // Calculate target void camera to show all portals
        let portals = self.calculate_void_portals();
        let to_camera = self.calculate_camera_to_fit(&portals);
        
        self.transitions.start(
            Transition::EnterVoid {
                from_desktop,
                from_camera,
                to_camera: to_camera.clone(),
            },
            TransitionSpec::default(),
        );
        
        self.state.void_camera = from_camera; // Start from desktop camera
        self.state.mode = Mode::Void;
        
        self.events.push(Event::ModeChanged { mode: Mode::Void });
    }
    
    fn calculate_camera_to_fit(&self, portals: &[VoidPortal]) -> Camera {
        if portals.is_empty() {
            return Camera::default();
        }
        
        // Find bounding box of all portals
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        
        for portal in portals {
            min_x = min_x.min(portal.rect.x);
            min_y = min_y.min(portal.rect.y);
            max_x = max_x.max(portal.rect.x + portal.rect.width as f64);
            max_y = max_y.max(portal.rect.y + portal.rect.height as f64);
        }
        
        let center = ((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
        let bounds_w = (max_x - min_x) as f32;
        let bounds_h = (max_y - min_y) as f32;
        
        // Calculate zoom to fit with padding
        let screen = self.screen_size();
        let padding = 100.0;
        let zoom_w = (screen.0 - padding * 2.0) / bounds_w;
        let zoom_h = (screen.1 - padding * 2.0) / bounds_h;
        let zoom = zoom_w.min(zoom_h).min(1.0);
        
        Camera { center, zoom }
    }
}
```

### Exit Void

```rust
impl Compositor {
    pub fn exit_void(&mut self, target: DesktopId) {
        if self.state.mode != Mode::Void {
            return;
        }
        
        let to_desktop = match self.state.desktops.get(&target) {
            Some(_) => target,
            None => return,
        };
        
        let from_camera = self.state.void_camera.clone();
        let to_camera = self.state.desktops[&to_desktop].camera.clone();
        
        self.transitions.start(
            Transition::ExitVoid {
                to_desktop,
                from_camera,
                to_camera,
            },
            TransitionSpec::default(),
        );
        
        self.state.active_desktop = to_desktop;
        self.state.mode = Mode::Desktop;
        
        self.events.push(Event::ModeChanged { mode: Mode::Desktop });
        self.events.push(Event::DesktopChanged { desktop: to_desktop });
    }
}
```

---

## Time Source Abstraction

For testability:

```rust
/// Time source trait (for deterministic testing)
pub trait TimeSource {
    fn now_ms(&self) -> f32;
}

/// Real time source using performance.now()
pub struct RealTimeSource;

impl TimeSource for RealTimeSource {
    fn now_ms(&self) -> f32 {
        web_sys::window()
            .and_then(|w| w.performance())
            .map(|p| p.now() as f32)
            .unwrap_or(0.0)
    }
}

/// Mock time source for testing
#[cfg(test)]
pub struct MockTimeSource {
    time: std::cell::Cell<f32>,
}

#[cfg(test)]
impl MockTimeSource {
    pub fn new() -> Self {
        Self { time: std::cell::Cell::new(0.0) }
    }
    
    pub fn advance(&self, ms: f32) {
        self.time.set(self.time.get() + ms);
    }
}

#[cfg(test)]
impl TimeSource for MockTimeSource {
    fn now_ms(&self) -> f32 {
        self.time.get()
    }
}
```

---

## Module Structure

```
transition/
├── system.rs       # TransitionSystem, lifecycle
├── easing.rs       # Easing functions
├── types.rs        # Transition, TransitionSpec, TransitionKind
└── interpolation.rs # Camera lerp, value interpolation
```

---

*[Back to Desktop](README.md) | [Previous: React Surfaces](07-react-surfaces.md) | [Next: Persistence](09-persistence.md)*
