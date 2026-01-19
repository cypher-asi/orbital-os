# Orbital OS Build System
# Works on Windows (with make), macOS, and Linux

.PHONY: all build build-processes dev server clean check test help

# Default target
all: build

# Build everything
build: build-processes
	@echo "Building supervisor WASM module..."
	cd crates/orbital-web && wasm-pack build --target web --out-dir ../../web/pkg
	@echo "Build complete!"

# Build test process WASM binaries
build-processes:
	@echo "Building process WASM binaries..."
	cargo build -p orbital-init --target wasm32-unknown-unknown --release
	cargo build -p orbital-terminal --target wasm32-unknown-unknown --release
	cargo build -p orbital-test-procs --target wasm32-unknown-unknown --release
	@echo "Copying WASM binaries to web/processes..."
	mkdir -p web/processes
	cp target/wasm32-unknown-unknown/release/orbital_init.wasm web/processes/init.wasm
	cp target/wasm32-unknown-unknown/release/orbital_terminal.wasm web/processes/terminal.wasm
	cp target/wasm32-unknown-unknown/release/idle.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/memhog.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/sender.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/receiver.wasm web/processes/
	cp target/wasm32-unknown-unknown/release/pingpong.wasm web/processes/
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
	@echo "Orbital OS Build System"
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
