# Zero OS Build Script for Windows
# Usage: .\build.ps1 [target]
#   targets: all, web, processes, kernel, qemu, qemu-debug, clean

param(
    [string]$Target = "all"
)

$ErrorActionPreference = "Stop"
$ProjectRoot = $PSScriptRoot

function Write-Step($message) {
    Write-Host "`n=== $message ===" -ForegroundColor Cyan
}

function Build-WebModules {
    Write-Step "Building supervisor and desktop WASM modules"
    
    $configPath = "$ProjectRoot\.cargo\config.toml"
    $configBackup = "$ProjectRoot\.cargo\config.toml.bak"
    $hasConfig = Test-Path $configPath
    
    try {
        # Temporarily disable threading config (only needed for process binaries)
        if ($hasConfig) {
            Write-Host "Temporarily disabling .cargo/config.toml (threading flags)"
            Move-Item $configPath $configBackup -Force
        }
        
        # Build zos-supervisor
        Write-Host "Building zos-supervisor..."
        Push-Location "$ProjectRoot\crates\zos-supervisor"
        wasm-pack build --target web --out-dir ../../web/pkg/supervisor
        if ($LASTEXITCODE -ne 0) { throw "zos-supervisor build failed" }
        Pop-Location
        
        # Build zos-desktop
        Write-Host "Building zos-desktop..."
        Push-Location "$ProjectRoot\crates\zos-desktop"
        wasm-pack build --target web --features wasm
        if ($LASTEXITCODE -ne 0) { throw "zos-desktop build failed" }
        Pop-Location
        
        # Copy desktop pkg to web folder
        Write-Host "Copying zos-desktop to web/pkg/desktop..."
        if (-not (Test-Path "$ProjectRoot\web\pkg\desktop")) {
            New-Item -ItemType Directory -Path "$ProjectRoot\web\pkg\desktop" -Force | Out-Null
        }
        Copy-Item -Recurse -Force "$ProjectRoot\crates\zos-desktop\pkg\*" "$ProjectRoot\web\pkg\desktop\"
        
        Write-Host "Web modules built successfully!" -ForegroundColor Green
    }
    finally {
        # Always restore the config
        if ($hasConfig -and (Test-Path $configBackup)) {
            Move-Item $configBackup $configPath -Force
            Write-Host "Restored .cargo/config.toml"
        }
    }
}

function Invoke-CargoBuild {
    param(
        [string]$Package,
        [string]$ExtraArgs = ""
    )
    
    # Build with cargo and filter out expected unstable feature warnings
    $cmd = "cargo +nightly build -p $Package --target wasm32-unknown-unknown --release -Z build-std=std,panic_abort $ExtraArgs"
    
    # Run cargo and capture output, filtering unstable feature warnings
    $pinfo = New-Object System.Diagnostics.ProcessStartInfo
    $pinfo.FileName = "cmd.exe"
    $pinfo.Arguments = "/c $cmd 2>&1"
    $pinfo.RedirectStandardOutput = $true
    $pinfo.UseShellExecute = $false
    $pinfo.WorkingDirectory = $ProjectRoot
    
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $pinfo
    $process.Start() | Out-Null
    
    $skipLines = 0
    while (-not $process.StandardOutput.EndOfStream) {
        $line = $process.StandardOutput.ReadLine()
        
        # Skip the unstable feature warning block (warning + empty line + note)
        if ($line -match "warning: unstable feature specified for .+-Ctarget-feature") {
            $skipLines = 3  # Skip this line and next 3 lines (|, = note:, empty)
            continue
        }
        if ($skipLines -gt 0) {
            $skipLines--
            continue
        }
        # Skip duplicate warning notes
        if ($line -match "warning: .+ generated 1 warning \(1 duplicate\)") {
            continue
        }
        
        Write-Host $line
    }
    
    $process.WaitForExit()
    return $process.ExitCode
}

function Build-Processes {
    Write-Step "Building process WASM binaries (with threading support)"
    
    Push-Location $ProjectRoot
    try {
        # Build init
        Write-Host "Building zos-init..."
        $exitCode = Invoke-CargoBuild -Package "zos-init"
        if ($exitCode -ne 0) { throw "zos-init build failed" }
        
        # Build test processes
        Write-Host "Building zos-system-procs..."
        $exitCode = Invoke-CargoBuild -Package "zos-system-procs"
        if ($exitCode -ne 0) { throw "zos-system-procs build failed" }
        
        # Build apps
        Write-Host "Building zos-apps..."
        $exitCode = Invoke-CargoBuild -Package "zos-apps" -ExtraArgs "--bins"
        if ($exitCode -ne 0) { throw "zos-apps build failed" }
        
        # Build services
        Write-Host "Building zos-services..."
        $exitCode = Invoke-CargoBuild -Package "zos-services" -ExtraArgs "--bins"
        if ($exitCode -ne 0) { throw "zos-services build failed" }
        
        # Copy to web/processes
        Write-Host "Copying process binaries to web/processes..."
        if (-not (Test-Path "$ProjectRoot\web\processes")) {
            New-Item -ItemType Directory -Path "$ProjectRoot\web\processes" | Out-Null
        }
        
        $releaseDir = "$ProjectRoot\target\wasm32-unknown-unknown\release"
        Copy-Item "$releaseDir\zos_init.wasm" "$ProjectRoot\web\processes\init.wasm" -Force
        Copy-Item "$releaseDir\terminal.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\permission.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\idle.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\memhog.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\sender.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\receiver.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\pingpong.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\clock.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\calculator.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\settings.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\identity.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\vfs.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\time.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\keystore.wasm" "$ProjectRoot\web\processes\" -Force
        
        Write-Host "Process binaries built successfully!" -ForegroundColor Green
    }
    finally {
        Pop-Location
    }
}

function Build-QemuProcesses {
    Write-Step "Building QEMU process WASM binaries (without shared memory)"
    
    # Use a separate target directory for QEMU builds to avoid conflicts with web builds
    # Web builds use shared memory (atomics), QEMU builds don't (wasmi doesn't support threads)
    $qemuTargetDir = "$ProjectRoot\target-qemu"
    
    $configPath = "$ProjectRoot\.cargo\config.toml"
    $configBackup = "$ProjectRoot\.cargo\config.toml.bak"
    $hasConfig = Test-Path $configPath
    
    Push-Location $ProjectRoot
    try {
        # Temporarily disable threading config (wasmi doesn't support threads)
        if ($hasConfig) {
            Write-Host "Temporarily disabling .cargo/config.toml (threading flags)"
            Move-Item $configPath $configBackup -Force
        }
        
        # Memory settings for QEMU WASM processes:
        # - Init needs 4MB to load large service binaries (identity is 1.1MB)
        # - Services only need 1MB each (they don't load other WASM binaries)
        # Total: 4MB (init) + 6×1MB (services) = 10MB linear memory (fits in 70MB kernel heap)
        
        $qemuConfigPath = "$ProjectRoot\.cargo\qemu-config.toml"
        
        # Init config: 6MB initial, 8MB max (loads large binaries sequentially)
        # Boot loads: perm(282KB) + vfs(462KB) + keystore(369KB) + identity(1.17MB) + time(386KB) + terminal(45KB) = ~2.7MB
        # Plus working memory and string formatting overhead
        $initMemoryFlags = 'target.wasm32-unknown-unknown.rustflags = ["-C", "link-arg=--initial-memory=6291456", "-C", "link-arg=--max-memory=8388608", "-C", "link-arg=-zstack-size=65536"]'
        $initMemoryFlags | Out-File -FilePath $qemuConfigPath -Encoding utf8
        
        # Build Init with larger memory (using separate target dir)
        # --features skip-identity: Skip IdentityService in QEMU (wasm-bindgen shims incomplete)
        Write-Host "Building zos-init (4MB initial memory for loading services)..."
        cargo +nightly build -p zos-init --target wasm32-unknown-unknown --release --config $qemuConfigPath --target-dir $qemuTargetDir --features skip-identity
        if ($LASTEXITCODE -ne 0) { throw "zos-init build failed" }
        
        # Service config: 1MB initial, 2MB max (services don't load binaries)
        $serviceMemoryFlags = 'target.wasm32-unknown-unknown.rustflags = ["-C", "link-arg=--initial-memory=1048576", "-C", "link-arg=--max-memory=2097152", "-C", "link-arg=-zstack-size=32768"]'
        $serviceMemoryFlags | Out-File -FilePath $qemuConfigPath -Encoding utf8
        
        Write-Host "Building zos-system-procs (no shared memory)..."
        cargo +nightly build -p zos-system-procs --target wasm32-unknown-unknown --release --config $qemuConfigPath --target-dir $qemuTargetDir
        if ($LASTEXITCODE -ne 0) { throw "zos-system-procs build failed" }
        
        Write-Host "Building zos-apps (no shared memory)..."
        cargo +nightly build -p zos-apps --bins --target wasm32-unknown-unknown --release --config $qemuConfigPath --target-dir $qemuTargetDir
        if ($LASTEXITCODE -ne 0) { throw "zos-apps build failed" }
        
        Write-Host "Building zos-services (no shared memory)..."
        cargo +nightly build -p zos-services --bins --target wasm32-unknown-unknown --release --config $qemuConfigPath --target-dir $qemuTargetDir
        if ($LASTEXITCODE -ne 0) { throw "zos-services build failed" }
        
        # Copy to qemu/processes from the QEMU-specific target directory
        Write-Host "Copying process binaries to qemu/processes..."
        if (-not (Test-Path "$ProjectRoot\qemu\processes")) {
            New-Item -ItemType Directory -Path "$ProjectRoot\qemu\processes" -Force | Out-Null
        }
        
        $releaseDir = "$qemuTargetDir\wasm32-unknown-unknown\release"
        Copy-Item "$releaseDir\zos_init.wasm" "$ProjectRoot\qemu\processes\init.wasm" -Force
        Copy-Item "$releaseDir\terminal.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\permission.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\idle.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\memhog.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\sender.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\receiver.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\pingpong.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\clock.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\calculator.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\settings.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\identity.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\vfs.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\time.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\keystore.wasm" "$ProjectRoot\qemu\processes\" -Force
        
        Write-Host "QEMU process binaries built successfully!" -ForegroundColor Green
    }
    finally {
        # Clean up QEMU config file
        $qemuConfigPath = "$ProjectRoot\.cargo\qemu-config.toml"
        if (Test-Path $qemuConfigPath) {
            Remove-Item $qemuConfigPath -Force
        }
        
        # Always restore the config
        if ($hasConfig -and (Test-Path $configBackup)) {
            Move-Item $configBackup $configPath -Force
            Write-Host "Restored .cargo/config.toml"
        }
        Pop-Location
    }
}

function Clean-Build {
    Write-Step "Cleaning build artifacts"
    Push-Location $ProjectRoot
    cargo clean
    # Also clean the separate QEMU target directory
    Remove-Item -Recurse -Force "$ProjectRoot\target-qemu" -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$ProjectRoot\web\pkg" -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$ProjectRoot\web\processes" -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$ProjectRoot\qemu\processes" -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force "$ProjectRoot\crates\zos-desktop\pkg" -ErrorAction SilentlyContinue
    Pop-Location
    Write-Host "Clean complete!" -ForegroundColor Green
}

# ============================================================================
# QEMU / x86_64 Bare Metal Targets (Phase 2)
# ============================================================================

function Build-Kernel {
    # First build QEMU-compatible WASM processes (without shared memory)
    Build-QemuProcesses
    
    Write-Step "Building Zero OS kernel for x86_64"
    
    Push-Location $ProjectRoot
    try {
        $cmd = "cargo +nightly build -p zos-boot --target x86_64-unknown-none --release -Z build-std=core,alloc -Z build-std-features=compiler-builtins-mem"
        
        Write-Host "Running: $cmd"
        Invoke-Expression $cmd
        
        if ($LASTEXITCODE -ne 0) { 
            throw "Kernel build failed" 
        }
        
        Write-Host "Kernel built successfully!" -ForegroundColor Green
        Write-Host "Binary: target\x86_64-unknown-none\release\zero-kernel"
    }
    finally {
        Pop-Location
    }
}

function Build-Bootimage {
    Write-Step "Building bootimage tool"
    
    Push-Location $ProjectRoot
    try {
        cargo build --release --manifest-path tools/bootimage/Cargo.toml
        if ($LASTEXITCODE -ne 0) { 
            throw "Bootimage tool build failed" 
        }
    }
    finally {
        Pop-Location
    }
}

function Create-Bootimage {
    Build-Kernel
    Build-Bootimage
    
    Write-Step "Creating bootable disk images"
    
    $kernelPath = "$ProjectRoot\target\x86_64-unknown-none\release\zero-kernel"
    $outputDir = "$ProjectRoot\target\x86_64-unknown-none\release"
    $bootimageExe = "$ProjectRoot\tools\bootimage\target\release\bootimage.exe"
    
    if (-not (Test-Path $kernelPath)) {
        throw "Kernel binary not found: $kernelPath"
    }
    
    & $bootimageExe $kernelPath $outputDir
    
    if ($LASTEXITCODE -ne 0) { 
        throw "Bootimage creation failed" 
    }
    
    Write-Host "Disk images created successfully!" -ForegroundColor Green
}

function Create-DataDisk {
    Write-Step "Creating VirtIO data disk (64MB)"
    
    $diskPath = "$ProjectRoot\target\x86_64-unknown-none\release\zero-os-data.img"
    $diskDir = Split-Path $diskPath -Parent
    
    # Ensure directory exists
    if (-not (Test-Path $diskDir)) {
        New-Item -ItemType Directory -Path $diskDir -Force | Out-Null
    }
    
    # Create disk only if it doesn't exist (to preserve data across runs)
    if (-not (Test-Path $diskPath)) {
        $sizeInMB = 64
        $sizeInBytes = $sizeInMB * 1024 * 1024
        
        # Create a sparse file filled with zeros
        $fileStream = [System.IO.File]::Create($diskPath)
        $fileStream.SetLength($sizeInBytes)
        $fileStream.Close()
        
        Write-Host "Created new data disk: $diskPath" -ForegroundColor Green
    } else {
        Write-Host "Using existing data disk (preserving data): $diskPath" -ForegroundColor Cyan
    }
}

function Reset-DataDisk {
    Write-Step "Resetting VirtIO data disk"
    
    $diskPath = "$ProjectRoot\target\x86_64-unknown-none\release\zero-os-data.img"
    
    if (Test-Path $diskPath) {
        Remove-Item $diskPath -Force
        Write-Host "Removed existing data disk" -ForegroundColor Yellow
    }
    
    Create-DataDisk
}

function Find-Qemu {
    # Try to find qemu-system-x86_64 executable
    $qemuPaths = @(
        "C:\Program Files\qemu\qemu-system-x86_64.exe",
        "C:\Program Files (x86)\qemu\qemu-system-x86_64.exe",
        "$env:USERPROFILE\scoop\apps\qemu\current\qemu-system-x86_64.exe",
        "$env:LOCALAPPDATA\Programs\qemu\qemu-system-x86_64.exe"
    )
    
    # First check if it's in PATH
    $inPath = Get-Command qemu-system-x86_64 -ErrorAction SilentlyContinue
    if ($inPath) {
        return $inPath.Source
    }
    
    # Otherwise search common locations
    foreach ($path in $qemuPaths) {
        if (Test-Path $path) {
            return $path
        }
    }
    
    return $null
}

function Run-Qemu {
    param(
        [switch]$Debug,
        [switch]$Vga,
        [switch]$Uefi
    )
    
    Create-Bootimage
    Create-DataDisk
    
    Write-Step "Starting QEMU with VirtIO block device"
    
    # Find QEMU executable
    $qemuExe = Find-Qemu
    if (-not $qemuExe) {
        throw "QEMU not found. Please install QEMU and ensure it's in your PATH or installed in a standard location.`nInstall with: winget install SoftwareFreedomConservancy.QEMU"
    }
    Write-Host "Using QEMU: $qemuExe" -ForegroundColor Gray
    
    $biosImage = "$ProjectRoot\target\x86_64-unknown-none\release\zero-os-bios.img"
    $uefiImage = "$ProjectRoot\target\x86_64-unknown-none\release\zero-os-uefi.img"
    $dataImage = "$ProjectRoot\target\x86_64-unknown-none\release\zero-os-data.img"
    
    if ($Uefi) {
        $imagePath = $uefiImage
        Write-Host "Using UEFI image (requires OVMF firmware)" -ForegroundColor Yellow
    } else {
        $imagePath = $biosImage
    }
    
    if (-not (Test-Path $imagePath)) {
        throw "Disk image not found: $imagePath"
    }
    
    $qemuArgs = @(
        "-drive", "format=raw,file=$imagePath",
        "-drive", "file=$dataImage,if=virtio,format=raw",
        "-nographic",  # Combines -serial stdio with proper stdin routing on Windows
        "-device", "isa-debug-exit,iobase=0xf4,iosize=0x04",
        "-no-reboot",
        "-no-shutdown"
    )
    
    if ($Uefi) {
        # Try common OVMF locations on Windows
        $qemuDir = Split-Path $qemuExe -Parent
        $ovmfPaths = @(
            "$qemuDir\share\edk2-x86_64-code.fd",
            "C:\Program Files\qemu\share\edk2-x86_64-code.fd",
            "C:\Program Files (x86)\qemu\share\edk2-x86_64-code.fd",
            "$env:USERPROFILE\scoop\apps\qemu\current\share\edk2-x86_64-code.fd"
        )
        $ovmfPath = $null
        foreach ($path in $ovmfPaths) {
            if (Test-Path $path) {
                $ovmfPath = $path
                break
            }
        }
        if ($ovmfPath) {
            $qemuArgs = @("-bios", $ovmfPath) + $qemuArgs
        } else {
            Write-Host "Warning: OVMF firmware not found. UEFI boot may not work." -ForegroundColor Yellow
        }
    }
    
    if (-not $Vga) {
        $qemuArgs += @("-display", "none")
    }
    
    if ($Debug) {
        $qemuArgs += @("-s", "-S")
        Write-Host "GDB server listening on port 1234" -ForegroundColor Yellow
        Write-Host "Connect with: gdb target\x86_64-unknown-none\release\zero-kernel" -ForegroundColor Yellow
        Write-Host "Then: target remote :1234" -ForegroundColor Yellow
    }
    
    Write-Host "Running: $qemuExe $($qemuArgs -join ' ')"
    & $qemuExe @qemuArgs
}

# Main
switch ($Target.ToLower()) {
    "all" {
        Build-Processes
        Build-WebModules
        Write-Host "`nBuild complete! Run 'cd web && npm run dev' to start the development server." -ForegroundColor Green
    }
    "web" {
        Build-Processes
        Build-WebModules
    }
    "processes" {
        Build-Processes
    }
    "qemu-processes" {
        Build-QemuProcesses
    }
    "kernel" {
        Build-Kernel
    }
    "bootimage" {
        Create-Bootimage
    }
    "create-disk" {
        Create-DataDisk
    }
    "reset-disk" {
        Reset-DataDisk
    }
    "qemu" {
        Run-Qemu
    }
    "qemu-uefi" {
        Run-Qemu -Uefi
    }
    "qemu-debug" {
        Run-Qemu -Debug
    }
    "qemu-vga" {
        Run-Qemu -Vga
    }
    "clean" {
        Clean-Build
    }
    default {
        Write-Host "Zero OS Build Script"
        Write-Host ""
        Write-Host "Usage: .\build.ps1 [target]"
        Write-Host ""
        Write-Host "Web Platform (Phase 1):"
        Write-Host "  all          - Build everything (default)"
        Write-Host "  web          - Build only supervisor/desktop WASM modules"
        Write-Host "  processes    - Build only process WASM binaries (with shared memory)"
        Write-Host ""
        Write-Host "QEMU / x86_64 (Phase 2):"
        Write-Host "  qemu-processes - Build WASM processes for QEMU (no shared memory)"
        Write-Host "  kernel       - Build the kernel for x86_64 (includes qemu-processes)"
        Write-Host "  bootimage    - Create bootable BIOS/UEFI disk images"
        Write-Host "  create-disk  - Create VirtIO data disk for storage"
        Write-Host "  reset-disk   - Reset VirtIO data disk (clear all data)"
        Write-Host "  qemu         - Build and run kernel in QEMU (BIOS mode)"
        Write-Host "  qemu-uefi    - Run QEMU in UEFI mode (requires OVMF)"
        Write-Host "  qemu-debug   - Run QEMU with GDB server (port 1234)"
        Write-Host "  qemu-vga     - Run QEMU with VGA display"
        Write-Host ""
        Write-Host "General:"
        Write-Host "  clean        - Clean all build artifacts"
    }
}
