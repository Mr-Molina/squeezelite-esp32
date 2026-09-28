<#
.SYNOPSIS
    Unified Local Build, Development, and Flashing Tool for squeezelite-esp32.

.DESCRIPTION
    Wraps the official containerized ESP-IDF v4.3.5 environment (via Podman/WSL or Docker)
    and host esptool.py to build, test, and flash firmware variants and web applications.

.PARAMETER Target
    Hardware target configuration. Default: 'I2S-4MFlash'.
    Available targets: 'I2S-4MFlash', 'Muse', 'SqueezeAmp', 'I2S-S3'.

.PARAMETER Depth
    Audio depth (16 or 32). Default: 16.

.PARAMETER Webapp
    Builds the wifi-manager React/Webpack frontend inside the container.

.PARAMETER Clean
    Performs full clean of the build directory.

.PARAMETER Menuconfig
    Launches interactive idf.py menuconfig inside the container.

.PARAMETER Interactive
    Launches an interactive bash shell inside the container.

.PARAMETER Command
    Executes a custom command inside the container environment.

.PARAMETER Flash
    Flashes compiled binaries to connected ESP32 board using host esptool.py.

.PARAMETER Port
    Serial COM port for flashing (e.g. 'COM3'). Auto-detects if not specified.

.PARAMETER Baud
    Flashing baud rate. Default: 921600.

.PARAMETER Status
    Displays diagnostic environment report (tools, images, serial ports).

.EXAMPLE
    .\scripts\build.ps1
    .\scripts\build.ps1 -Target Muse
    .\scripts\build.ps1 -Webapp
    .\scripts\build.ps1 -Menuconfig
    .\scripts\build.ps1 -Flash -Port COM3
    .\scripts\build.ps1 -Status
#>

[CmdletBinding()]
param (
    [Parameter(Position = 0)]
    [string]$Target = "I2S-4MFlash",

    [int]$Depth = 16,

    [switch]$Webapp,
    [switch]$Clean,
    [switch]$Menuconfig,
    [switch]$Interactive,
    [string]$Command,
    [switch]$Flash,
    [string]$Port,
    [int]$Baud = 921600,
    [switch]$Status
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectDir = (Resolve-Path "$ScriptDir\..").Path
$ImageName = "docker.io/sle118/squeezelite-esp32-idfv435"

function Write-Step {
    param([string]$Message)
    Write-Host "`n[+] $Message" -ForegroundColor Cyan
}

function Write-Success {
    param([string]$Message)
    Write-Host "[OK] $Message" -ForegroundColor Green
}

function Write-Warn {
    param([string]$Message)
    Write-Host "[!] $Message" -ForegroundColor Yellow
}

function Write-Err {
    param([string]$Message)
    Write-Host "[x] $Message" -ForegroundColor Red
}

function Get-ContainerEngine {
    # 1. Native podman
    if (Get-Command podman -ErrorAction SilentlyContinue) {
        return @{ Type = "native-podman"; Exec = "podman" }
    }
    # 2. Native docker
    if (Get-Command docker -ErrorAction SilentlyContinue) {
        return @{ Type = "native-docker"; Exec = "docker" }
    }
    # 3. WSL with podman
    $wslPodman = $false
    try {
        $check = wsl -- which podman 2>$null
        if ($check -and $check.Trim().Length -gt 0) {
            $wslPodman = $true
        }
    } catch {}

    if ($wslPodman) {
        return @{ Type = "wsl-podman"; Exec = "wsl -- podman" }
    }

    # 4. WSL with docker
    $wslDocker = $false
    try {
        $check = wsl -- which docker 2>$null
        if ($check -and $check.Trim().Length -gt 0) {
            $wslDocker = $true
        }
    } catch {}

    if ($wslDocker) {
        return @{ Type = "wsl-docker"; Exec = "wsl -- docker" }
    }

    return $null
}

function Get-WslPath {
    param([string]$WinPath)
    $normalized = $WinPath.Replace("\", "/")
    $res = wsl -- wslpath -a -u "$normalized"
    return $res.Trim()
}

function Get-HostEsptool {
    $candidates = @(
        "C:\Users\jmolina\.espressif\python_env\idf5.4_py3.11_env\Scripts\python.exe",
        "python.exe"
    )
    foreach ($cand in $candidates) {
        try {
            $ver = & $cand -m esptool version 2>$null
            if ($LASTEXITCODE -eq 0 -and $ver) {
                return $cand
            }
        } catch {}
    }
    return $null
}

function Show-StatusReport {
    Write-Host "==========================================================" -ForegroundColor Magenta
    Write-Host " Squeezelite-ESP32 Local Development Environment Status" -ForegroundColor Magenta
    Write-Host "==========================================================" -ForegroundColor Magenta

    # Check Host System
    Write-Host "`n[Host System]" -ForegroundColor Yellow
    Write-Host "  OS Version      : Windows $((Get-CimInstance Win32_OperatingSystem).Caption)"
    Write-Host "  Workspace       : $ProjectDir"

    # Container Engine
    Write-Host "`n[Container Engine]" -ForegroundColor Yellow
    $engine = Get-ContainerEngine
    if ($engine) {
        Write-Host "  Detected Engine : $($engine.Type) ($($engine.Exec))" -ForegroundColor Green
    } else {
        Write-Host "  Detected Engine : NONE (WSL / Podman / Docker not found)" -ForegroundColor Red
    }

    # Container Image
    Write-Host "`n[ESP-IDF v4.3.5 Container Image]" -ForegroundColor Yellow
    if ($engine) {
        $hasImg = $false
        if ($engine.Type.StartsWith("wsl")) {
            $imgCheck = wsl -- podman images -q $ImageName 2>$null
            if ($imgCheck) { $hasImg = $true }
        } else {
            $imgCheck = & $engine.Exec images -q $ImageName 2>$null
            if ($imgCheck) { $hasImg = $true }
        }

        if ($hasImg) {
            Write-Host "  Image ($ImageName) : Present locally" -ForegroundColor Green
        } else {
            Write-Host "  Image ($ImageName) : Not yet downloaded (will auto-pull)" -ForegroundColor Yellow
        }
    }

    # Flashing Tooling
    Write-Host "`n[Flashing Subsystem]" -ForegroundColor Yellow
    $esptoolPy = Get-HostEsptool
    if ($esptoolPy) {
        $ver = & $esptoolPy -m esptool version 2>$null | Select-Object -First 1
        Write-Host "  esptool.py      : $ver ($esptoolPy)" -ForegroundColor Green
    } else {
        Write-Host "  esptool.py      : Not found" -ForegroundColor Red
    }

    # Available Serial COM Ports
    Write-Host "`n[Connected Serial Devices]" -ForegroundColor Yellow
    try {
        [System.IO.Ports.SerialPort]::GetPortNames() | ForEach-Object {
            Write-Host "  Port            : $_" -ForegroundColor Green
        }
    } catch {}

    Write-Host "`n==========================================================" -ForegroundColor Magenta
}

# --- Action Dispatch ---

if ($Status) {
    Show-StatusReport
    return
}

# Check container engine
$engine = Get-ContainerEngine
if (-not $engine) {
    Write-Err "No supported container engine found! Please ensure WSL with Podman/Docker or native Docker is installed."
    exit 1
}

# If Flash requested, run host esptool
if ($Flash) {
    Write-Step "Executing ESP32 Flash Tooling..."
    $esptoolPy = Get-HostEsptool
    if (-not $esptoolPy) {
        Write-Err "esptool.py is not available in host Python environment."
        exit 1
    }

    # Auto-detect COM port if omitted
    if (-not $Port) {
        $ports = [System.IO.Ports.SerialPort]::GetPortNames()
        if ($ports.Count -eq 1) {
            $Port = $ports[0]
            Write-Host "Auto-detected serial port: $Port" -ForegroundColor Green
        } elseif ($ports.Count -gt 1) {
            Write-Warn "Multiple serial ports detected: $($ports -join ', '). Please specify with -Port <COMx>."
            exit 1
        } else {
            Write-Err "No connected COM ports found. Connect ESP32 via USB and retry."
            exit 1
        }
    }

    $flashArgsFile = "$ProjectDir\build\flash_project_args"
    if (Test-Path $flashArgsFile) {
        Write-Host "Flashing using build/flash_project_args on port $Port at $Baud baud..." -ForegroundColor Cyan
        Set-Location "$ProjectDir\build"
        & $esptoolPy -m esptool --chip esp32 --port $Port --baud $Baud write_flash "@flash_project_args"
        Set-Location $ProjectDir
    } else {
        Write-Err "build/flash_project_args not found. Please build the firmware first."
        exit 1
    }
    Write-Success "Flashing completed successfully."
    return
}

# Resolve workspace mount path
if ($engine.Type.StartsWith("wsl")) {
    $mountPath = Get-WslPath $ProjectDir
} else {
    $mountPath = $ProjectDir
}

# Build container run command base
$containerWorkspace = "/workspace/squeezelite-esp32"
$volumeArg = "${mountPath}:${containerWorkspace}:z"

function Invoke-InContainer {
    param(
        [string]$Cmd,
        [bool]$IsInteractive = $false
    )

    $itFlag = if ($IsInteractive) { "-it" } else { "" }

    if ($engine.Type -eq "wsl-podman") {
        $fullCmd = "podman run --rm $itFlag -v $volumeArg -w $containerWorkspace $ImageName bash -c `"$Cmd`""
        if ($IsInteractive) {
            wsl -- podman run --rm -it -v $volumeArg -w $containerWorkspace $ImageName bash -c "$Cmd"
        } else {
            wsl -- podman run --rm -v $volumeArg -w $containerWorkspace $ImageName bash -c "$Cmd"
        }
    } elseif ($engine.Type -eq "wsl-docker") {
        if ($IsInteractive) {
            wsl -- docker run --rm -it -v $volumeArg -w $containerWorkspace $ImageName bash -c "$Cmd"
        } else {
            wsl -- docker run --rm -v $volumeArg -w $containerWorkspace $ImageName bash -c "$Cmd"
        }
    } elseif ($engine.Type -eq "native-podman") {
        & podman run --rm $itFlag -v $volumeArg -w $containerWorkspace $ImageName bash -c "$Cmd"
    } elseif ($engine.Type -eq "native-docker") {
        & docker run --rm $itFlag -v $volumeArg -w $containerWorkspace $ImageName bash -c "$Cmd"
    }
}

# Interactive Shell
if ($Interactive) {
    Write-Step "Opening interactive bash shell inside ESP-IDF container..."
    Invoke-InContainer "bash" $true
    return
}

# Menuconfig
if ($Menuconfig) {
    Write-Step "Launching idf.py menuconfig..."
    Invoke-InContainer "source /opt/esp/idf/export.sh && idf.py menuconfig" $true
    return
}

# Clean
if ($Clean) {
    Write-Step "Cleaning build artifacts..."
    Invoke-InContainer "source /opt/esp/idf/export.sh && idf.py fullclean" $false
    Write-Success "Build directory cleaned."
    return
}

# Webapp Build
if ($Webapp) {
    Write-Step "Compiling Web Frontend (components/wifi-manager/webapp)..."
    Invoke-InContainer "cd components/wifi-manager/webapp && npm install && npm run build" $false
    Write-Success "Web application compiled successfully."
    return
}

# Custom command
if ($Command) {
    Write-Step "Running custom command: $Command"
    Invoke-InContainer "source /opt/esp/idf/export.sh && $Command" $false
    return
}

# Firmware Build (Default)
Write-Step "Building Firmware Variant: Target='$Target', Depth=$Depth..."

$targetDefaults = "$ProjectDir\build-scripts\${Target}-sdkconfig.defaults"
if (-not (Test-Path $targetDefaults)) {
    Write-Warn "Target defaults '$targetDefaults' not found. Checking if target is sdkconfig name..."
}

$buildCmd = "export TARGET_BUILD_NAME='$Target' && export DEPTH='$Depth' && bash ./buildFirmware.sh"
Invoke-InContainer "$buildCmd" $false

if ($LASTEXITCODE -eq 0) {
    Write-Success "Firmware build completed successfully for target '$Target'!"
    Write-Host "Binaries located in: $ProjectDir\build\" -ForegroundColor Cyan
} else {
    Write-Err "Build failed with exit code $LASTEXITCODE."
    exit $LASTEXITCODE
}
