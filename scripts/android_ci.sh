#!/usr/bin/env bash
set -euo pipefail
set -x

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(realpath "${SCRIPT_DIR}/..")"
ANDROID_DIR="${PROJECT_ROOT}/android"
WEB_DIR="${PROJECT_ROOT}/gamiscreen-web"

if [[ ! -d "${ANDROID_DIR}" ]]; then
  echo "Android project directory not found at ${ANDROID_DIR}" >&2
  exit 1
fi

if [[ -d "${WEB_DIR}" ]]; then
  cd "${WEB_DIR}"
  if [[ -f package-lock.json ]]; then
    npm ci --no-audit --no-fund
  else
    npm install --no-audit --no-fund
  fi
  npm run build
  cd "${ANDROID_DIR}"
else
  echo "Warning: gamiscreen-web directory not found; skipping embedded PWA build." >&2
fi

cd "${ANDROID_DIR}"

if [[ ! -x "./gradlew" ]]; then
  echo "Gradle wrapper not found. Run 'gradle wrapper --gradle-version 9.2' inside android/ before executing CI tasks." >&2
  exit 1
fi

./gradlew lint test assembleDebug
