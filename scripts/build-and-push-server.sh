#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CARGO_TOML="${ROOT_DIR}/Cargo.toml"
IMAGE_REPO="lklimek/gamiscreen-server"

usage() {
  cat <<'EOF'
Usage:
  build-and-push-server.sh [dev|release] [--version <version>]
  build-and-push-server.sh --mode <dev|release> [--version <version>]

Options:
  --mode <dev|release>        Explicitly set the mode (default: dev)
  --version <version>         Override the version used for tagging
  -h, --help                  Show this help message

Examples:
  build-and-push-server.sh              # Dev build (default)
  build-and-push-server.sh release      # Release build using Cargo.toml version
  build-and-push-server.sh dev --version 1.3.0-dev
  build-and-push-server.sh release --version v1.3.0
EOF
}

validate_semver() {
  local version="${1:-}"
  # Bash [[ =~ ]] uses POSIX ERE, so we avoid PCRE extensions such as (?:...).
  local semver_regex='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(0|[1-9A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9A-Za-z-][0-9A-Za-z-]*))*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'

  if [[ -z "${version}" ]]; then
    echo "Version must not be empty" >&2
    return 1
  fi

  if [[ ! "${version}" =~ ${semver_regex} ]]; then
    echo "Version \"${version}\" is not valid semantic versioning (expected MAJOR.MINOR.PATCH with optional pre-release/build metadata)." >&2
    return 1
  fi

  return 0
}

normalize_version_input() {
  local version="${1:-}"

  if [[ "${version}" =~ ^[Vv][0-9] ]]; then
    version="${version:1}"
  fi

  printf '%s\n' "${version}"
}

read_workspace_version() {
  awk '
    BEGIN { in_pkg = 0 }
    /^\[workspace\.package\]$/ { in_pkg = 1; next }
    /^\[/ { in_pkg = 0 }
    in_pkg && $1 == "version" {
      gsub(/"/, "", $3)
      print $3
      exit
    }
  ' "${CARGO_TOML}"
}

ensure_dev_suffix() {
  local raw_version sanitized target
  raw_version="$(read_workspace_version)"
  if [[ -z "${raw_version}" ]]; then
    echo "Failed to read version from Cargo.toml" >&2
    return 1
  fi

  sanitized="$(normalize_version_input "${raw_version}")"
  if [[ -z "${sanitized}" ]]; then
    echo "Workspace version is empty after normalization" >&2
    return 1
  fi

  if [[ "${sanitized}" == *-dev ]]; then
    if ! validate_semver "${sanitized}"; then
      return 1
    fi
    if [[ "${raw_version}" != "${sanitized}" ]]; then
      if ! sanitized="$(set_workspace_version "${sanitized}")"; then
        return 1
      fi
    fi
    printf '%s\n' "${sanitized}"
    return 0
  fi

  if ! validate_semver "${sanitized}"; then
    return 1
  fi

  if ! target="$(set_workspace_version "${sanitized}-dev")"; then
    return 1
  fi

  printf '%s\n' "${target}"
}

set_workspace_version() {
  local new_version_input="${1:-}"
  local new_version current_raw

  new_version="$(normalize_version_input "${new_version_input}")"

  if [[ -z "${new_version}" ]]; then
    echo "New version must be provided" >&2
    return 1
  fi

  if ! validate_semver "${new_version}"; then
    return 1
  fi

  current_raw="$(read_workspace_version)"
  if [[ -z "${current_raw}" ]]; then
    echo "Failed to read current version from Cargo.toml" >&2
    return 1
  fi

  if [[ "${current_raw}" == "${new_version}" ]]; then
    printf '%s\n' "${new_version}"
    return 0
  fi

  if ! sed -i '0,/version = "'"${current_raw}"'"/s//version = "'"${new_version}"'"/' "${CARGO_TOML}"; then
    echo "Failed to update version in Cargo.toml" >&2
    return 1
  fi

  printf '%s\n' "${new_version}"
}

build_and_push() {
  local primary_tag="${1:-}"
  local version="${2:-}"
  local tags=()

  if [[ -z "${primary_tag}" ]]; then
    echo "Primary tag is required" >&2
    return 1
  fi

  if [[ -z "${version}" ]]; then
    echo "Version is required" >&2
    return 1
  fi

  if ! validate_semver "${version}"; then
    return 1
  fi

  tags+=("${IMAGE_REPO}:${primary_tag}")
  if [[ "${primary_tag}" != "${version}" ]]; then
    tags+=("${IMAGE_REPO}:${version}")
  fi

  echo "Building and pushing:"
  for tag in "${tags[@]}"; do
    echo "  ${tag}"
  done

  local build_args=(
    docker buildx build
    -f "${ROOT_DIR}/gamiscreen-server/Dockerfile"
  )

  for tag in "${tags[@]}"; do
    build_args+=(-t "${tag}")
  done

  build_args+=(
    --push
    "${ROOT_DIR}"
  )

  "${build_args[@]}"

  echo "Pushed to:"
  for tag in "${tags[@]}"; do
    echo "  ${tag}"
  done

  echo "Done."
}

MODE="dev"
MODE_SET=""
VERSION_OVERRIDE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    dev|release)
      if [[ -n "${MODE_SET}" && "${MODE_SET}" != "$1" ]]; then
        echo "Mode specified multiple times" >&2
        usage
        exit 1
      fi
      MODE="$1"
      MODE_SET="$1"
      shift
      ;;
    --mode)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --mode" >&2
        usage
        exit 1
      fi
      if [[ -n "${MODE_SET}" && "${MODE_SET}" != "$2" ]]; then
        echo "Mode specified multiple times" >&2
        usage
        exit 1
      fi
      MODE="$2"
      MODE_SET="$2"
      shift 2
      ;;
    --mode=*)
      MODE="${1#*=}"
      if [[ -n "${MODE_SET}" && "${MODE_SET}" != "${MODE}" ]]; then
        echo "Mode specified multiple times" >&2
        usage
        exit 1
      fi
      MODE_SET="${MODE}"
      shift
      ;;
    --version)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --version" >&2
        usage
        exit 1
      fi
      if [[ -n "${VERSION_OVERRIDE}" ]]; then
        echo "--version specified multiple times" >&2
        usage
        exit 1
      fi
      VERSION_OVERRIDE="$2"
      shift 2
      ;;
    --version=*)
      if [[ -n "${VERSION_OVERRIDE}" ]]; then
        echo "--version specified multiple times" >&2
        usage
        exit 1
      fi
      VERSION_OVERRIDE="${1#*=}"
      shift
      ;;
    *)
      echo "Unexpected argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ -n "${VERSION_OVERRIDE}" ]]; then
  VERSION_OVERRIDE="$(normalize_version_input "${VERSION_OVERRIDE}")"
fi

MODE="$(printf '%s' "${MODE}" | tr '[:upper:]' '[:lower:]')"
case "${MODE}" in
  dev|release)
    ;;
  *)
    echo "Unsupported mode: ${MODE}" >&2
    usage
    exit 1
    ;;
esac

case "${MODE}" in
  dev)
    if [[ -n "${VERSION_OVERRIDE}" ]]; then
      dev_version="${VERSION_OVERRIDE}"
      if [[ "${dev_version}" != *-dev ]]; then
        dev_version="${dev_version}-dev"
      fi
      if ! dev_version="$(set_workspace_version "${dev_version}")"; then
        exit 1
      fi
      echo "Workspace version for dev build: ${dev_version}"
    else
      if ! dev_version="$(ensure_dev_suffix)"; then
        exit 1
      fi
      echo "Workspace version for dev build: ${dev_version}"
    fi
    if [[ -z "${dev_version}" ]]; then
      echo "Dev version could not be determined" >&2
      exit 1
    fi
    build_and_push "dev" "${dev_version}"
    ;;
  release)
    release_version="${VERSION_OVERRIDE}"
    workspace_version_raw="$(read_workspace_version)"
    if [[ -z "${workspace_version_raw}" ]]; then
      echo "Failed to read version from Cargo.toml" >&2
      exit 1
    fi
    workspace_version="$(normalize_version_input "${workspace_version_raw}")"
    if [[ -z "${workspace_version}" ]]; then
      echo "Workspace version is empty after normalization" >&2
      exit 1
    fi
    if [[ -z "${release_version}" ]]; then
      release_version="${workspace_version}"
    fi
    if [[ "${release_version}" == *-dev ]]; then
      echo "Release version still has -dev suffix (${release_version})" >&2
      exit 1
    fi
    if ! validate_semver "${workspace_version}"; then
      echo "Workspace version (${workspace_version}) is not valid semantic versioning" >&2
      exit 1
    fi
    if ! validate_semver "${release_version}"; then
      exit 1
    fi
    if [[ "${release_version}" != "${workspace_version}" ]]; then
      echo "Provided release version (${release_version}) does not match workspace version (${workspace_version})" >&2
      exit 1
    fi
    echo "Validated release version ${release_version}."
    build_and_push "latest" "${release_version}"
    ;;
esac
