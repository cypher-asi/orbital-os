//! Keystore Service entry point
//!
//! Thin wrapper that invokes the Keystore Service from the library.

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use zos_services::services::KeystoreService;
use zos_apps::app_main;

app_main!(KeystoreService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("KeystoreService is meant to run as WASM in Zero OS");
}
