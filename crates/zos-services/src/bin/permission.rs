//! Permission Service entry point
//!
//! Thin wrapper that invokes the Permission Service from the library.

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use zos_services::services::PermissionService;
use zos_apps::app_main;

app_main!(PermissionService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("PermissionService is meant to run as WASM in Zero OS");
}
