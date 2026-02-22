#!/usr/bin/env bash
# Install npm dependencies for the gamiscreen-web React SPA.
# Warns on failure but continues.
#
# Usage: ./scripts/install-deps-web.sh

set -uo pipefail

info()  { printf '\033[1;34m[info]\033[0m  %s\n' "$*"; }
warn()  { printf '\033[1;33m[warn]\033[0m  %s\n' "$*"; }
ok()    { printf '\033[1;32m[ ok ]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[ !! ]\033[0m  %s\n' "$*"; }

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WEB_DIR="$SCRIPT_DIR/../gamiscreen-web"

if [ ! -d "$WEB_DIR" ] || [ ! -f "$WEB_DIR/package.json" ]; then
    err "gamiscreen-web directory or package.json not found"
    exit 1
fi

if [ ! -d "$WEB_DIR/node_modules" ]; then
    info "Installing npm dependencies in gamiscreen-web ..."
    if npm ci --prefix "$WEB_DIR"; then
        ok "npm dependencies installed"
    else
        err "Failed to install npm dependencies"
        exit 1
    fi
else
    ok "npm dependencies already present"
fi
