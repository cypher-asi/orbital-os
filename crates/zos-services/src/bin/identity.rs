//! Identity Service entry point
//!
//! Thin wrapper that invokes the Identity Service from the library.

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use zos_services::services::IdentityService;
use zos_apps::app_main;

app_main!(IdentityService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("IdentityService is meant to run as WASM in Zero OS");
}
