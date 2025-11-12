#!/usr/bin/env bash
set -euo pipefail
set -x

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(realpath "${SCRIPT_DIR}/..")"
ANDROID_DIR="${PROJECT_ROOT}/android"

if [[ ! -d "${ANDROID_DIR}" ]]; then
  echo "Android project directory not found at ${ANDROID_DIR}" >&2
  exit 1
fi

cd "${ANDROID_DIR}"

if [[ ! -x "./gradlew" ]]; then
  echo "Gradle wrapper not found. Run 'gradle wrapper --gradle-version 9.2' inside android/ before executing CI tasks." >&2
  exit 1
fi

./gradlew lint test assembleDebug
