@echo off
set Z3_SYS_Z3_HEADER=D:\beam\z3-bin\z3-4.12.2-x64-win\include\z3.h
set BINDGEN_EXTRA_CLANG_ARGS=-ID:\beam\z3-bin\z3-4.12.2-x64-win\include
set Z3_DIR=D:\beam\z3-bin\z3-4.12.2-x64-win

echo ⚡ Launching MAP-MTL (MetalMap) Standalone Vibe Coding TUI...
cargo run --package ltn-cli --bin map-mtl -- tui
