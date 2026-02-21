# Gamiscreen -- Comprehensive Security & Code Review

**Date:** 2026-02-21
**Scope:** Full codebase review covering all Rust crates, web frontend, Android app, CI/CD, Docker, and configuration.

---

## Executive Summary

Gamiscreen demonstrates **above-average security posture** for a self-hosted application. The architecture reflects deliberate security-conscious design: layered auth/ACL middleware with default-deny, Diesel ORM preventing SQL injection, RustEmbed for static files eliminating path traversal, comprehensive HTTP security headers, bcrypt password hashing, and TLS via `rustls`.

No **critical** vulnerabilities were found that would allow immediate remote exploitation. The most significant findings relate to supply-chain risks in the auto-update mechanism and CI/CD pipeline, the inherent limitation of a user-level systemd service for enforcement, and missing defense-in-depth measures (CSP headers, rate limiting).

### Finding Distribution

| Severity | Count |
|----------|-------|
| HIGH     | 5     |
| MEDIUM   | 21    |
| LOW      | 18    |
| INFO     | 20+   |

---

## Table of Contents

1. [HIGH Severity Findings](#1-high-severity-findings)
2. [MEDIUM Severity Findings](#2-medium-severity-findings)
3. [LOW Severity Findings](#3-low-severity-findings)
4. [Positive Security Observations](#4-positive-security-observations)
5. [Prioritized Recommendations](#5-prioritized-recommendations)

---

## 1. HIGH Severity Findings

### H1. Auto-Update Lacks Cryptographic Signature Verification

**Files:** `gamiscreen-client/src/update.rs:23-24, 282-294, 332-335`

The client auto-update downloads binaries from GitHub Releases and verifies them using SHA-256. However, the SHA-256 hash file is fetched from the **same GitHub release** as the binary. This only verifies download integrity (no corruption), **not authenticity**. If the GitHub repository is compromised (stolen credentials, CI supply-chain attack), an attacker can push a malicious binary with a matching hash.

There is no GPG/Ed25519 signature verification, no code-signing certificate, and no pinned public key. The binary is then `exec()`'d directly.

**Impact:** Remote code execution on all gamiscreen clients if the GitHub repository is compromised.

**Recommendation:**
- Implement cryptographic signature verification using a key embedded in the binary (e.g., Ed25519 via `minisign`)
- The signing key should be separate from the GitHub deploy key
- Consider a server-side manifest with signed hashes

### H2. Client Agent Runs as User-Level systemd Service -- Child Can Kill It

**Files:** `gamiscreen-client/systemd/gamiscreen-client.service`, `gamiscreen-client/src/platform/linux/install.rs:105-131`

The agent is installed as a **user-level** systemd service under the child's UID. The child can:
- `kill -9 $(pidof gamiscreen-client)` or `systemctl --user stop gamiscreen-client`
- `systemctl --user disable gamiscreen-client` or `systemctl --user mask gamiscreen-client`
- Delete the unit file entirely

While `Restart=always` restarts after a kill, the child can permanently disable enforcement.

**Impact:** A technically savvy child can bypass all screen time enforcement.

**Recommendation:**
- Consider a system-level systemd service (root or dedicated user) that locks via `loginctl lock-session`
- At minimum, implement server-side "heartbeat missed" alerts so parents are notified when the agent stops reporting
- Use file immutability (`chattr +i`) on the unit file as an additional barrier

### H3. GitHub Actions Use Unpinned Version Tags (Supply Chain Risk)

**Files:** All workflow files in `.github/workflows/`

Every third-party action is referenced by mutable tag (`@v4`, `@v2`, `@v1`, `@stable`) rather than pinned to a specific commit SHA. If any action's repository is compromised, arbitrary code runs in CI with `contents: write` permissions.

Examples:
```yaml
uses: actions/checkout@v4
uses: softprops/action-gh-release@v2
uses: anthropics/claude-code-action@v1
uses: dtolnay/rust-toolchain@stable
```

**Impact:** Supply chain compromise of any referenced action leads to arbitrary code execution in CI, potentially poisoning releases.

**Recommendation:** Pin all actions to full commit SHAs:
```yaml
uses: actions/checkout@b4ffde65f46336ab88eb53be808477a3936bae11  # v4.1.1
```

### H4. Android WebView Allows Mixed HTTP/HTTPS Content

**File:** `android/pwaShell/src/androidMain/kotlin/ws/klimek/gamiscreen/pwashell/PwaShellHost.kt:314`

```kotlin
mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
```

This permissive setting allows insecure HTTP sub-resources within HTTPS pages, enabling man-in-the-middle injection of malicious content.

**Impact:** An attacker on the network path can inject content into the Android app.

**Recommendation:** Use `WebSettings.MIXED_CONTENT_NEVER_ALLOW`.

### H5. Storage Layer Uses `Result<T, String>` -- Losing All Error Context

**File:** `gamiscreen-server/src/storage/mod.rs` (all ~30 public methods)

Every method returns `Result<T, String>`, flattening Diesel errors, pool errors, and migration errors to plain strings via `.map_err(|e| e.to_string())`. This makes it impossible to distinguish error types for retry logic, constraint violation handling, or proper HTTP status code mapping.

**Impact:** Degraded error handling quality across the entire server. Cannot differentiate between transient errors (retry-worthy) and permanent errors.

**Recommendation:** Define a `StorageError` enum with `thiserror`:
```rust
#[derive(Debug, Error)]
enum StorageError {
    #[error("pool: {0}")] Pool(#[from] r2d2::Error),
    #[error("diesel: {0}")] Diesel(#[from] diesel::result::Error),
    #[error("join: {0}")] Join(#[from] tokio::task::JoinError),
}
```

---

## 2. MEDIUM Severity Findings

### M1. No Rate Limiting on Login Endpoint

**File:** `gamiscreen-server/src/server/mod.rs:1024-1060`

The `/api/v1/auth/login` endpoint has no rate limiting. While bcrypt slows each attempt (~100-250ms), an attacker can still try thousands of passwords per hour and cause CPU-based DoS.

**Recommendation:** Add rate limiting via `tower-governor` or a simple in-memory counter (e.g., max 10 attempts/minute/IP).

### M2. SSE Token in URL Query Parameter (Token Leakage)

**Files:** `gamiscreen-server/src/server/mod.rs:770-784`, `gamiscreen-web/src/App.tsx:231`, `gamiscreen-client/src/sse.rs:116-129`

The SSE endpoint authenticates via `?token=<JWT>` in the URL. Tokens in URLs leak through server logs, proxy logs, and browser history. The `Referrer-Policy: no-referrer` header mitigates referrer leakage but not log leakage.

**Note:** The Rust client uses `reqwest` which supports custom headers -- the query parameter is unnecessary for the native client.

**Recommendation:**
- For the Rust client: switch to `Authorization: Bearer` header
- For the web client: use a short-lived, single-purpose SSE ticket instead of the main JWT
- Document the trade-off for the EventSource API limitation

### M3. SSE Authentication Bypasses Session Store Validation

**File:** `gamiscreen-server/src/server/mod.rs:775-784`

The SSE handler verifies the JWT signature but does **not** check the session store or idle timeout:
```rust
// NOTE: SSE auth does not consult or touch the sessions table
let claims = jwt::decode_and_verify(&q.token, state.config.jwt_secret.as_bytes())
```

A revoked session or idle-timed-out session can still be used for SSE until JWT expiry (up to 30-60 days).

**Recommendation:** Validate the SSE token against the session store at connection time.

### M4. Push Subscription Endpoint Lacks URL Validation (Potential SSRF)

**Files:** `gamiscreen-server/src/server/mod.rs:545-603`, `gamiscreen-server/src/server/push.rs:135-205`

An authenticated user can register a push subscription with any URL. The server will then make HTTP requests to that URL. This could target internal network services.

**Recommendation:** Validate push endpoint URLs: require HTTPS, reject private/loopback IPs, optionally restrict to known push service domains.

### M5. No Content-Security-Policy Header

**Files:** `gamiscreen-server/src/server/mod.rs:292-348`, `gamiscreen-web/index.html`

Despite comprehensive security headers (X-Content-Type-Options, X-Frame-Options, HSTS, etc.), there is no CSP. This is the strongest defense-in-depth against XSS, especially important since JWT tokens are stored in localStorage.

**Recommendation:** Add CSP starting with:
```
default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self' https:; img-src 'self' data:;
```

### M6. JWT Token Stored in localStorage

**File:** `gamiscreen-web/src/api.ts:79-103`

JWT in localStorage is accessible to any JavaScript on the same origin. Any XSS would allow token exfiltration.

**Mitigating factors:** No `dangerouslySetInnerHTML`, minimal dependencies, security headers present. On Android, tokens use EncryptedSharedPreferences instead.

**Recommendation:** Adding CSP (M5) is the primary mitigation. Consider httpOnly cookies if architecture allows.

### M7. Missing Explicit Request Body Size Limit

**File:** `gamiscreen-server/src/server/mod.rs:127-258`

No explicit `DefaultBodyLimit` is configured. Axum defaults to 2MB, which is reasonable but implicit. For an API with small JSON payloads, a tighter limit (64-256KB) would reduce attack surface.

### M8. Unbounded Heartbeat Minutes Array (DoS Vector)

**Files:** `gamiscreen-shared/src/api/mod.rs:84`, `gamiscreen-server/src/server/mod.rs:955-978`

`HeartbeatReq` accepts `Vec<i64>` with no upper bound. A malicious client could send millions of entries, causing excessive memory allocation and database lock time.

**Recommendation:** Reject `body.minutes.len() > 1440` (minutes in a day).

### M9. `claude.yml` Workflow Triggerable by Any Commenter

**File:** `.github/workflows/claude.yml:16-20`

Any user who can comment on issues/PRs can trigger the Claude workflow by mentioning `@claude`, which has `contents: write` and `pull-requests: write` permissions.

**Recommendation:** Add an actor check to restrict to repository collaborators only.

### M10. Overly Broad CI Workflow Permissions

**Files:** `.github/workflows/android-apk.yml:28`, `.github/workflows/claude.yml:24-27`

Several workflows declare broad `contents: write` at the workflow level instead of the job level.

**Recommendation:** Follow least privilege -- grant permissions at the job level only.

### M11. `.gitignore` Does Not Exclude `.env` Files

**File:** `.gitignore`

The documentation recommends `.env` templates per environment, but `.env` and `.env.*` are not gitignored.

**Recommendation:** Add `.env`, `.env.*`, `!.env.example`, `*.pem`, `*.key` patterns.

### M12. JWT Secret Generation Uses UUID v4

**File:** `gamiscreen-server/src/install.rs:10-13`

UUID v4 provides 122 bits of randomness. While above brute-force threshold, a 256-bit CSPRNG-generated secret is best practice for JWT signing.

### M13. No JWT Secret Strength Validation at Startup

**File:** `gamiscreen-server/src/server/config.rs:17`

No minimum length or entropy check on `jwt_secret`. A user could set `jwt_secret: "abc"`.

**Recommendation:** Warn or refuse to start if `jwt_secret` is shorter than 32 characters.

### M14. Pending Minutes Log File Has No Integrity Protection

**File:** `gamiscreen-client/src/app/agent.rs:286-370`

The pending-minutes log is stored as plaintext in the child's data directory (`~/.local/share/gamiscreen/`). The child can delete or edit it to erase unsent usage minutes.

**Recommendation:** Server-side anomaly detection for gaps in minute sequences.

### M15. 60-Second Relock Delay Window

**File:** `gamiscreen-client/src/app/agent.rs:17-19, 613-648, 703-709`

After the initial lock, a child who immediately unlocks gets a 60-second grace period before relock. Repeated unlock cycles yield ~3.5 minutes of unauthorized use (60+50+40+30+20+10 seconds).

**Recommendation:** Reduce `RELOCK_INITIAL_DELAY_SECS` to 5-10 seconds. Consider notifying parents after repeated unlock cycles.

### M16. Excessive Boilerplate in Storage Layer

**File:** `gamiscreen-server/src/storage/mod.rs` (875 lines)

Every method repeats: clone pool, spawn_blocking, get connection, configure_sqlite, execute, map errors. A `with_conn` helper would eliminate ~60% of the boilerplate.

### M17. SQLite PRAGMAs Called on Every Database Operation

**File:** `gamiscreen-server/src/storage/mod.rs:867-874`

`configure_sqlite_conn()` runs 3 PRAGMA statements on every pool checkout. These persist for the connection lifetime.

**Recommendation:** Use r2d2's `CustomizeConnection::on_acquire` to set PRAGMAs once per connection.

### M18. Integer Overflow in `compute_remaining`: i64 to i32 Cast

**File:** `gamiscreen-server/src/storage/mod.rs:773`

```rust
let remaining = (rewards_sum.unwrap_or(0) - used) as i32;
```

Silent truncation if the value exceeds `i32::MAX`.

**Recommendation:** Use `i32::try_from(...).unwrap_or(i32::MAX)`.

### M19. Fire-and-Forget Push Notification Tasks

**File:** `gamiscreen-server/src/server/push.rs:75-80`

`tokio::spawn` handles are discarded. Panics are silently lost, graceful shutdown can't wait for in-flight deliveries, and unlimited tasks can be spawned.

**Recommendation:** Track in a `JoinSet` or use a bounded semaphore.

### M20. No Unit Tests for Storage Layer or Push Logic

**Files:** `gamiscreen-server/src/storage/mod.rs`, `gamiscreen-server/src/server/push.rs`

The storage module (875 lines) and push deduplication logic have zero unit tests. Edge cases like `compute_remaining` with no rewards, overflow scenarios, and `should_push_remaining` thresholds are untested.

### M21. Server Base URL Stored in localStorage Without Validation

**File:** `gamiscreen-web/src/api.ts:120-135`

No validation that the URL is HTTPS or points to a legitimate server. An XSS attacker could redirect all API calls to a malicious server.

**Recommendation:** Validate against a URL pattern (must be `https://` or `http://localhost`).

---

## 3. LOW Severity Findings

| # | Finding | File |
|---|---------|------|
| L1 | Hardcoded CORS origin `gamiscreen.klimek.ws` | `server/mod.rs:243` |
| L2 | No session garbage collection for expired sessions | `storage/mod.rs` |
| L3 | Unpaginated list endpoints could grow unbounded | `storage/mod.rs` |
| L4 | Broadcast channel capacity=64, silently drops lagged events | `server/mod.rs:49` |
| L5 | Client-supplied `x-request-id` not length-bounded | `server/mod.rs:275-280` |
| L6 | `serde_yaml` dependency is officially deprecated | `Cargo.toml` (2 crates) |
| L7 | Docker Compose missing security hardening (no-new-privileges, cap_drop) | `docker-compose.yml` |
| L8 | Docker Compose binds port on all interfaces | `docker-compose.yml:7` |
| L9 | `.dockerignore` missing `.env` exclusions | `.dockerignore` |
| L10 | Example config ships with known example passwords | `config.yaml.example:11-22` |
| L11 | Config file written without restrictive permissions (0644) | `client/src/config.rs:39-47` |
| L12 | Predictable temp file path during polkit rule install | `client/platform/linux/install.rs:167-173` |
| L13 | Windows update batch script has fragile quoting | `client/platform/windows/mod.rs:68-128` |
| L14 | Token in keyring accessible to child user | `client/src/lib.rs:35-39` |
| L15 | No Android certificate pinning | `android/app/src/main/AndroidManifest.xml` |
| L16 | SW caches all GET responses without scope restriction | `gamiscreen-web/public/sw.js:54-61` |
| L17 | Silent error swallowing in web frontend (20+ empty catch blocks) | Multiple TS files |
| L18 | Server `mod.rs` is 1185 lines; should be split into handler modules | `server/mod.rs` |

---

## 4. Positive Security Observations

These are things done **well** that should be maintained:

| Area | Detail |
|------|--------|
| **SQL Injection** | All queries use Diesel ORM with parameterized queries. No raw SQL with interpolation. |
| **Path Traversal** | Static files served via `RustEmbed` (compile-time embedded), no filesystem access at runtime. |
| **ACL Design** | Default-deny pattern. Children strictly scoped to own data. URL percent-encoding handled. |
| **Session Management** | Server-side session tracking with JTI. Atomic `touch_session_with_cutoff`. Idle + hard TTL. |
| **JWT Validation** | HS256 only (no algorithm confusion). `exp` validated by default. Tenant isolation checked. |
| **Password Storage** | bcrypt at cost 12. Passwords never logged. |
| **Error Information** | Internal errors not leaked to clients ("internal server error" only). |
| **Security Headers** | X-Content-Type-Options, X-Frame-Options, HSTS, Referrer-Policy, Permissions-Policy, COOP, CORP. |
| **TLS** | Uses `rustls` (pure-Rust). No `danger_accept_invalid_certs`. HSTS with includeSubDomains. |
| **Credential Storage** | OS keyring on desktop, EncryptedSharedPreferences on Android. |
| **Minimal Dependencies** | Web frontend: only React + PicoCSS. Small attack surface. |
| **No XSS Vectors** | No `dangerouslySetInnerHTML`, `innerHTML`, `eval()`, or `document.write()` in the web app. |
| **CORS** | Disabled by default. When enabled, uses explicit allowlist, no wildcards. |
| **Integration Tests** | Auth bypass, ACL enforcement, session idle timeout, and token renewal are all tested. |
| **Fail-Closed Design** | Client locks screen when server is unreachable for >5 min (correct posture). |
| **Auto-Update Integrity** | SHA-256 with constant-time comparison prevents corruption and timing attacks. |
| **Android Security** | WebView debugging restricted to debug builds. Backup disabled. Minimal permissions (INTERNET only). |

---

## 5. Prioritized Recommendations

### Tier 1 -- Address Soon (HIGH impact, reasonable effort)

1. **Pin GitHub Actions to commit SHAs** (H3) -- Prevents CI supply chain attacks. Low effort, high impact.
2. **Fix Android mixed content mode** (H4) -- One-line change: `MIXED_CONTENT_NEVER_ALLOW`.
3. **Add Content-Security-Policy** (M5) -- Single highest-impact defense-in-depth measure for XSS.
4. **Add rate limiting on login** (M1) -- Prevents brute force and CPU DoS.
5. **Cap heartbeat array size** (M8) -- Simple bounds check prevents DoS.
6. **Restrict `claude.yml` trigger** (M9) -- Add actor/association check.

### Tier 2 -- Address in Near Term (MEDIUM impact)

7. **Use short-lived SSE tickets** (M2/M3) -- Reduces token leakage risk and enables session validation on SSE.
8. **Validate push subscription URLs** (M4) -- Prevent SSRF.
9. **Add `.env` to `.gitignore`** (M11) -- Prevents accidental secret commits.
10. **Add JWT secret minimum length check** (M13) -- Startup validation.
11. **Reduce relock delay** (M15) -- Change `RELOCK_INITIAL_DELAY_SECS` from 60 to 5-10.
12. **Validate server base URL in web frontend** (M21) -- URL pattern check.
13. **Move SQLite PRAGMAs to connection pool setup** (M17) -- Performance improvement.
14. **Fix i64->i32 cast in `compute_remaining`** (M18) -- Use `try_from`.

### Tier 3 -- Address When Convenient (code quality, LOW items)

15. **Introduce `StorageError` enum** (H5/M16) -- Major code quality improvement.
16. **Add unit tests for storage and push logic** (M20)
17. **Implement auto-update signature verification** (H1) -- Important but requires infrastructure (signing key management).
18. **Investigate system-level service for enforcement** (H2) -- Architectural change, significant effort.
19. **Migrate from deprecated `serde_yaml`** (L6)
20. **Harden Docker Compose** (L7) -- Add security options.
21. **Split server `mod.rs` into handler modules** (L18)

---

*Review performed by a team of 6 specialized agents covering: Auth/JWT/ACL, Server API, Client Security, Web Frontend, Infrastructure/Config, and Rust Code Quality.*
