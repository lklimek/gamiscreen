#!/usr/bin/env bash
set -euo pipefail

# Cross-compile gamiscreen-client for Windows (x86_64-pc-windows-gnu) on Linux.
#
# Prerequisites (Debian/Ubuntu):
#   sudo apt-get update && sudo apt-get install -y mingw-w64
#
# This script:
#   - Ensures the Rust Windows GNU target is installed
#   - Uses MinGW-w64 linker and archiver
#   - Builds a release binary
#   - Copies the .exe to ./dist

TARGET="x86_64-pc-windows-gnu"
PKG="gamiscreen-client"

echo "[+] Checking toolchain availability..."
command -v rustup >/dev/null || { echo "rustup not found"; exit 1; }
command -v cargo >/dev/null || { echo "cargo not found"; exit 1; }
command -v x86_64-w64-mingw32-gcc >/dev/null || {
  echo "x86_64-w64-mingw32-gcc not found. Install MinGW-w64 (e.g., apt install mingw-w64).";
  exit 1;
}
command -v x86_64-w64-mingw32-ar >/dev/null || {
  echo "x86_64-w64-mingw32-ar not found. Install MinGW-w64 (e.g., apt install mingw-w64).";
  exit 1;
}

echo "[+] Adding Rust target ${TARGET} (idempotent)..."
rustup target add "${TARGET}" >/dev/null 2>&1 || true

export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar

echo "[+] Building ${PKG} for ${TARGET} in release mode..."
cargo build --release -p "${PKG}" --target "${TARGET}"

BIN_PATH="target/${TARGET}/release/${PKG}.exe"
if [[ ! -f "${BIN_PATH}" ]]; then
  echo "[-] Build artifact not found at ${BIN_PATH}"
  exit 1
fi

mkdir -p dist
OUT="dist/${PKG}-windows-x86_64.exe"
cp -f "${BIN_PATH}" "${OUT}"
echo "[+] Done: ${OUT}"

