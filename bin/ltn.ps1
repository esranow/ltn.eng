<#
.SYNOPSIS
    LTN.ENG Host Launcher Script
    Delegates commands to the ltn-sandbox container if running, or falls back to native Windows host binary.
#>

$ErrorActionPreference = "Continue"
$WorkspacePath = "D:\ltn-eng-workspace"
$NativeCliDebug = "D:\ltn-eng-workspace\target\debug\ltn.exe"
$NativeCliRelease = "D:\ltn-eng-workspace\target\release\ltn.exe"

$Arguments = $args

# Check if native binary exists and Docker is not ready
$UseNative = $true
if (Get-Command docker -ErrorAction SilentlyContinue) {
    $DockerRunning = docker info 2>&1
    if ($LASTEXITCODE -eq 0) {
        $SandboxStatus = docker inspect -f '{{.State.Running}}' ltn-sandbox 2>$null
        if ($SandboxStatus -eq "true") {
            $UseNative = $false
        }
    }
}

if ($UseNative) {
    if (Test-Path $NativeCliDebug) {
        & $NativeCliDebug @Arguments
        exit $LASTEXITCODE
    } elseif (Test-Path $NativeCliRelease) {
        & $NativeCliRelease @Arguments
        exit $LASTEXITCODE
    } else {
        Write-Host "Compiling native ltn-cli..." -ForegroundColor Yellow
        $env:Z3_SYS_Z3_HEADER = "D:\beam\z3-bin\z3-4.12.2-x64-win\include\z3.h"
        $env:BINDGEN_EXTRA_CLANG_ARGS = "-ID:\beam\z3-bin\z3-4.12.2-x64-win\include"
        $env:Z3_DIR = "D:\beam\z3-bin\z3-4.12.2-x64-win"
        cargo build --manifest-path "D:\ltn-eng-workspace\Cargo.toml" --bin ltn
        & $NativeCliDebug @Arguments
        exit $LASTEXITCODE
    }
} else {
    $ArgString = $Arguments -join " "
    if (-not $ArgString) { $ArgString = "--help" }
    wsl -d Ubuntu-24.04 bash -c "sudo docker exec ltn-sandbox /workspace/target/release/ltn-cli $ArgString"
}

