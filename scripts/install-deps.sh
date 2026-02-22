#!/usr/bin/env bash
# Install ALL build dependencies (system packages, Rust nightly, npm modules).
# This is a convenience wrapper — each step can also be run individually:
#
#   ./scripts/install-deps-system.sh   — system packages (pkg-config, libdbus, libsqlite3)
#   ./scripts/install-deps-rust.sh     — Rust nightly toolchain
#   ./scripts/install-deps-web.sh      — npm dependencies for gamiscreen-web
#
# Usage: ./scripts/install-deps.sh

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

errors=0

"$SCRIPT_DIR/install-deps-system.sh" || errors=$((errors + 1))
"$SCRIPT_DIR/install-deps-rust.sh"   || errors=$((errors + 1))
"$SCRIPT_DIR/install-deps-web.sh"    || errors=$((errors + 1))

if (( errors )); then
    printf '\033[1;33m[warn]\033[0m  Completed with %d error(s)\n' "$errors"
    exit 1
else
    printf '\033[1;32m[ ok ]\033[0m  All dependencies are ready\n'
fi
