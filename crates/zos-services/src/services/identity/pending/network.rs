//! Network pending operations

extern crate alloc;

use alloc::string::String;
use zos_identity::keystore::MachineKeyRecord;

use super::RequestContext;

/// Tracks pending network operations awaiting results.
#[derive(Clone)]
pub enum PendingNetworkOp {
    /// Request challenge from ZID server (network request)
    RequestZidChallenge {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Machine key for signing the challenge
        machine_key: Box<MachineKeyRecord>,
    },
    /// Submit signed challenge to ZID server (network request)
    SubmitZidLogin {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
    },
    /// Submit email credential to ZID server (network request)
    SubmitEmailToZid {
        ctx: RequestContext,
        user_id: u128,
        email: String,
    },
    /// Submit enrollment to ZID server (network request)
    SubmitZidEnroll {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Derived identity ID for storage
        identity_id: u128,
        /// Derived machine ID for storage
        machine_id: u128,
        /// Identity signing public key (for storage)
        identity_signing_public_key: [u8; 32],
        /// Machine signing public key (for storage)
        machine_signing_public_key: [u8; 32],
        /// Machine encryption public key (for storage)
        machine_encryption_public_key: [u8; 32],
        /// Machine signing seed (for keypair reconstruction)
        machine_signing_sk: [u8; 32],
        /// Machine encryption seed (for keypair reconstruction)
        machine_encryption_sk: [u8; 32],
    },
    /// Request challenge after identity creation (for chained login)
    /// Identity creation returns {identity_id, machine_id, namespace_id} without tokens.
    /// We need to chain into login flow to get tokens.
    RequestZidChallengeAfterEnroll {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Machine ID for storage (our local u128 ID)
        machine_id: u128,
        /// Identity signing public key (for storage)
        identity_signing_public_key: [u8; 32],
        /// Machine signing public key (for storage)
        machine_signing_public_key: [u8; 32],
        /// Machine encryption public key (for storage)
        machine_encryption_public_key: [u8; 32],
        /// Machine signing seed (for keypair reconstruction and signing challenge)
        machine_signing_sk: [u8; 32],
        /// Machine encryption seed (for keypair reconstruction)
        machine_encryption_sk: [u8; 32],
    },
    /// Submit login after identity creation (final step to get tokens)
    SubmitZidLoginAfterEnroll {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Machine ID for storage
        machine_id: u128,
        /// Identity signing public key (for storage)
        identity_signing_public_key: [u8; 32],
        /// Machine signing public key (for storage)
        machine_signing_public_key: [u8; 32],
        /// Machine encryption public key (for storage)
        machine_encryption_public_key: [u8; 32],
        /// Machine signing seed (for storage)
        machine_signing_sk: [u8; 32],
        /// Machine encryption seed (for storage)
        machine_encryption_sk: [u8; 32],
    },

    // =========================================================================
    // Combined Machine Key + ZID Enrollment operations
    // =========================================================================
    /// Submit enrollment to ZID server for combined flow (machine key already stored)
    SubmitZidEnrollForCombined {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Machine ID already stored locally
        machine_id: u128,
        /// Identity signing public key
        identity_signing_public_key: [u8; 32],
        /// Machine signing public key
        machine_signing_public_key: [u8; 32],
        /// Machine encryption public key
        machine_encryption_public_key: [u8; 32],
        /// Machine signing SK (for signing challenge)
        machine_signing_sk: [u8; 32],
        /// Machine encryption SK
        machine_encryption_sk: [u8; 32],
        /// Stored machine key record (to return on success)
        machine_key_record: Box<MachineKeyRecord>,
    },
    /// Request challenge after combined enrollment identity creation
    RequestZidChallengeForCombined {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        machine_id: u128,
        identity_signing_public_key: [u8; 32],
        machine_signing_public_key: [u8; 32],
        machine_encryption_public_key: [u8; 32],
        machine_signing_sk: [u8; 32],
        machine_encryption_sk: [u8; 32],
        machine_key_record: Box<MachineKeyRecord>,
    },
    /// Submit login after combined enrollment challenge
    SubmitZidLoginForCombined {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        machine_id: u128,
        identity_signing_public_key: [u8; 32],
        machine_signing_public_key: [u8; 32],
        machine_encryption_public_key: [u8; 32],
        machine_signing_sk: [u8; 32],
        machine_encryption_sk: [u8; 32],
        machine_key_record: Box<MachineKeyRecord>,
    },

    // =========================================================================
    // ZID Token Refresh operations
    // =========================================================================
    /// Submit token refresh request to ZID server
    SubmitZidRefresh {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
        /// Session ID from the stored session (refresh response may not include it)
        session_id: String,
        /// Machine ID from the stored session (refresh response may not include it)
        machine_id: String,
        /// Login type from the original session (to preserve through refresh)
        login_type: zos_identity::ipc::LoginType,
    },

    // =========================================================================
    // ZID Email Login operations
    // =========================================================================
    /// Submit email/password login to ZID server
    SubmitZidEmailLogin {
        ctx: RequestContext,
        user_id: u128,
        zid_endpoint: String,
    },
}
