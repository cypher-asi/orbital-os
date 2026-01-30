//! Zero OS Bootimage Builder
//!
//! This tool creates a bootable disk image from the kernel binary
//! using the bootloader crate.

use bootloader::DiskImageBuilder;
use std::path::PathBuf;
use std::{env, process};

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: bootimage <kernel-binary> [output-path]");
        eprintln!();
        eprintln!("Creates a bootable UEFI and BIOS disk image from the kernel binary.");
        process::exit(1);
    }

    let kernel_path = PathBuf::from(&args[1]);
    let output_dir = if args.len() > 2 {
        PathBuf::from(&args[2])
    } else {
        kernel_path.parent().unwrap_or(&PathBuf::from(".")).to_path_buf()
    };

    if !kernel_path.exists() {
        eprintln!("Error: Kernel binary not found: {}", kernel_path.display());
        process::exit(1);
    }

    println!("Creating bootable disk images...");
    println!("  Kernel: {}", kernel_path.display());
    println!("  Output: {}", output_dir.display());

    // Create the disk image builder
    let builder = DiskImageBuilder::new(kernel_path.clone());

    // Create UEFI disk image
    let uefi_path = output_dir.join("zero-os-uefi.img");
    match builder.create_uefi_image(&uefi_path) {
        Ok(_) => println!("  Created UEFI image: {}", uefi_path.display()),
        Err(e) => {
            eprintln!("Error creating UEFI image: {}", e);
            process::exit(1);
        }
    }

    // Create BIOS disk image
    let bios_path = output_dir.join("zero-os-bios.img");
    match builder.create_bios_image(&bios_path) {
        Ok(_) => println!("  Created BIOS image: {}", bios_path.display()),
        Err(e) => {
            eprintln!("Error creating BIOS image: {}", e);
            process::exit(1);
        }
    }

    println!();
    println!("Done! You can now run the images in QEMU:");
    println!();
    println!("  BIOS mode:");
    println!("    qemu-system-x86_64 -drive format=raw,file={} -serial stdio", bios_path.display());
    println!();
    println!("  UEFI mode (requires OVMF):");
    println!("    qemu-system-x86_64 -bios /usr/share/OVMF/OVMF_CODE.fd -drive format=raw,file={} -serial stdio", uefi_path.display());
}
