use serde::{Deserialize, Serialize};

use super::shaders::{SHADER_GRAIN, SHADER_MIST_SMOKE};

/// Available background types
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundType {
    /// Subtle film grain on dark background
    #[default]
    Grain,
    /// Animated misty/smoky atmosphere with glass overlay
    Mist,
}

impl BackgroundType {
    /// Get all available background types
    pub fn all() -> &'static [BackgroundType] {
        &[BackgroundType::Grain, BackgroundType::Mist]
    }

    /// Get the display name for this background
    pub fn name(&self) -> &'static str {
        match self {
            BackgroundType::Grain => "Film Grain",
            BackgroundType::Mist => "Misty Smoke",
        }
    }

    /// Get the shader source for this background
    pub(crate) fn shader_source(&self) -> &'static str {
        match self {
            BackgroundType::Grain => SHADER_GRAIN,
            BackgroundType::Mist => SHADER_MIST_SMOKE,
        }
    }

    /// Parse from string ID (e.g., "grain", "mist")
    pub fn from_id(id: &str) -> Option<Self> {
        match id.to_lowercase().as_str() {
            "grain" => Some(BackgroundType::Grain),
            "mist" => Some(BackgroundType::Mist),
            _ => None,
        }
    }

    /// Get the string ID for this background
    pub fn id(&self) -> &'static str {
        match self {
            BackgroundType::Grain => "grain",
            BackgroundType::Mist => "mist",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_background_type_default() {
        let bg: BackgroundType = Default::default();
        assert_eq!(bg, BackgroundType::Grain);
    }

    #[test]
    fn test_background_type_all() {
        let all = BackgroundType::all();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&BackgroundType::Grain));
        assert!(all.contains(&BackgroundType::Mist));
    }

    #[test]
    fn test_background_type_name() {
        assert_eq!(BackgroundType::Grain.name(), "Film Grain");
        assert_eq!(BackgroundType::Mist.name(), "Misty Smoke");
    }

    #[test]
    fn test_background_type_id() {
        assert_eq!(BackgroundType::Grain.id(), "grain");
        assert_eq!(BackgroundType::Mist.id(), "mist");
    }

    #[test]
    fn test_background_type_from_id() {
        assert_eq!(
            BackgroundType::from_id("grain"),
            Some(BackgroundType::Grain)
        );
        assert_eq!(BackgroundType::from_id("mist"), Some(BackgroundType::Mist));
        assert_eq!(BackgroundType::from_id("invalid"), None);
    }

    #[test]
    fn test_background_type_from_id_case_insensitive() {
        assert_eq!(
            BackgroundType::from_id("GRAIN"),
            Some(BackgroundType::Grain)
        );
        assert_eq!(
            BackgroundType::from_id("Grain"),
            Some(BackgroundType::Grain)
        );
        assert_eq!(BackgroundType::from_id("MIST"), Some(BackgroundType::Mist));
        assert_eq!(BackgroundType::from_id("Mist"), Some(BackgroundType::Mist));
    }

    #[test]
    fn test_background_type_roundtrip() {
        for bg in BackgroundType::all() {
            let id = bg.id();
            let parsed = BackgroundType::from_id(id);
            assert_eq!(parsed, Some(*bg));
        }
    }

    #[test]
    fn test_background_type_shader_source_not_empty() {
        for bg in BackgroundType::all() {
            let source = bg.shader_source();
            assert!(
                !source.is_empty(),
                "Shader source for {:?} should not be empty",
                bg
            );
        }
    }

    #[test]
    fn test_background_type_serialize_deserialize() {
        let grain = BackgroundType::Grain;
        let serialized = serde_json::to_string(&grain).unwrap();
        assert_eq!(serialized, "\"grain\"");

        let deserialized: BackgroundType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, grain);

        let mist = BackgroundType::Mist;
        let serialized = serde_json::to_string(&mist).unwrap();
        assert_eq!(serialized, "\"mist\"");

        let deserialized: BackgroundType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, mist);
    }

    #[test]
    fn test_background_type_equality() {
        assert_eq!(BackgroundType::Grain, BackgroundType::Grain);
        assert_eq!(BackgroundType::Mist, BackgroundType::Mist);
        assert_ne!(BackgroundType::Grain, BackgroundType::Mist);
    }

    #[test]
    fn test_background_type_clone() {
        let bg = BackgroundType::Grain;
        let cloned = bg;
        assert_eq!(bg, cloned);
    }

    #[test]
    fn test_background_type_copy() {
        let bg = BackgroundType::Mist;
        let copied: BackgroundType = bg; // Copy trait
        assert_eq!(bg, copied);
    }

    #[test]
    fn test_background_type_debug() {
        let debug_grain = format!("{:?}", BackgroundType::Grain);
        assert!(debug_grain.contains("Grain"));

        let debug_mist = format!("{:?}", BackgroundType::Mist);
        assert!(debug_mist.contains("Mist"));
    }

    #[test]
    fn test_background_type_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(BackgroundType::Grain);
        set.insert(BackgroundType::Mist);
        set.insert(BackgroundType::Grain); // Duplicate

        assert_eq!(set.len(), 2);
    }
}
