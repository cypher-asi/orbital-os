//! Network Service entry point
//!
//! Thin wrapper that invokes the Network Service from the library.

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use zos_services::services::NetworkService;
use zos_apps::app_main;

app_main!(NetworkService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("NetworkService is meant to run as WASM in Zero OS");
}
