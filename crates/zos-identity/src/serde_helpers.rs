//! Shared serde helpers for identity layer serialization.
//!
//! This module provides common serde helper modules used across the identity crate
//! for consistent serialization/deserialization of types like u128 (as hex strings)
//! and byte vectors (as hex strings).
//!
//! # Why Hex Strings?
//!
//! JavaScript/JSON can only represent numbers up to 2^53 - 1 (Number.MAX_SAFE_INTEGER).
//! u128 values like user IDs and machine IDs exceed this limit, so we serialize them
//! as hex strings (e.g., "0x00000000000000000000000000000001") for JavaScript interop.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;
use serde::{self, de, Deserializer, Serializer};

// ============================================================================
// u128 as hex string
// ============================================================================

/// Serde module for serializing/deserializing u128 as hex string (e.g., "0x123abc").
///
/// Also accepts numbers for backward compatibility with existing stored data.
///
/// # Usage
///
/// ```ignore
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct MyStruct {
///     #[serde(with = "zos_identity::serde_helpers::u128_hex_string")]
///     user_id: u128,
/// }
/// ```
pub mod u128_hex_string {
    use super::*;

    pub fn serialize<S>(value: &u128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("0x{:032x}", value))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u128, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct U128Visitor;

        impl<'de> de::Visitor<'de> for U128Visitor {
            type Value = u128;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string like '0x...' or a number")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let s = s.trim_start_matches("0x").trim_start_matches("0X");
                u128::from_str_radix(s, 16).map_err(de::Error::custom)
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(v as u128)
            }

            fn visit_u128<E>(self, v: u128) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(v)
            }
        }

        deserializer.deserialize_any(U128Visitor)
    }
}

// ============================================================================
// Option<Vec<u8>> as hex string
// ============================================================================

/// Serde module for serializing/deserializing `Option<Vec<u8>>` as hex string.
///
/// This is used for large byte vectors (like PQ keys) to avoid massive JSON arrays.
/// Supports both hex string format and legacy array format for backward compatibility.
///
/// # Format
///
/// - `None` serializes as JSON `null`
/// - `Some(bytes)` serializes as hex string, e.g., `"0a1b2c3d..."`
///
/// # Usage
///
/// ```ignore
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct MyStruct {
///     #[serde(default, skip_serializing_if = "Option::is_none", with = "zos_identity::serde_helpers::option_bytes_hex")]
///     pq_key: Option<Vec<u8>>,
/// }
/// ```
pub mod option_bytes_hex {
    use super::*;

    pub fn serialize<S>(value: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(bytes) => {
                let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
                serializer.serialize_some(&hex)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OptionBytesVisitor;

        impl<'de> de::Visitor<'de> for OptionBytesVisitor {
            type Value = Option<Vec<u8>>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string, byte array, or null")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(None)
            }

            fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de>,
            {
                deserializer.deserialize_any(BytesVisitor).map(Some)
            }

            fn visit_unit<E>(self) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(None)
            }
        }

        struct BytesVisitor;

        impl<'de> de::Visitor<'de> for BytesVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string or byte array")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // Parse hex string
                let s = s.trim_start_matches("0x").trim_start_matches("0X");
                let mut bytes = Vec::with_capacity(s.len() / 2);
                let mut chars = s.chars();
                while let (Some(h), Some(l)) = (chars.next(), chars.next()) {
                    let byte = u8::from_str_radix(&format!("{}{}", h, l), 16)
                        .map_err(de::Error::custom)?;
                    bytes.push(byte);
                }
                Ok(bytes)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                // Parse array of numbers (backward compatibility)
                let mut bytes = Vec::new();
                while let Some(b) = seq.next_element::<u8>()? {
                    bytes.push(b);
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_option(OptionBytesVisitor)
    }
}

// ============================================================================
// Vec<u8> as hex string
// ============================================================================

/// Serde module for serializing/deserializing `Vec<u8>` as hex string.
///
/// # Usage
///
/// ```ignore
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct MyStruct {
///     #[serde(with = "zos_identity::serde_helpers::bytes_hex")]
///     ciphertext: Vec<u8>,
/// }
/// ```
pub mod bytes_hex {
    use super::*;

    pub fn serialize<S>(value: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex: String = value.iter().map(|b| format!("{:02x}", b)).collect();
        serializer.serialize_str(&hex)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BytesVisitor;

        impl<'de> de::Visitor<'de> for BytesVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string or byte array")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let s = s.trim_start_matches("0x").trim_start_matches("0X");
                let mut bytes = Vec::with_capacity(s.len() / 2);
                let mut chars = s.chars();
                while let (Some(h), Some(l)) = (chars.next(), chars.next()) {
                    let byte = u8::from_str_radix(&format!("{}{}", h, l), 16)
                        .map_err(de::Error::custom)?;
                    bytes.push(byte);
                }
                Ok(bytes)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut bytes = Vec::new();
                while let Some(b) = seq.next_element::<u8>()? {
                    bytes.push(b);
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_any(BytesVisitor)
    }
}

// ============================================================================
// [u8; 12] as hex string (for AES-GCM nonces)
// ============================================================================

/// Serde module for serializing/deserializing `[u8; 12]` as hex string.
pub mod array_hex_12 {
    use super::*;

    pub fn serialize<S>(value: &[u8; 12], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex: String = value.iter().map(|b| format!("{:02x}", b)).collect();
        serializer.serialize_str(&hex)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 12], D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ArrayVisitor;

        impl<'de> de::Visitor<'de> for ArrayVisitor {
            type Value = [u8; 12];

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string (24 chars) or array of 12 bytes")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let s = s.trim_start_matches("0x").trim_start_matches("0X");
                if s.len() != 24 {
                    return Err(de::Error::custom(format!(
                        "expected 24 hex chars for 12 bytes, got {}",
                        s.len()
                    )));
                }
                let mut bytes = [0u8; 12];
                let mut chars = s.chars();
                for b in bytes.iter_mut() {
                    let h = chars.next().ok_or_else(|| de::Error::custom("unexpected end"))?;
                    let l = chars.next().ok_or_else(|| de::Error::custom("unexpected end"))?;
                    *b = u8::from_str_radix(&format!("{}{}", h, l), 16)
                        .map_err(de::Error::custom)?;
                }
                Ok(bytes)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut bytes = [0u8; 12];
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_any(ArrayVisitor)
    }
}

// ============================================================================
// [u8; 16] as hex string (for AES-GCM auth tags)
// ============================================================================

/// Serde module for serializing/deserializing `[u8; 16]` as hex string.
pub mod array_hex_16 {
    use super::*;

    pub fn serialize<S>(value: &[u8; 16], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex: String = value.iter().map(|b| format!("{:02x}", b)).collect();
        serializer.serialize_str(&hex)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 16], D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ArrayVisitor;

        impl<'de> de::Visitor<'de> for ArrayVisitor {
            type Value = [u8; 16];

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string (32 chars) or array of 16 bytes")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let s = s.trim_start_matches("0x").trim_start_matches("0X");
                if s.len() != 32 {
                    return Err(de::Error::custom(format!(
                        "expected 32 hex chars for 16 bytes, got {}",
                        s.len()
                    )));
                }
                let mut bytes = [0u8; 16];
                let mut chars = s.chars();
                for b in bytes.iter_mut() {
                    let h = chars.next().ok_or_else(|| de::Error::custom("unexpected end"))?;
                    let l = chars.next().ok_or_else(|| de::Error::custom("unexpected end"))?;
                    *b = u8::from_str_radix(&format!("{}{}", h, l), 16)
                        .map_err(de::Error::custom)?;
                }
                Ok(bytes)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: de::SeqAccess<'de>,
            {
                let mut bytes = [0u8; 16];
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b = seq
                        .next_element()?
                        .ok_or_else(|| de::Error::invalid_length(i, &self))?;
                }
                Ok(bytes)
            }
        }

        deserializer.deserialize_any(ArrayVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestU128 {
        #[serde(with = "u128_hex_string")]
        value: u128,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestBytes {
        #[serde(default, skip_serializing_if = "Option::is_none", with = "option_bytes_hex")]
        data: Option<Vec<u8>>,
    }

    #[test]
    fn test_u128_roundtrip() {
        let original = TestU128 { value: 0x123456789abcdef0u128 };
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("0x"));
        
        let decoded: TestU128 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_u128_from_number() {
        // Should accept plain numbers for backward compatibility
        let json = r#"{"value": 42}"#;
        let decoded: TestU128 = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.value, 42);
    }

    #[test]
    fn test_bytes_hex_roundtrip() {
        let original = TestBytes { data: Some(vec![0x01, 0x02, 0x0a, 0xff]) };
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("01020aff"));
        
        let decoded: TestBytes = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_bytes_none() {
        let original = TestBytes { data: None };
        let json = serde_json::to_string(&original).unwrap();
        // None should not appear in JSON due to skip_serializing_if
        assert!(!json.contains("data"));
        
        let decoded: TestBytes = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.data, None);
    }

    #[test]
    fn test_bytes_from_array() {
        // Should accept array format for backward compatibility
        let json = r#"{"data": [1, 2, 10, 255]}"#;
        let decoded: TestBytes = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.data, Some(vec![0x01, 0x02, 0x0a, 0xff]));
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestBytesRequired {
        #[serde(with = "bytes_hex")]
        data: Vec<u8>,
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestArray12 {
        #[serde(with = "array_hex_12")]
        nonce: [u8; 12],
    }

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestArray16 {
        #[serde(with = "array_hex_16")]
        tag: [u8; 16],
    }

    #[test]
    fn test_bytes_required_roundtrip() {
        let original = TestBytesRequired { data: vec![0x01, 0x02, 0xab, 0xcd] };
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("0102abcd"));
        
        let decoded: TestBytesRequired = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_array_12_roundtrip() {
        let original = TestArray12 { nonce: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11] };
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("000102030405060708090a0b"));
        
        let decoded: TestArray12 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_array_16_roundtrip() {
        let original = TestArray16 { tag: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15] };
        let json = serde_json::to_string(&original).unwrap();
        assert!(json.contains("000102030405060708090a0b0c0d0e0f"));
        
        let decoded: TestArray16 = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }
}
