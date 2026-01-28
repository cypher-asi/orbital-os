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
        Copy-Item "$releaseDir\permission_service.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\idle.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\memhog.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\sender.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\receiver.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\pingpong.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\clock.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\calculator.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\settings.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\identity_service.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\vfs_service.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\time_service.wasm" "$ProjectRoot\web\processes\" -Force
        Copy-Item "$releaseDir\keystore_service.wasm" "$ProjectRoot\web\processes\" -Force
        
        Write-Host "Process binaries built successfully!" -ForegroundColor Green
    }
    finally {
        Pop-Location
    }
}

function Build-QemuProcesses {
    Write-Step "Building QEMU process WASM binaries (without shared memory)"
    
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
        
        # Build without shared memory using simple cargo (no build-std needed without atomics)
        Write-Host "Building zos-init (no shared memory)..."
        cargo +nightly build -p zos-init --target wasm32-unknown-unknown --release
        if ($LASTEXITCODE -ne 0) { throw "zos-init build failed" }
        
        Write-Host "Building zos-system-procs (no shared memory)..."
        cargo +nightly build -p zos-system-procs --target wasm32-unknown-unknown --release
        if ($LASTEXITCODE -ne 0) { throw "zos-system-procs build failed" }
        
        Write-Host "Building zos-apps (no shared memory)..."
        cargo +nightly build -p zos-apps --bins --target wasm32-unknown-unknown --release
        if ($LASTEXITCODE -ne 0) { throw "zos-apps build failed" }
        
        Write-Host "Building zos-services (no shared memory)..."
        cargo +nightly build -p zos-services --bins --target wasm32-unknown-unknown --release
        if ($LASTEXITCODE -ne 0) { throw "zos-services build failed" }
        
        # Copy to qemu/processes
        Write-Host "Copying process binaries to qemu/processes..."
        if (-not (Test-Path "$ProjectRoot\qemu\processes")) {
            New-Item -ItemType Directory -Path "$ProjectRoot\qemu\processes" -Force | Out-Null
        }
        
        $releaseDir = "$ProjectRoot\target\wasm32-unknown-unknown\release"
        Copy-Item "$releaseDir\zos_init.wasm" "$ProjectRoot\qemu\processes\init.wasm" -Force
        Copy-Item "$releaseDir\terminal.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\permission_service.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\idle.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\memhog.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\sender.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\receiver.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\pingpong.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\clock.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\calculator.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\settings.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\identity_service.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\vfs_service.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\time_service.wasm" "$ProjectRoot\qemu\processes\" -Force
        Copy-Item "$releaseDir\keystore_service.wasm" "$ProjectRoot\qemu\processes\" -Force
        
        Write-Host "QEMU process binaries built successfully!" -ForegroundColor Green
    }
    finally {
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
        "-serial", "stdio",
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
