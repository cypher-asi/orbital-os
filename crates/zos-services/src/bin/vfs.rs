//! VFS Service entry point
//!
//! Thin wrapper that invokes the VFS Service from the library.

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use zos_services::services::VfsService;
use zos_apps::app_main;

app_main!(VfsService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("VfsService is meant to run as WASM in Zero OS");
}
