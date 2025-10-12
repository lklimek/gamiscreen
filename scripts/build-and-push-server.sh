#!/usr/bin/env bash
set -euo pipefail

# Determine repository root
ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE_REPO="lklimek/gamiscreen-server"
PRIMARY_TAG="${1:-}"

# Extract version from the workspace Cargo.toml
VERSION="$(awk '
  /^\[workspace.package]$/ { in_pkg = 1; next }
  /^\[/ { in_pkg = 0 }
  in_pkg && $1 == "version" {
    gsub(/"/, "", $3)
    print $3
    exit
  }
' "${ROOT_DIR}/Cargo.toml")"

if [[ -z "${VERSION}" ]]; then
  echo "Failed to read version from Cargo.toml" >&2
  exit 1
fi

# Derive a reasonable default tag when none is supplied explicitly.
if [[ -z "${PRIMARY_TAG}" ]]; then
  if [[ "${VERSION}" == *-* ]]; then
    PRIMARY_TAG="${VERSION##*-}"
  else
    PRIMARY_TAG="latest"
  fi
fi

TAGS=("${IMAGE_REPO}:${PRIMARY_TAG}")
if [[ "${PRIMARY_TAG}" != "${VERSION}" ]]; then
  TAGS+=("${IMAGE_REPO}:${VERSION}")
fi

echo "Building and pushing:"
for tag in "${TAGS[@]}"; do
  echo "  ${tag}"
done

build_args=(
  docker buildx build
  -f "${ROOT_DIR}/gamiscreen-server/Dockerfile"
)

for tag in "${TAGS[@]}"; do
  build_args+=(-t "${tag}")
done

build_args+=(
  --push
  "${ROOT_DIR}"
)

"${build_args[@]}"

echo "Done."
