# Zero OS Build System
# Works on Windows (with make), macOS, and Linux

.PHONY: all build build-processes build-kernel clean check test help qemu qemu-debug

# Default target
all: build

# Build everything (web platform)
build: build-processes
	@echo "Building supervisor WASM module..."
	cd crates/zos-supervisor && wasm-pack build --target web --out-dir ../../web/pkg/supervisor
	@echo "Building desktop WASM module..."
	cd crates/zos-desktop && wasm-pack build --target web --features wasm
	mkdir -p web/pkg/desktop
	cp -r crates/zos-desktop/pkg/* web/pkg/desktop/
	@echo "Build complete!"

# Build test process WASM binaries
# Requires nightly Rust with rust-src component for atomics/shared memory support
# Memory config and linker flags are in .cargo/config.toml
build-processes:
	@echo "Building process WASM binaries with shared memory support (nightly required)..."
	cargo +nightly build -p zos-init --target wasm32-unknown-unknown --release -Z build-std=std,panic_abort
	cargo +nightly build -p zos-system-procs --target wasm32-unknown-unknown --release -Z build-std=std,panic_abort
	cargo +nightly build -p zos-apps --bins --target wasm32-unknown-unknown --release -Z build-std=std,panic_abort
	@echo "Copying WASM binaries to web/processes..."
	mkdir -p web/processes
	cp target/wasm32-unknown-unknown/release/zos_init.wasm web/processes/init.wasm
	cp target/wasm32-unknown-unknown/release/terminal.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/permission_service.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/idle.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/memhog.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/sender.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/receiver.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/pingpong.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/clock.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/calculator.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/settings.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/identity_service.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/vfs_service.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/time_service.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/keystore_service.wasm web/processes/
	@echo "Process binaries ready!"

# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	rm -rf web/pkg
	rm -rf web/processes
	@echo "Clean complete!"

# Run cargo check
check:
	cargo check --workspace

# Run tests
test:
	cargo test --workspace

# ============================================================================
# QEMU / x86_64 Bare Metal Targets (Phase 2)
# ============================================================================

# Build the kernel for x86_64
build-kernel:
	@echo "Building Zero OS kernel for x86_64..."
	cargo +nightly build -p zos-boot --target x86_64-unknown-none --release -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem
	@echo "Kernel built: target/x86_64-unknown-none/release/zero-kernel"

# Build the bootimage tool
build-bootimage:
	@echo "Building bootimage tool..."
	cargo build --release --manifest-path tools/bootimage/Cargo.toml

# Create bootable disk images
bootimage: build-kernel build-bootimage
	@echo "Creating bootable disk images..."
	./tools/bootimage/target/release/bootimage target/x86_64-unknown-none/release/zero-kernel target/x86_64-unknown-none/release/

# Create a VirtIO block device disk image for persistent storage
create-disk:
	@echo "Creating VirtIO disk image (64MB)..."
	@mkdir -p target/x86_64-unknown-none/release
	@if [ ! -f target/x86_64-unknown-none/release/zero-os-data.img ]; then \
		dd if=/dev/zero of=target/x86_64-unknown-none/release/zero-os-data.img bs=1M count=64 2>/dev/null; \
		echo "Created new disk image."; \
	else \
		echo "Disk image already exists (keeping existing data)."; \
	fi

# Run the kernel in QEMU (BIOS mode)
qemu: bootimage create-disk
	@echo "Starting QEMU (BIOS mode) with VirtIO block device..."
	qemu-system-x86_64 \
		-drive format=raw,file=target/x86_64-unknown-none/release/zero-os-bios.img \
		-drive file=target/x86_64-unknown-none/release/zero-os-data.img,if=virtio,format=raw \
		-serial stdio \
		-display none \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-no-reboot

# Run QEMU with GDB server for debugging
qemu-debug: bootimage create-disk
	@echo "Starting QEMU with GDB server on port 1234..."
	@echo "Connect with: gdb target/x86_64-unknown-none/release/zero-kernel"
	@echo "Then: target remote :1234"
	qemu-system-x86_64 \
		-drive format=raw,file=target/x86_64-unknown-none/release/zero-os-bios.img \
		-drive file=target/x86_64-unknown-none/release/zero-os-data.img,if=virtio,format=raw \
		-serial stdio \
		-display none \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-no-reboot \
		-s -S

# Run QEMU with VGA display (for testing graphics later)
qemu-vga: bootimage create-disk
	@echo "Starting QEMU with VGA display and VirtIO block..."
	qemu-system-x86_64 \
		-drive format=raw,file=target/x86_64-unknown-none/release/zero-os-bios.img \
		-drive file=target/x86_64-unknown-none/release/zero-os-data.img,if=virtio,format=raw \
		-serial stdio \
		-device isa-debug-exit,iobase=0xf4,iosize=0x04 \
		-no-reboot

# Run QEMU in UEFI mode (requires OVMF firmware)
qemu-uefi: bootimage create-disk
	@echo "Starting QEMU (UEFI mode) with VirtIO block..."
	qemu-system-x86_64 \
		-bios /usr/share/OVMF/OVMF_CODE.fd \
		-drive format=raw,file=target/x86_64-unknown-none/release/zero-os-uefi.img \
		-drive file=target/x86_64-unknown-none/release/zero-os-data.img,if=virtio,format=raw \
		-serial stdio \
		-display none \
		-no-reboot \
		-no-shutdown

# Reset the data disk (for testing fresh state)
reset-disk:
	@echo "Resetting VirtIO disk image..."
	rm -f target/x86_64-unknown-none/release/zero-os-data.img
	$(MAKE) create-disk

# Show help
help:
	@echo "Zero OS Build System"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Web Platform (Phase 1):"
	@echo "  build           - Build everything (supervisor + test processes)"
	@echo "  build-processes - Build only test process WASM binaries"
	@echo ""
	@echo "QEMU / x86_64 (Phase 2):"
	@echo "  build-kernel    - Build the kernel for x86_64"
	@echo "  bootimage       - Create bootable BIOS/UEFI disk images"
	@echo "  create-disk     - Create VirtIO disk image for storage"
	@echo "  reset-disk      - Reset VirtIO disk (clear all data)"
	@echo "  qemu            - Build and run kernel in QEMU (BIOS mode)"
	@echo "  qemu-uefi       - Run QEMU in UEFI mode (requires OVMF)"
	@echo "  qemu-debug      - Run QEMU with GDB server (port 1234)"
	@echo "  qemu-vga        - Run QEMU with VGA display"
	@echo ""
	@echo "General:"
	@echo "  clean           - Clean build artifacts"
	@echo "  check           - Run cargo check"
	@echo "  test            - Run tests"
	@echo "  help            - Show this help message"
	@echo ""
	@echo "To start the dev server, run: cd web && npm run dev"
