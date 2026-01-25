//! Cryptographic helpers for identity service
//!
//! This module re-exports canonical crypto from zos-identity::crypto
//! and provides Zero OS-specific utility functions.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use zos_identity::KeyError;

// Re-export canonical crypto from zos-identity
pub use zos_identity::crypto::{
    canonicalize_challenge,
    canonicalize_enrollment_message,
    canonicalize_identity_creation_message,
    derive_identity_signing_keypair,
    derive_machine_keypair_with_scheme,
    sign_message,
    verify_signature,
    Ed25519KeyPair as IdentityKeypair,
    KeyScheme,
    MachineKeyPair,
    NeuralKey,
};


/// Convert bytes to hex string.
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut hex = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        hex.push(HEX_CHARS[(byte >> 4) as usize] as char);
        hex.push(HEX_CHARS[(byte & 0x0F) as usize] as char);
    }
    hex
}

/// Format a u128 as a UUID string (8-4-4-4-12 format with dashes).
pub fn format_uuid(value: u128) -> String {
    let hex = format!("{:032x}", value);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Convert hex string to bytes.
pub fn hex_to_bytes(hex: &str) -> Result<Vec<u8>, &'static str> {
    if hex.len() % 2 != 0 {
        return Err("Hex string must have even length");
    }
    
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    let hex_bytes = hex.as_bytes();
    
    for i in (0..hex.len()).step_by(2) {
        let high = hex_digit_to_value(hex_bytes[i])?;
        let low = hex_digit_to_value(hex_bytes[i + 1])?;
        bytes.push((high << 4) | low);
    }
    
    Ok(bytes)
}

/// Convert a hex digit character to its numeric value.
fn hex_digit_to_value(digit: u8) -> Result<u8, &'static str> {
    match digit {
        b'0'..=b'9' => Ok(digit - b'0'),
        b'a'..=b'f' => Ok(digit - b'a' + 10),
        b'A'..=b'F' => Ok(digit - b'A' + 10),
        _ => Err("Invalid hex character"),
    }
}

/// Decode base64 string to bytes (standard alphabet, with optional padding).
pub fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    const DECODE_TABLE: [i8; 128] = [
        -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, // 0-15
        -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, // 16-31
        -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 62, -1, -1, -1, 63, // 32-47 (+, /)
        52, 53, 54, 55, 56, 57, 58, 59, 60, 61, -1, -1, -1, -1, -1, -1, // 48-63 (0-9)
        -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, // 64-79 (A-O)
        15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, -1, -1, -1, -1, -1, // 80-95 (P-Z)
        -1, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, // 96-111 (a-o)
        41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, -1, -1, -1, -1, -1, // 112-127 (p-z)
    ];

    // Remove padding and whitespace
    let input = input.trim_end_matches('=');
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let mut output = Vec::with_capacity((input.len() * 3) / 4);
    let mut buffer: u32 = 0;
    let mut bits_collected: u8 = 0;

    for c in input.chars() {
        let c = c as usize;
        if c >= 128 {
            return Err("Invalid base64 character");
        }
        let value = DECODE_TABLE[c];
        if value < 0 {
            return Err("Invalid base64 character");
        }
        buffer = (buffer << 6) | (value as u32);
        bits_collected += 6;
        if bits_collected >= 8 {
            bits_collected -= 8;
            output.push((buffer >> bits_collected) as u8);
            buffer &= (1 << bits_collected) - 1;
        }
    }

    Ok(output)
}


/// Create a NeuralKey from raw 32-byte seed material.
pub fn neural_key_from_bytes(seed: &[u8; 32]) -> NeuralKey {
    NeuralKey::from_bytes(*seed)
}

/// Convert u128 to UUID bytes (16 bytes in big-endian format).
/// Used by zid-crypto which expects uuid::Uuid type, but we construct it from bytes.
pub fn u128_to_uuid_bytes(value: u128) -> [u8; 16] {
    value.to_be_bytes()
}

/// Reconstruct a MachineKeyPair from stored seed material for signing.
/// 
/// This allows machine keys to operate independently without needing the Neural Key.
/// The seeds are securely stored and used to reconstruct the keypair when needed.
pub fn machine_keypair_from_seeds(
    signing_sk: &[u8; 32],
    encryption_sk: &[u8; 32],
) -> Result<MachineKeyPair, KeyError> {
    use zos_identity::crypto::ZidMachineKeyCapabilities;
    
    // Create full capabilities (SIGN | ENCRYPT | VAULT_OPERATIONS)
    let capabilities = ZidMachineKeyCapabilities::all();
    
    MachineKeyPair::from_seeds(signing_sk, encryption_sk, capabilities)
        .map_err(|e| KeyError::CryptoError(format!("Failed to reconstruct keypair: {:?}", e)))
}

/// Sign a message with a machine keypair (for challenge-response).
pub fn sign_with_machine_keypair(
    message: &[u8],
    machine_keypair: &MachineKeyPair,
) -> [u8; 64] {
    // Sign using the machine keypair's signing component
    sign_message(&machine_keypair.signing_key_pair(), message)
}
