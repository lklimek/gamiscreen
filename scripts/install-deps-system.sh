#!/usr/bin/env bash
# Install system packages required to build gamiscreen (Rust workspace).
# Intended for CI environments and Claude Code sessions.
#
# Warns on failure but continues, so a network outage does not block the
# session when packages are already present.
#
# Usage: ./scripts/install-deps-system.sh

set -uo pipefail

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ ok ]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[ !! ]\033[0m  %s\n' "$*"; }

errors=0

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

if (( errors )); then
    warn "Completed with $errors error(s) — some packages may be missing"
    exit 1
else
    ok "System dependencies are ready"
fi
