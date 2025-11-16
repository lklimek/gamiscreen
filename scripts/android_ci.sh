#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 [debug|release]" >&2
  exit 1
}

BUILD_TYPE="${1:-debug}"
if [[ "${BUILD_TYPE}" != "debug" && "${BUILD_TYPE}" != "release" ]]; then
  usage
fi

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
  pushd "${WEB_DIR}" >/dev/null
  if [[ -f package-lock.json ]]; then
    npm ci --no-audit --no-fund
  else
    npm install --no-audit --no-fund
  fi
  VITE_BASE_PATH="/android-assets/" npm run build
  popd >/dev/null
else
  echo "Warning: gamiscreen-web directory not found; skipping embedded PWA build." >&2
fi

cd "${ANDROID_DIR}"

if [[ ! -x "./gradlew" ]]; then
  echo "Gradle wrapper not found. Run 'gradle wrapper --gradle-version 9.2' inside android/ before executing CI tasks." >&2
  exit 1
fi

if [[ "${BUILD_TYPE}" == "release" ]]; then
  ./gradlew clean lint test assembleRelease bundleRelease
else
  ./gradlew clean lint test assembleDebug
fi

METADATA_FILE="${ANDROID_DIR}/app/build/outputs/apk/${BUILD_TYPE}/output-metadata.json"
if [[ -f "${METADATA_FILE}" ]]; then
  VERSION_NAME=$(jq -r '.elements[0].versionName // .variantOutputs[0].versionName' "${METADATA_FILE}")
  VERSION_CODE=$(jq -r '.elements[0].versionCode // .variantOutputs[0].versionCode' "${METADATA_FILE}")
  echo "versionName=${VERSION_NAME}"
  echo "versionCode=${VERSION_CODE}"
else
  echo "output-metadata.json not found at ${METADATA_FILE}" >&2
fi
