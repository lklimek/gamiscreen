#!/usr/bin/env bash
# Install system and toolchain dependencies required to build gamiscreen.
# Intended for CI environments and Claude Code sessions.
#
# Each section warns on failure but continues, so a network outage does not
# block the entire session when some dependencies are already present.
#
# Usage: ./scripts/install-deps.sh

set -uo pipefail

# ---------- helpers -----------------------------------------------------------

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ ok ]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[ !! ]\033[0m  %s\n' "$*"; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
errors=0

# ---------- system packages ---------------------------------------------------

info "Checking system dependencies ..."

REQUIRED_PKGS=(pkg-config libdbus-1-dev libsqlite3-dev)
missing=()

for pkg in "${REQUIRED_PKGS[@]}"; do
    if ! dpkg -s "$pkg" &>/dev/null; then
        warn "$pkg is not installed"
        missing+=("$pkg")
    fi
done

if (( ${#missing[@]} )); then
    info "Installing missing packages: ${missing[*]}"
    if sudo apt-get update -qq && sudo apt-get install -y -qq "${missing[@]}"; then
        ok "System packages installed"
    else
        err "Failed to install system packages (network unavailable?)"
        errors=$((errors + 1))
    fi
else
    ok "All system packages present"
fi

# ---------- Rust nightly toolchain (for cargo fmt) ----------------------------

if ! rustup toolchain list | grep -q nightly; then
    info "Installing Rust nightly toolchain ..."
    if rustup toolchain install nightly --profile minimal; then
        ok "Rust nightly installed"
    else
        err "Failed to install Rust nightly toolchain"
        errors=$((errors + 1))
    fi
else
    ok "Rust nightly already installed"
fi

# ---------- npm dependencies --------------------------------------------------

WEB_DIR="$SCRIPT_DIR/../gamiscreen-web"

if [ -d "$WEB_DIR" ] && [ -f "$WEB_DIR/package.json" ]; then
    if [ ! -d "$WEB_DIR/node_modules" ]; then
        info "Installing npm dependencies in gamiscreen-web ..."
        if npm ci --prefix "$WEB_DIR"; then
            ok "npm dependencies installed"
        else
            err "Failed to install npm dependencies"
            errors=$((errors + 1))
        fi
    else
        ok "npm dependencies already present"
    fi
fi

# ---------- summary -----------------------------------------------------------

if (( errors )); then
    warn "Completed with $errors error(s) — some dependencies may be missing"
    exit 1
else
    ok "All dependencies are ready"
fi
