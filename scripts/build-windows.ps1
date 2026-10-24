# Builds the PlatypusDB installers (.exe with NSIS, .msi) and the portable executable on
# Windows, from a clean checkout or an existing one.
#
#   .\scripts\build-windows.ps1            installers and executable
#   .\scripts\build-windows.ps1 -ExeOnly   only target\release\platypus-db.exe, faster
#   .\scripts\build-windows.ps1 -Open      opens the output folder at the end
#
# Without TAURI_SIGNING_PRIVATE_KEY in the environment the updater files (.sig) are not made:
# the installers work, the in-app updates of this build do not.
param(
    [switch]$ExeOnly,
    [switch]$Open
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$desktop = Join-Path $root "apps\desktop"
$release = Join-Path $root "target\release"

function Need($command, $hint) {
    if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
        throw "$command not found: $hint"
    }
}

Need "cargo" "install Rust from https://rustup.rs"
Need "npm" "install Node.js from https://nodejs.org"

# A copy of the app started from target\release keeps the executable locked.
$running = Get-Process platypus-db -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($release, [StringComparison]::OrdinalIgnoreCase) }
if ($running) {
    throw "PlatypusDB is running from $release (process $($running.Id -join ', ')): close it and run the script again"
}

$started = Get-Date
Push-Location $desktop
try {
    if (-not (Test-Path "node_modules")) {
        Write-Host "Installing the frontend's packages..."
        npm ci
        if ($LASTEXITCODE -ne 0) { throw "npm ci failed" }
    }

    $arguments = @("run", "tauri", "build", "--")
    if ($ExeOnly) {
        $arguments += "--no-bundle"
    } elseif (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
        # The updater files need the release key, which is only in the release workflow.
        $override = Join-Path $env:TEMP "platypus-build-config.json"
        Set-Content -Path $override -Value '{"bundle":{"createUpdaterArtifacts":false}}' -Encoding ascii
        $arguments += @("--config", $override)
    }

    Write-Host "Building (the first build takes several minutes)..."
    & npm @arguments
    if ($LASTEXITCODE -ne 0) { throw "the build failed" }
} finally {
    Pop-Location
}

$exe = Join-Path $release "platypus-db.exe"
Write-Host ""
Write-Host "Executable: $exe"
if (-not $ExeOnly) {
    $bundle = Join-Path $release "bundle"
    Get-ChildItem -Path (Join-Path $bundle "nsis"), (Join-Path $bundle "msi") -Include *.exe, *.msi -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -ge $started } |
        ForEach-Object { Write-Host "Installer:  $($_.FullName)" }
}
if ($Open) {
    Invoke-Item $(if ($ExeOnly) { $release } else { Join-Path $release "bundle" })
}
