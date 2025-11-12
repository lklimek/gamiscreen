# TODO

MVP will be shipped in three parts, in order: Server → Web App → Linux Client. The items below are grouped and ordered for that flow. Use checkboxes to track progress.

- [x] Pick web stack: `axum` + `tokio` + `serde` (simple, async, stable)
- [x] Project layout: keep single repo; add `server` module (binary entry) and `shared` module for types
- [x] Config file loader: `config.yaml` for children and tasks
  - [x] Define children (id, display_name)
  - [x] Define tasks (id, name, minutes)
- [x] SQLite storage via Diesel with embedded migrations (schema updates)
- [x] Core domain types in `shared`
  - [x] `ChildId`, `TaskId`, `Minutes` (newtype)
  - [x] `Child`, `Task`, `Reward`, `UsageTick`
- [x] Time accounting logic
  - [x] Add minutes to a child’s balance (rewards)
  - [x] Decrement on heartbeat (one minute per tick)
  - [x] Carry over unused minutes to next day (simple balance model)
- [x] HTTP API (v1)
  - [x] `GET /api/v1/family/{tenant}/children` → list children
  - [x] `GET /api/v1/family/{tenant}/tasks` → list tasks
  - [x] `POST /api/v1/family/{tenant}/children/{id}/reward { task_id | minutes }` → add minutes
  - [x] `POST /api/v1/family/{tenant}/children/{id}/device/{device_id}/heartbeat` → decrement balance, returns remaining minutes
  - [x] `GET /api/v1/family/{tenant}/children/{id}/remaining` → remaining minutes
  
- [x] Auth (MVP)
  - [x] JWT login endpoint with bcrypt users (config-driven)
  - [x] Middleware validates JWT and enforces 7-day inactivity timeout
- [x] Serve static files for Web App from `/` (serve `web/dist` build output)
- [x] Enable CORS for dev from `http://localhost:5173` (Vite dev server)
- [x] Basic logging + error handling
- [x] Tests for accounting logic (rewards, decrement, carry-over)

## Web App (MVP)

- [x] React + Vite + TypeScript web app
  - [x] Prereq: install Node.js LTS and `npm`/`pnpm`
  - [x] Scaffold in `web/` using Vite React TS template
  - [x] Dev: `npm run dev` (port 5173); API via proxy or CORS
  - [x] Build: `npm run build` → outputs to `web/dist` (served by Rust server)
  - [x] Page: "Login"
  - [x] Page: “Reward Minutes”
    - [x] Load children and tasks from `/api/v1/family/{tenant}/children` + `/api/v1/family/{tenant}/tasks`
    - [x] UI: select child, select task, or enter custom minutes
    - [x] Submit to `/api/v1/family/{tenant}/children/{id}/reward` and show remaining minutes
  - [x] Page: “Status”
    - [x] Show per-child remaining minutes (calls `/api/v1/family/{tenant}/children` and `/api/v1/family/{tenant}/children/{id}/remaining`)
  - [x] API client wrapper
    - [x] Attach bearer token header when present
    - [x] Centralized error handling and JSON parsing
  - [x] Minimal styling; mobile-friendly for parent phone
  - [x] Handle auth token (simple input stored in `localStorage`)
  - [x] Manual test checklist documented in `README.md`
  - [x] Tasks can also have negative time. In web, such tasks should be visually marked (like, it's a deduction, not a reward).

## Linux Client (MVP)

- [x] Binary that runs as the child’s session agent (systemd user service)
 - [x] Config: server URL, `child_id`, `device_id`, heartbeat interval (default 60s)
 - [x] Token stored in keyring; auto-read by agent
 - [x] `login` command writes config after server registration
- [x] Every minute: `POST /api/v1/family/{tenant}/children/{id}/device/{device_id}/heartbeat` → get remaining minutes
- [x] Enforcement
  - [x] If remaining <= 0 → trigger screen lock
  - [x] If server unreachable for >5 minutes → trigger screen lock (failsafe)
- [x] Observability: minimal logs; backoff/jitter on network failure
- [x] Packaging notes: systemd unit file example, installation steps

## User Warning & Countdown (Linux first; cross‑platform design)

- [x] Core behavior
  - [x] Add `warn_before_lock_secs` to `gamiscreen-client` config (default: 10).
  - [x] Compute seconds to next minute boundary and trigger warning when `remaining_minutes == 1` and `secs_to_next_minute <= warn_before_lock_secs`.
  - [x] Cancel/clear pending warning if a later heartbeat reports `remaining_minutes > 1` (e.g., reward added) before lock.
- [x] Notification abstraction
  - [x] Define `NotificationBackend` trait with `show_countdown(total_secs)`, `update(seconds_left)`, and `close()`.
  - [x] Use `notify-rust` as the default implementation (Linux now; Windows later via crate support).
  - [x] Wire fallback to log-only behavior if notifications are unsupported/unavailable.
- [x] Linux implementation (initial)
  - [x] Implement `NotificationBackend` using `notify-rust` (DBus org.freedesktop.Notifications) with replace ID to update the same toast each second; set critical urgency.
  - [x] Fallback path: if session bus not available, attempt `zenity` or skip with a warning.
  - [x] Verify operation under systemd user service (session DBus); document any needed unit tweaks.
- [x] Integration with locking
  - [x] During countdown, enforce screen lock when the countdown reaches zero using existing `LockBackend`.
  - [x] Cancel countdown on early lock or process shutdown to avoid stale toasts.
- [x] Config & docs
  - [x] Document `warn_before_lock_secs` in `gamiscreen-client/config.example.yaml` and `docs/INSTALL.md`.
  - [x] Add short “How it works” note in `README.md` (10s pre‑lock warning and visible countdown).

## Windows Service Migration

- [ ] CLI alignment
  - [x] Restructure clap commands into `agent`, `login`, `install`, `uninstall`, plus Windows-only `service` and `session-agent`.
  - [x] Extract the shared main loop into a reusable `app::agent::run` so both CLI paths and the service spawn reuse it.
  - [x] Update help text/docs to steer Windows admins to `service` commands and Linux users to existing flows.
- [ ] Service host
  - [x] Implement `--service` entry point that registers with SCM, handles session-change controls, and supervises per-session workers.
  - [x] Provide graceful shutdown semantics and restart/backoff for crashed workers.
- [ ] Session agent mode
  - [ ] Teach the CLI to run as `session-agent` command with IPC (named pipe/Event) back to the service.
  - [ ] Ensure keyring access stays per-user and exits cleanly when no token/config is found.
- [ ] Installer / uninstaller
  - [ ] Stage binaries under `%ProgramFiles%\GamiScreen\Client` and create data/log directories under `%ProgramData%`.
  - [ ] Register/unregister the Windows Service, add the Event Log source, and gate with admin privilege checks.
- [ ] Worker lifecycle
  - [ ] Use `WTSQueryUserToken` + `CreateProcessAsUser` to spawn session agents on logon/unlock.
  - [ ] Track `SESSION_LOGOFF`/disconnect events and terminate agents promptly.
- [ ] Logging & diagnostics
  - [ ] Emit service diagnostics to the Windows Event Log and to rotating file logs.
  - [ ] Surface missing token/config conditions with throttled warnings.
- [ ] Self-update compatibility
  - [ ] Verify the session-agent command self-update keeps the service binary intact.
  - [ ] Add smoke test covering staged update while the service is running.
- [ ] Testing & CI
  - [ ] Add Windows CI coverage (GitHub Actions runner) for service install/start/stop scripts.
  - [ ] Provide a manual QA checklist for multi-user PCs.
- [ ] Documentation
  - [ ] Sync `docs/INSTALL.md` with Windows instructions.
  - [ ] Expand `docs/WINDOWS.md` with troubleshooting guidance.

## Nice-to-Haves (post-MVP)

- [ ] Negative remaining time support
- [x] Child can submit task completions for parent's acceptance
- [ ] Multi-device coordination per child (don’t double-decrement)
- [ ] Admin UI for CRUD on children/tasks (instead of static config)
- [ ] Per-task caps and expiry windows
- [ ] Parent auth beyond shared token (e.g., local accounts)
- [ ] Android and Windows clients
- [ ] Graphs: rewards and usage history

## Web Push Integration

### Configuration & Keys

- [x] Generate VAPID key pair (`public`, `private`) and add to server configuration (`AppConfig` + env overrides).
- [x] Expose VAPID public key to gamiscreen-web build (e.g., `VITE_VAPID_PUB_KEY`).
- [x] Document secrets management for production (rotation, `.env` templates).

### Database & Models

- [x] Add Diesel migration for `push_subscriptions` table (`id`, `child_id`, `endpoint`, `p256dh`, `auth`, `created_at`, `updated_at`, `last_success_at`, `last_error`).
- [x] Extend storage layer with insert/update/delete/list helpers for push subscriptions.
- [x] Ensure cleanup of orphaned subscriptions when child is removed.

### Shared API & Types

- [x] Define `PushSubscribeReq`, `PushSubscribeResp`, `PushUnsubscribeReq` in `gamiscreen-shared` and regenerate TS bindings.
- [x] Update ACL helpers to cover new endpoints.

### Server Endpoints

- [x] Implement `POST /children/{child}/push/subscriptions` to register a subscription (parent for child or child self).
- [x] Implement `POST /children/{child}/push/subscriptions/unsubscribe` to remove a subscription.
- [x] Add validation to limit subscriptions per child and deduplicate by endpoint.

### Push Delivery Service

- [x] Introduce service module wrapping `web-push` crate with VAPID keys.
- [x] Map `ServerEvent` variants to push payloads (minimum JSON).
- [x] Integrate with existing event flow: fire push when `remaining_updated` is ≤5 minutes, and other critical events as needed.
- [x] Handle delivery results (remove subscription on 404/410, log errors, update `last_success_at`).
- [x] Add throttling/backoff to avoid repeated sends when remaining stays ≤5.

### Frontend (gamiscreen-web)

- [x] Replace current in-app notification prompt with Web Push registration flow.
- [x] Use `serviceWorker.ready.pushManager.subscribe` with VAPID public key and post subscription to server.
- [x] Handle unsubscription on logout or user action.
- [x] Update service worker to listen for `push` events and display notifications; handle `notificationclick` to reopen app.
- [x] Provide UI state to reflect subscription status/errors.
- [x] Implement client-side countdown/alarm for near-expiry time (no push required for steady minute decrements).

### Client & Compatibility

- [ ] Keep SSE for existing clients (web/tab + gamiscreen-client); ensure push is optional enhancement.
- [ ] Document limitations (requires HTTPS, browser support, user permission).

### Testing & Deployment

- [ ] Unit tests for storage methods and push payload builder.
- [ ] Integration test for subscription endpoints (auth + persistence).
- [ ] Manual scenario: enable push, trigger remaining=5, verify notification with app closed.
- [ ] Update deployment scripts to provide VAPID keys and rebuild frontend.
- [ ] Document rollback strategy (disable push via config flag if necessary).

## Android App (Native)

### Foundations

- [x] Confirm mobile product requirements (scope, rollout, managed vs unmanaged devices).
  - Clarified in `docs/ANDROID.md` (single-child devices, parent-managed rewards, side-load distribution, Hilt adoption).
- [x] Choose `minSdkVersion`/`targetSdkVersion` and define baseline device support matrix.
  - Locked to `minSdkVersion` 31 (Android 12) and `targetSdkVersion` 34; documented in `docs/ANDROID.md`.
- [x] Scaffold Android project with Kotlin + Jetpack Compose; add `:app` module to workspace.
  - Project lives under `android/` with multi-module layout (`app`, `core`, `pwaShell`, `deviceControl`).
- [x] Establish multi-module Gradle structure (`:pwaShell`, `:core`, `:deviceControl`) and shared dependency management.
  - Version catalog in `android/gradle/libs.versions.toml` centralises Compose and Kotlin dependencies.
- [x] Configure CI Gradle build, lint (Detekt/Ktlint), and unit test steps in existing pipeline.
  - Added `scripts/android_ci.sh` invoking `./gradlew lint test assembleDebug`; documented usage in `docs/ANDROID.md`.

### Phase 1 - PWA Shell

- [x] Implement single-activity Compose UI hosting a `WebView` that loads gamiscreen-web PWA.
- [x] Enable required WebView settings (JS, storage, service workers, safe browsing) and inject user agent tweaks if needed.
- [x] Persist auth/session data: hook WebView cookie store into native secure storage for tokens.
- [ ] Handle navigation (back button, deep links, external URLs) and expose error/offline UI.
- [ ] Provide `JavascriptInterface` bridge with no-op native hooks to unblock future features.
- [ ] Support file uploads and camera intents invoked from the PWA.
- [ ] Instrument crash/analytics reporting (Firebase Crashlytics + optional analytics).

### Phase 2 - Device Control

- [ ] Research and document Device Policy Manager / Device Owner requirements and enrollment flow.
- [ ] Prototype Lock Task (kiosk) mode enabling/disabling with fallback when privileges missing.
- [ ] Implement service to trigger immediate lock/unlock using `DevicePolicyManager` API.
- [ ] Schedule background workers for minute heartbeats aligned with battery optimization rules.
- [ ] Mirror desktop warning UX with native notifications/countdown overlay.
- [ ] Add failsafe to lock when server unreachable beyond configured threshold.

### Shared Logic Integration

- [ ] Compile Rust core logic for Android via `cargo-ndk` or `gradle-rust-plugin`.
- [ ] Define JNI/UniFFI bindings for accounting and countdown functions consumed by Kotlin layer.
- [ ] Move shared business rules into Rust crate; keep UI/device handling in Kotlin modules.
- [ ] Wire Kotlin workers to invoke Rust logic on background dispatcher and handle errors.

### Testing & Release

- [ ] Instrumentation tests for WebView wrapper (login flow, offline handling, deep links).
- [ ] Unit tests for Kotlin↔Rust bridge, lock state machine, and failsafe timer.
- [ ] Manual QA checklist covering device enrollment, reward sync, lock/unlock, recovery.
- [ ] Prepare Play Store internal testing track, signing keys, and release notes template.
- [ ] Document release versioning strategy aligned with other clients and backend.

## Acceptance Criteria (MVP)

- [ ] Parent can reward minutes via Web App; server updates child balance
- [ ] React app builds to `web/dist` and is served from Rust server
- [ ] Linux client decrements minutes every minute while running
- [ ] Client logs out the child when minutes hit zero or server is down >5 min
- [ ] Minutes carry over to next day if unused
- [ ] All components run on local network without external services
