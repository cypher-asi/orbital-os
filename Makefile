# Zero OS Build System
# Works on Windows (with make), macOS, and Linux

.PHONY: all build build-processes dev server clean check test help

# Default target
all: build

# Build everything
build: build-processes
	@echo "Building supervisor WASM module..."
	cd crates/zos-supervisor-web && wasm-pack build --target web --out-dir ../../web/pkg
	@echo "Building desktop WASM module..."
	cd crates/zos-desktop && wasm-pack build --target web --features wasm
	mkdir -p web/pkg-desktop
	cp -r crates/zos-desktop/pkg/* web/pkg-desktop/
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
	cp target/wasm32-unknown-unknown/release/permission_manager.wasm web/processes/
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
	@echo "Process binaries ready!"

# Build and run the dev server
dev: build server

# Run the dev server (without rebuilding)
server:
	@echo "Starting development server..."
	cargo run -p dev-server

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

# Show help
help:
	@echo "Zero OS Build System"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Targets:"
	@echo "  build           - Build everything (supervisor + test processes)"
	@echo "  build-processes - Build only test process WASM binaries"
	@echo "  dev             - Build and start the dev server"
	@echo "  server          - Start the dev server (without rebuilding)"
	@echo "  clean           - Clean build artifacts"
	@echo "  check           - Run cargo check"
	@echo "  test            - Run tests"
	@echo "  help            - Show this help message"
