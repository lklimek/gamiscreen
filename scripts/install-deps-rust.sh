#!/usr/bin/env bash
# Install the Rust nightly toolchain (needed for cargo +nightly fmt).
# Warns on failure but continues.
#
# Usage: ./scripts/install-deps-rust.sh

set -uo pipefail

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ ok ]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[ !! ]\033[0m  %s\n' "$*"; }

if ! rustup toolchain list | grep -q nightly; then
    info "Installing Rust nightly toolchain ..."
    if rustup toolchain install nightly --profile minimal; then
        ok "Rust nightly installed"
    else
        err "Failed to install Rust nightly toolchain"
        exit 1
    fi
else
    ok "Rust nightly already installed"
fi
