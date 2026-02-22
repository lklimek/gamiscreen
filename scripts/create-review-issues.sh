#!/usr/bin/env bash
# Creates GitHub issues for unresolved HIGH and MEDIUM findings
# from the security & code review (docs/SECURITY_AND_CODE_REVIEW.md).
#
# Prerequisites: gh CLI authenticated (`gh auth login`)
# Usage: ./scripts/create-review-issues.sh [--dry-run]

set -euo pipefail

REPO="lklimek/gamiscreen"
DRY_RUN=false
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=true

create_issue() {
  local title="$1"
  local labels="$2"
  local body="$3"

  if $DRY_RUN; then
    echo "DRY RUN: would create issue: $title"
    return
  fi

  echo "Creating: $title"
  gh issue create --repo "$REPO" --title "$title" --label "$labels" --body "$body"
  echo ""
  sleep 1  # avoid rate limiting
}

# ─── HIGH SEVERITY ──────────────────────────────────────────────────────────────

create_issue \
  "[Security] Auto-update lacks cryptographic signature verification" \
  "security,priority:high" \
  "$(cat <<'BODY'
## Severity: HIGH

## Description

The client auto-update downloads binaries from GitHub Releases and verifies them using SHA-256 hash comparison. However, the SHA-256 hash file is fetched from the **same GitHub release** as the binary itself, so it only verifies download integrity (no corruption), **not authenticity**.

If the GitHub repository is compromised (stolen credentials, CI supply-chain attack), an attacker can push a malicious binary with a matching hash file. There is no GPG/Ed25519 signature verification, no code-signing certificate, and no pinned public key.

## Files
- `gamiscreen-client/src/update.rs:23-24, 282-294, 332-335`

## Impact
Remote code execution on all gamiscreen clients if the GitHub repository or release pipeline is compromised.

## Recommendation
- Implement cryptographic signature verification using a key embedded in the binary (e.g., Ed25519 via `minisign`)
- The signing key should be separate from the GitHub deploy key
- Consider a server-side manifest with signed hashes

_Source: security review finding H1_
BODY
)"

create_issue \
  "[Security] Client agent runs as user-level systemd service — child can kill it" \
  "security,priority:high" \
  "$(cat <<'BODY'
## Severity: HIGH

## Description

The agent is installed as a **user-level** systemd service under the child's UID. The child can:
- `kill -9 $(pidof gamiscreen-client)` or `systemctl --user stop gamiscreen-client`
- `systemctl --user disable gamiscreen-client` or `systemctl --user mask gamiscreen-client`
- Delete the unit file entirely

While `Restart=always` restarts after a kill, the child can permanently disable enforcement.

## Files
- `gamiscreen-client/systemd/gamiscreen-client.service`
- `gamiscreen-client/src/platform/linux/install.rs:105-131`

## Impact
A technically savvy child can bypass all screen time enforcement.

## Recommendation
- Consider a system-level systemd service (root or dedicated user) that locks via `loginctl lock-session`
- At minimum, implement server-side "heartbeat missed" alerts so parents are notified when the agent stops reporting
- Use file immutability (`chattr +i`) on the unit file as an additional barrier

_Source: security review finding H2_
BODY
)"

create_issue \
  "[Security] GitHub Actions use unpinned version tags (supply chain risk)" \
  "security,priority:high" \
  "$(cat <<'BODY'
## Severity: HIGH

## Description

Every third-party action is referenced by mutable tag (`@v4`, `@v2`, `@v1`, `@stable`) rather than pinned to a specific commit SHA. If any action's repository is compromised, arbitrary code runs in CI with `contents: write` permissions.

Examples:
```yaml
uses: actions/checkout@v4
uses: softprops/action-gh-release@v2
uses: anthropics/claude-code-action@v1
uses: dtolnay/rust-toolchain@stable
```

## Files
- All workflow files in `.github/workflows/`

## Impact
Supply chain compromise of any referenced action leads to arbitrary code execution in CI, potentially poisoning releases.

## Recommendation
Pin all actions to full commit SHAs:
```yaml
uses: actions/checkout@b4ffde65f46336ab88eb53be808477a3936bae11  # v4.1.1
```

_Source: security review finding H3_
BODY
)"

# ─── MEDIUM SEVERITY ────────────────────────────────────────────────────────────

create_issue \
  "[Security] SSE token passed in URL query parameter (token leakage)" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

The SSE endpoint authenticates via `?token=<JWT>` in the URL. Tokens in URLs can leak through server logs, proxy logs, and browser history. The `Referrer-Policy: no-referrer` header mitigates referrer leakage but not log leakage.

The Rust client uses `reqwest` which supports custom headers, so the query parameter is unnecessary for the native client.

## Files
- `gamiscreen-server/src/server/mod.rs:770-784`
- `gamiscreen-web/src/App.tsx:231`
- `gamiscreen-client/src/sse.rs:116-129`

## Recommendation
- For the Rust client: switch to `Authorization: Bearer` header
- For the web client: use a short-lived, single-purpose SSE ticket instead of the main JWT
- Document the trade-off for the EventSource API limitation

_Source: security review finding M2_
BODY
)"

create_issue \
  "[Security] SSE authentication bypasses session store validation" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

The SSE handler verifies the JWT signature but does **not** check the session store or idle timeout:

```rust
// NOTE: SSE auth does not consult or touch the sessions table
let claims = jwt::decode_and_verify(&q.token, state.config.jwt_secret.as_bytes())
```

A revoked session or idle-timed-out session can still be used for SSE until JWT expiry (up to 30-60 days).

## Files
- `gamiscreen-server/src/server/mod.rs:775-784`

## Recommendation
Validate the SSE token against the session store at connection time.

_Source: security review finding M3_
BODY
)"

create_issue \
  "[Security] Push subscription endpoint lacks URL validation (potential SSRF)" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

An authenticated user can register a push subscription with any URL. The server will then make HTTP requests to that URL via web-push. This could be used to target internal network services (SSRF).

## Files
- `gamiscreen-server/src/server/mod.rs:545-603`
- `gamiscreen-server/src/server/push.rs:135-205`

## Recommendation
- Validate push endpoint URLs: require HTTPS scheme
- Reject private/loopback IP addresses
- Optionally restrict to known push service domains (fcm.googleapis.com, updates.push.services.mozilla.com, etc.)

_Source: security review finding M4_
BODY
)"

create_issue \
  "[Security] No Content-Security-Policy header" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

Despite comprehensive security headers (X-Content-Type-Options, X-Frame-Options, HSTS, etc.), there is no Content-Security-Policy header. CSP is the strongest defense-in-depth against XSS, especially important since JWT tokens are stored in localStorage.

## Files
- `gamiscreen-server/src/server/mod.rs:292-348`
- `gamiscreen-web/index.html`

## Recommendation
Add a CSP header starting with:
```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' https:; img-src 'self' data:;
```

_Source: security review finding M5_
BODY
)"

create_issue \
  "[Security] JWT token stored in localStorage (XSS exfiltration risk)" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

JWT tokens are stored in `localStorage`, which is accessible to any JavaScript on the same origin. Any XSS vulnerability would allow token exfiltration.

**Mitigating factors:** No `dangerouslySetInnerHTML`, minimal dependencies, security headers present. On Android, tokens use EncryptedSharedPreferences instead.

## Files
- `gamiscreen-web/src/api.ts:79-103`

## Recommendation
Adding CSP (see separate issue) is the primary mitigation. Consider httpOnly cookies if architecture allows.

_Source: security review finding M6_
BODY
)"

create_issue \
  "[Security] Missing explicit request body size limit" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

No explicit `DefaultBodyLimit` is configured on the Axum router. While Axum defaults to 2MB, which is reasonable, it is implicit. For an API that only handles small JSON payloads, a tighter limit (64-256KB) would reduce attack surface.

## Files
- `gamiscreen-server/src/server/mod.rs:127-258`

## Recommendation
Add `DefaultBodyLimit::max(256 * 1024)` to the router.

_Source: security review finding M7_
BODY
)"

create_issue \
  "[Security] Unbounded heartbeat minutes array (DoS vector)" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

`HeartbeatReq` accepts `Vec<i64>` for the `minutes` field with no upper bound on array length. A malicious client could send millions of entries, causing excessive memory allocation and database lock time (each minute is inserted individually).

## Files
- `gamiscreen-shared/src/api/mod.rs:84`
- `gamiscreen-server/src/server/mod.rs:955-978`

## Recommendation
Reject requests where `body.minutes.len() > 1440` (minutes in a day).

_Source: security review finding M8_
BODY
)"

create_issue \
  "[Security] Claude workflow triggerable by any commenter" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

Any user who can comment on issues/PRs can trigger the Claude workflow by mentioning `@claude`. This workflow has `contents: write` and `pull-requests: write` permissions.

## Files
- `.github/workflows/claude.yml:16-20`

## Recommendation
Add an actor association check to restrict the trigger to repository collaborators only:
```yaml
if: |
  (github.event.issue.pull_request && contains(github.event.comment.body, '@claude')) &&
  github.event.comment.author_association in ['OWNER', 'MEMBER', 'COLLABORATOR']
```

_Source: security review finding M9_
BODY
)"

create_issue \
  "[Security] .gitignore does not exclude .env files" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

The `.gitignore` file does not include patterns for `.env` or `.env.*` files. This could lead to accidental commits of environment variables containing secrets.

## Files
- `.gitignore`

## Recommendation
Add the following patterns:
```
.env
.env.*
!.env.example
*.pem
*.key
```

_Source: security review finding M11_
BODY
)"

create_issue \
  "[Security] JWT secret generation uses UUID v4 instead of 256-bit CSPRNG" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

The JWT secret is generated using UUID v4 which provides 122 bits of randomness. While above brute-force threshold, a 256-bit CSPRNG-generated secret is best practice for HMAC-SHA256 JWT signing.

## Files
- `gamiscreen-server/src/install.rs:10-13`

## Recommendation
Use `rand::thread_rng().gen::<[u8; 32]>()` encoded as hex or base64 for the JWT secret.

_Source: security review finding M12_
BODY
)"

create_issue \
  "[Security] No JWT secret strength validation at startup" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

There is no minimum length or entropy check on the configured `jwt_secret`. A user could accidentally set `jwt_secret: "abc"` and run with a trivially brute-forceable signing key.

## Files
- `gamiscreen-server/src/server/config.rs:17`

## Recommendation
Warn or refuse to start if `jwt_secret` is shorter than 32 characters.

_Source: security review finding M13_
BODY
)"

create_issue \
  "[Security] Pending minutes log file has no integrity protection" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

The pending-minutes log is stored as plaintext in the child's data directory (`~/.local/share/gamiscreen/`). The child can delete or edit this file to erase unsent usage minutes, effectively hiding screen time usage from the parent dashboard.

## Files
- `gamiscreen-client/src/app/agent.rs:286-370`

## Recommendation
Implement server-side anomaly detection for gaps in minute sequences. Additionally, consider HMAC signing of the pending log file.

_Source: security review finding M14_
BODY
)"

create_issue \
  "[Enhancement] Reduce 60-second relock delay window" \
  "enhancement,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

After the initial lock, a child who immediately unlocks gets a 60-second grace period before relock. Through repeated unlock cycles, the child can accumulate ~3.5 minutes of unauthorized use (60+50+40+30+20+10 seconds with the current exponential backoff).

## Files
- `gamiscreen-client/src/app/agent.rs:17-19, 613-648, 703-709`

## Recommendation
- Reduce `RELOCK_INITIAL_DELAY_SECS` from 60 to 5-10 seconds
- Consider notifying parents after repeated unlock cycles

_Source: security review finding M15_
BODY
)"

create_issue \
  "[Code Quality] Reduce excessive boilerplate in storage layer" \
  "code-quality,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

Every method in the storage layer repeats an identical pattern:
1. Clone pool
2. Clone/own parameters
3. `tokio::task::spawn_blocking`
4. Get connection from pool
5. Call `configure_sqlite_conn`
6. Execute Diesel query
7. Map errors
8. Flatten JoinError

This pattern is repeated ~30 times across 800+ lines.

## Files
- `gamiscreen-server/src/storage/mod.rs`

## Recommendation
Create a helper method:
```rust
async fn with_conn<F, T>(&self, f: F) -> Result<T, StorageError>
where
    F: FnOnce(&mut SqliteConnection) -> Result<T, StorageError> + Send + 'static,
    T: Send + 'static,
{
    let pool = self.pool.clone();
    tokio::task::spawn_blocking(move || {
        let mut conn = pool.get()?;
        f(&mut conn)
    }).await?
}
```

_Source: security review finding M16_
BODY
)"

create_issue \
  "[Performance] SQLite PRAGMAs called on every database operation" \
  "performance,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

`configure_sqlite_conn()` runs 3 PRAGMA statements (`journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=5000`) on every pool checkout. These PRAGMAs persist for the lifetime of the connection (and `journal_mode=WAL` persists at the database level), so calling them on every operation is unnecessary overhead — 3 extra round-trips to SQLite per database call.

## Files
- `gamiscreen-server/src/storage/mod.rs:822-829` (function definition)
- Called in every storage method

## Recommendation
Use r2d2's `CustomizeConnection` trait to set PRAGMAs once when a connection is first created:
```rust
impl CustomizeConnection<SqliteConnection, r2d2::Error> for SqliteConnectionCustomizer {
    fn on_acquire(&self, conn: &mut SqliteConnection) -> Result<(), r2d2::Error> {
        configure_sqlite_conn(conn).map_err(|e| r2d2::Error::QueryError(e))?;
        Ok(())
    }
}
```

_Source: security review finding M17_
BODY
)"

create_issue \
  "[Testing] Add unit tests for storage layer and push notification logic" \
  "testing,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

The storage module (800+ lines) and push notification deduplication logic have zero unit tests. Edge cases like `compute_remaining` with no rewards, overflow scenarios, and `should_push_remaining` threshold crossings are untested.

The integration tests in `tests/integration_scenario.rs` exercise storage indirectly through the HTTP API but don't cover these edge cases.

## Files
- `gamiscreen-server/src/storage/mod.rs`
- `gamiscreen-server/src/server/push.rs`

## Recommendation
- Add unit tests for `compute_remaining` (no rewards, large values, boundary conditions)
- Add unit tests for `should_push_remaining` (deduplication logic, 5-minute threshold crossings)
- Add unit tests for storage methods with in-memory SQLite

_Source: security review finding M20_
BODY
)"

create_issue \
  "[Security] Server base URL in web frontend stored without validation" \
  "security,priority:medium" \
  "$(cat <<'BODY'
## Severity: MEDIUM

## Description

The server base URL stored in localStorage has no validation that it uses HTTPS or points to a legitimate server. An XSS attacker could redirect all API calls to a malicious server by modifying this value.

## Files
- `gamiscreen-web/src/api.ts:120-135`

## Recommendation
Validate the URL against a pattern: must be `https://` or `http://localhost` / `http://127.0.0.1`.

_Source: security review finding M21_
BODY
)"

echo ""
echo "Done! All issues created."
