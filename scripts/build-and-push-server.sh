#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE_REPO="lklimek/gamiscreen-server"
TAG="${1:-latest}"
IMAGE="${IMAGE_REPO}:${TAG}"

echo "Building ${IMAGE}..."
docker buildx build \
  -f "${ROOT_DIR}/gamiscreen-server/Dockerfile" \
  -t "${IMAGE}" \
  --push \
  "${ROOT_DIR}"

echo "Done."
