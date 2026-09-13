$ErrorActionPreference = "Stop"

# Set Z3 environment paths cleanly without line breaks
$env:Z3_SYS_Z3_HEADER = "D:\beam\z3-bin\z3-4.12.2-x64-win\include\z3.h"
$env:BINDGEN_EXTRA_CLANG_ARGS = "-ID:\beam\z3-bin\z3-4.12.2-x64-win\include"
$env:Z3_DIR = "D:\beam\z3-bin\z3-4.12.2-x64-win"

Write-Host "🌩️ Launching ltn.eng AGY Vibe Coding TUI..." -ForegroundColor Cyan

cargo run --package ltn-cli -- tui
