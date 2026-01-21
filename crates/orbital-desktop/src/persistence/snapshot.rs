//! Snapshot serialization for desktop state

use serde::{Deserialize, Serialize};
use crate::desktop::PersistedDesktop;

/// Snapshot of desktop state for persistence
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    /// Version for migration support
    pub version: u32,
    /// Active desktop index
    pub active_desktop: usize,
    /// Persisted desktop data
    pub desktops: Vec<PersistedDesktop>,
}

impl Snapshot {
    /// Current snapshot version
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a new snapshot
    pub fn new(active_desktop: usize, desktops: Vec<PersistedDesktop>) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            active_desktop,
            desktops,
        }
    }

    /// Check if snapshot needs migration
    pub fn needs_migration(&self) -> bool {
        self.version < Self::CURRENT_VERSION
    }

    /// Migrate snapshot to current version
    pub fn migrate(&mut self) {
        // Add migration logic as versions increase
        self.version = Self::CURRENT_VERSION;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Camera, Vec2};

    #[test]
    fn test_snapshot_creation() {
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "Main".to_string(),
                camera: Camera::new(),
                background: "grain".to_string(),
            },
        ];
        let snapshot = Snapshot::new(0, desktops);
        
        assert_eq!(snapshot.version, Snapshot::CURRENT_VERSION);
        assert_eq!(snapshot.active_desktop, 0);
        assert_eq!(snapshot.desktops.len(), 1);
    }

    #[test]
    fn test_snapshot_serialization() {
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "Main".to_string(),
                camera: Camera::at(Vec2::new(100.0, 200.0), 1.5),
                background: "grain".to_string(),
            },
        ];
        let snapshot = Snapshot::new(0, desktops);
        
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: Snapshot = serde_json::from_str(&json).unwrap();
        
        assert_eq!(restored.desktops[0].name, "Main");
        assert!((restored.desktops[0].camera.center.x - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_snapshot_default() {
        let snapshot: Snapshot = Default::default();
        
        assert_eq!(snapshot.version, 0); // Default doesn't set CURRENT_VERSION
        assert_eq!(snapshot.active_desktop, 0);
        assert!(snapshot.desktops.is_empty());
    }

    #[test]
    fn test_snapshot_empty_desktops() {
        let snapshot = Snapshot::new(0, vec![]);
        
        assert_eq!(snapshot.version, Snapshot::CURRENT_VERSION);
        assert!(snapshot.desktops.is_empty());
        
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: Snapshot = serde_json::from_str(&json).unwrap();
        
        assert!(restored.desktops.is_empty());
    }

    #[test]
    fn test_snapshot_multiple_desktops() {
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "Main".to_string(),
                camera: Camera::at(Vec2::new(0.0, 0.0), 1.0),
                background: "grain".to_string(),
            },
            PersistedDesktop {
                id: 2,
                name: "Work".to_string(),
                camera: Camera::at(Vec2::new(2000.0, 0.0), 1.2),
                background: "mist".to_string(),
            },
            PersistedDesktop {
                id: 3,
                name: "Gaming".to_string(),
                camera: Camera::at(Vec2::new(4000.0, 0.0), 0.8),
                background: "grain".to_string(),
            },
        ];
        let snapshot = Snapshot::new(1, desktops);
        
        assert_eq!(snapshot.active_desktop, 1);
        assert_eq!(snapshot.desktops.len(), 3);
        
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: Snapshot = serde_json::from_str(&json).unwrap();
        
        assert_eq!(restored.active_desktop, 1);
        assert_eq!(restored.desktops.len(), 3);
        assert_eq!(restored.desktops[0].name, "Main");
        assert_eq!(restored.desktops[1].name, "Work");
        assert_eq!(restored.desktops[2].name, "Gaming");
    }

    #[test]
    fn test_snapshot_camera_state_preservation() {
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "Test".to_string(),
                camera: Camera::at(Vec2::new(-500.0, 300.0), 2.5),
                background: "mist".to_string(),
            },
        ];
        let snapshot = Snapshot::new(0, desktops);
        
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: Snapshot = serde_json::from_str(&json).unwrap();
        
        let camera = &restored.desktops[0].camera;
        assert!((camera.center.x - (-500.0)).abs() < 0.001);
        assert!((camera.center.y - 300.0).abs() < 0.001);
        assert!((camera.zoom - 2.5).abs() < 0.001);
    }

    #[test]
    fn test_snapshot_background_preservation() {
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "Grain Desktop".to_string(),
                camera: Camera::new(),
                background: "grain".to_string(),
            },
            PersistedDesktop {
                id: 2,
                name: "Mist Desktop".to_string(),
                camera: Camera::new(),
                background: "mist".to_string(),
            },
        ];
        let snapshot = Snapshot::new(0, desktops);
        
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: Snapshot = serde_json::from_str(&json).unwrap();
        
        assert_eq!(restored.desktops[0].background, "grain");
        assert_eq!(restored.desktops[1].background, "mist");
    }

    #[test]
    fn test_snapshot_needs_migration() {
        let mut snapshot = Snapshot::new(0, vec![]);
        assert!(!snapshot.needs_migration());
        
        // Simulate old version
        snapshot.version = 0;
        assert!(snapshot.needs_migration());
    }

    #[test]
    fn test_snapshot_migrate() {
        let mut snapshot = Snapshot {
            version: 0,
            active_desktop: 2,
            desktops: vec![
                PersistedDesktop {
                    id: 1,
                    name: "Old".to_string(),
                    camera: Camera::new(),
                    background: "grain".to_string(),
                },
            ],
        };
        
        assert!(snapshot.needs_migration());
        
        snapshot.migrate();
        
        assert!(!snapshot.needs_migration());
        assert_eq!(snapshot.version, Snapshot::CURRENT_VERSION);
        // Data should be preserved
        assert_eq!(snapshot.active_desktop, 2);
        assert_eq!(snapshot.desktops[0].name, "Old");
    }

    #[test]
    fn test_snapshot_clone() {
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "Main".to_string(),
                camera: Camera::at(Vec2::new(100.0, 200.0), 1.5),
                background: "grain".to_string(),
            },
        ];
        let snapshot = Snapshot::new(0, desktops);
        let cloned = snapshot.clone();
        
        assert_eq!(cloned.version, snapshot.version);
        assert_eq!(cloned.active_desktop, snapshot.active_desktop);
        assert_eq!(cloned.desktops.len(), snapshot.desktops.len());
    }

    #[test]
    fn test_snapshot_full_roundtrip() {
        // Create a complex snapshot
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "Desktop 1".to_string(),
                camera: Camera::at(Vec2::new(0.0, 0.0), 1.0),
                background: "grain".to_string(),
            },
            PersistedDesktop {
                id: 2,
                name: "Desktop 2".to_string(),
                camera: Camera::at(Vec2::new(2020.0, 0.0), 1.5),
                background: "mist".to_string(),
            },
            PersistedDesktop {
                id: 3,
                name: "Desktop 3".to_string(),
                camera: Camera::at(Vec2::new(4040.0, 0.0), 0.75),
                background: "grain".to_string(),
            },
        ];
        let original = Snapshot::new(1, desktops);
        
        // Serialize
        let json = serde_json::to_string_pretty(&original).unwrap();
        
        // Deserialize
        let restored: Snapshot = serde_json::from_str(&json).unwrap();
        
        // Verify all fields
        assert_eq!(restored.version, original.version);
        assert_eq!(restored.active_desktop, original.active_desktop);
        assert_eq!(restored.desktops.len(), original.desktops.len());
        
        for (orig, rest) in original.desktops.iter().zip(restored.desktops.iter()) {
            assert_eq!(orig.id, rest.id);
            assert_eq!(orig.name, rest.name);
            assert_eq!(orig.background, rest.background);
            assert!((orig.camera.center.x - rest.camera.center.x).abs() < 0.001);
            assert!((orig.camera.center.y - rest.camera.center.y).abs() < 0.001);
            assert!((orig.camera.zoom - rest.camera.zoom).abs() < 0.001);
        }
    }

    #[test]
    fn test_snapshot_json_structure() {
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "Test".to_string(),
                camera: Camera::new(),
                background: "grain".to_string(),
            },
        ];
        let snapshot = Snapshot::new(0, desktops);
        
        let json = serde_json::to_string(&snapshot).unwrap();
        
        // Verify JSON contains expected keys
        assert!(json.contains("\"version\""));
        assert!(json.contains("\"active_desktop\""));
        assert!(json.contains("\"desktops\""));
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"camera\""));
        assert!(json.contains("\"background\""));
    }

    #[test]
    fn test_snapshot_special_characters_in_name() {
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "Work & Play \"Special\" <Test>".to_string(),
                camera: Camera::new(),
                background: "grain".to_string(),
            },
        ];
        let snapshot = Snapshot::new(0, desktops);
        
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: Snapshot = serde_json::from_str(&json).unwrap();
        
        assert_eq!(restored.desktops[0].name, "Work & Play \"Special\" <Test>");
    }

    #[test]
    fn test_snapshot_unicode_name() {
        let desktops = vec![
            PersistedDesktop {
                id: 1,
                name: "工作桌面 🖥️".to_string(),
                camera: Camera::new(),
                background: "grain".to_string(),
            },
        ];
        let snapshot = Snapshot::new(0, desktops);
        
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: Snapshot = serde_json::from_str(&json).unwrap();
        
        assert_eq!(restored.desktops[0].name, "工作桌面 🖥️");
    }
}
