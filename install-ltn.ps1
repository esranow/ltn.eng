<#
.SYNOPSIS
    LTN.ENG Host Installer
    Adds D:\ltn-eng-workspace\bin to User PATH environment variable.
#>

$BinDir = "D:\ltn-eng-workspace\bin"

Write-Host "=== Installing LTN.ENG Global CLI ===" -ForegroundColor Cyan

if (-not (Test-Path $BinDir)) {
    New-Item -ItemType Directory -Force -Path $BinDir | Out-Null
}

# Read current User PATH
$UserPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($UserPath -notlike "*$BinDir*") {
    $NewPath = "$UserPath;$BinDir"
    [Environment]::SetEnvironmentVariable("PATH", $NewPath, "User")
    Write-Host "[✓] Added $BinDir to User PATH environment variable." -ForegroundColor Green
} else {
    Write-Host "[✓] $BinDir is already in User PATH." -ForegroundColor Green
}

Write-Host "`n=== LTN.ENG Installation Complete ===" -ForegroundColor Green
Write-Host "You can now run 'ltn' commands from any terminal session!" -ForegroundColor Yellow
Write-Host "Examples:" -ForegroundColor Gray
Write-Host "  ltn doctor" -ForegroundColor Gray
Write-Host "  ltn analyze ." -ForegroundColor Gray
Write-Host "  ltn architect ." -ForegroundColor Gray
Write-Host "  ltn status" -ForegroundColor Gray
