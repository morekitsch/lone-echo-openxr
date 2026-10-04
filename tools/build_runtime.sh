#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
toolchain="${LLVM_MINGW_DIR:-$PWD/working/llvm-mingw-20260922-ucrt-ubuntu-22.04-x86_64}"
export PATH="$toolchain/bin:$PATH"
export CARGO_HOME="${CARGO_HOME:-$PWD/working/cargo}"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER=x86_64-w64-mingw32-clang
export RUSTFLAGS='-C target-feature=+crt-static'
cargo +stable build --locked --manifest-path runtime/Cargo.toml --target x86_64-pc-windows-gnullvm --release
python tools/assemble_payload.py
