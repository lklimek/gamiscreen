#!/usr/bin/env bash
set -euo pipefail

# Adds 10 minutes to child "alice" via the server API.
# Prompts for parent username/password, logs in to obtain a JWT, and uses it.
#
# Usage:
#   scripts/reward_alice_10.sh
#
# Optional env vars:
#   SERVER_URL  Base URL of the server (default: http://127.0.0.1:5151)

SERVER_URL=${SERVER_URL:-http://127.0.0.1:5151}
API_V1_PREFIX="/api/v1"

read -rp "Parent username: " USERNAME
read -rsp "Parent password: " PASSWORD; echo ""

# Build JSON safely using python3 if available, else jq, else naive (may fail on quotes)
build_json() {
  local u="$1" p="$2"
#   if command -v python3 >/dev/null 2>&1; then
#     U="$u" P="$p" python3 - <<'PY'
# import json, os
# print(json.dumps({"username": os.environ["U"], "password": os.environ["P"]}))
# PY
#   el
  if command -v jq >/dev/null 2>&1; then
    jq -n --arg u "$u" --arg p "$p" '{username:$u,password:$p}'
  else
    # Fallback (unsafe if values contain quotes)
    printf '{"username":"%s","password":"%s"}' "$u" "$p"
  fi
}

LOGIN_BODY=$(build_json "$USERNAME" "$PASSWORD")

# Perform login
LOGIN_RESP=$(curl -sS -w "\n%{http_code}" -X POST "${SERVER_URL%/}${API_V1_PREFIX}/auth/login" \
  -H 'Content-Type: application/json' \
  -d "$LOGIN_BODY")
LOGIN_HTTP=$(printf "%s" "$LOGIN_RESP" | tail -n1)
LOGIN_JSON=$(printf "%s" "$LOGIN_RESP" | sed '$d')

if [[ "$LOGIN_HTTP" != 200 ]]; then
  echo "Login failed: HTTP $LOGIN_HTTP: $LOGIN_JSON" >&2
  exit 1
fi

# Extract token using jq if available; else regex fallback
if command -v jq >/dev/null 2>&1; then
  TOKEN=$(printf "%s" "$LOGIN_JSON" | jq -r '.token')
else
  TOKEN=$(printf "%s" "$LOGIN_JSON" | sed -n 's/.*"token"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
fi

if [[ -z "${TOKEN:-}" || "$TOKEN" == "null" ]]; then
  echo "Failed to parse token from login response" >&2
  exit 1
fi

if [[ -z "${TENANT_ID:-}" ]]; then
  if command -v python3 >/dev/null 2>&1; then
    TENANT_ID=$(python3 - "$TOKEN" <<'PY'
import base64, json, sys
token = sys.argv[1]
try:
    payload = token.split('.')[1]
except IndexError:
    sys.exit(1)
padding = '=' * (-len(payload) % 4)
try:
    data = json.loads(base64.urlsafe_b64decode((payload + padding).encode()).decode())
except Exception:
    sys.exit(1)
tenant = data.get('tenant_id', '')
print(tenant)
PY
  fi
fi

if [[ -z "${TENANT_ID:-}" ]] && command -v jq >/dev/null 2>&1; then
  TENANT_ID=$(printf '%s' "$TOKEN" | jq -r 'split(".")[1] // "" | sub("-"; "+") | sub("_"; "/") | (. + ("==="[0:(4 - (length % 4)) % 4])) | @base64d | fromjson | .tenant_id // empty' || true)
fi

if [[ -z "${TENANT_ID:-}" ]]; then
  echo "Could not determine tenant_id; set TENANT_ID env var" >&2
  exit 1
fi

TENANT_SCOPE="${API_V1_PREFIX}/family/${TENANT_ID}"

REWARD_PAYLOAD='{"child_id":"alice","minutes":10}'

curl -fSs -X POST "${SERVER_URL%/}${TENANT_SCOPE}/children/alice/reward" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d "${REWARD_PAYLOAD}"
echo
