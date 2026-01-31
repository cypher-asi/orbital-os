//! Time Service entry point
//!
//! Thin wrapper that invokes the Time Service from the library.

#![cfg_attr(target_arch = "wasm32", no_main)]

extern crate alloc;

use zos_services::services::TimeService;
use zos_apps::app_main;

app_main!(TimeService);

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    println!("TimeService is meant to run as WASM in Zero OS");
}
