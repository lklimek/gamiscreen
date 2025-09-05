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
  - [x] `GET /api/children` → list children
  - [x] `GET /api/tasks` → list tasks
  - [x] `POST /api/reward { child_id, task_id | minutes }` → add minutes
  - [x] `POST /api/heartbeat { child_id, device_id }` → decrement balance, returns remaining minutes
  - [x] `GET /api/children/{id}/remaining` → remaining minutes
  
- [x] Auth (MVP)
  - [x] JWT login endpoint with bcrypt users (config-driven)
  - [x] Middleware validates JWT and enforces 7-day inactivity timeout
- [x] Serve static files for Web App from `/` (serve `web/dist` build output)
- [x] Enable CORS for dev from `http://localhost:5173` (Vite dev server)
- [x] Basic logging + error handling
- [x] Tests for accounting logic (rewards, decrement, carry-over)

## Web App (MVP)

- [x] React + Vite + TypeScript web app
  - [ ] Prereq: install Node.js LTS and `npm`/`pnpm`
  - [x] Scaffold in `web/` using Vite React TS template
  - [x] Dev: `npm run dev` (port 5173); API via proxy or CORS
  - [x] Build: `npm run build` → outputs to `web/dist` (served by Rust server)
  - [x] Page: "Login"
  - [x] Page: “Reward Minutes”
    - [x] Load children and tasks from `/api/children` + `/api/tasks`
    - [x] UI: select child, select task, or enter custom minutes
    - [x] Submit to `/api/reward` and show remaining minutes
  - [x] Page: “Status”
    - [x] Show per-child remaining minutes (calls `/api/children` and `/api/children/{id}/remaining`)
  - [x] API client wrapper
    - [x] Attach bearer token header when present
    - [x] Centralized error handling and JSON parsing
  - [x] Minimal styling; mobile-friendly for parent phone
  - [x] Handle auth token (simple input stored in `localStorage`)
  - [ ] Manual test checklist documented in `README.md`

## Linux Client (MVP)

- [x] Binary that runs as the child’s session agent (systemd user service)
 - [x] Config: server URL, `child_id`, `device_id`, heartbeat interval (default 60s)
 - [x] Token stored in keyring; auto-read by agent
 - [x] `login` command writes config after server registration
- [x ] Every minute: `POST /api/heartbeat` → get remaining minutes
- [ ] Enforcement
  - [x] If remaining <= 0 → trigger screen lock
  - [x] If server unreachable for >5 minutes → trigger screen lock (failsafe)

- [x] Observability: minimal logs; backoff/jitter on network failure
- [x] Packaging notes: systemd unit file example, installation steps

## User Warning & Countdown (Linux first; cross‑platform design)

- [x] Core behavior
  - [x] Add `warn_before_lock_secs` to `gamiscreen-client` config (default: 10).
  - [x] Compute seconds to next minute boundary and trigger warning when `remaining_minutes == 1` and `secs_to_next_minute <= warn_before_lock_secs`.
  - [x] Cancel/clear pending warning if a later heartbeat reports `remaining_minutes > 1` (e.g., reward added) before lock.
- [ ] Notification abstraction
  - [x] Define `NotificationBackend` trait with `show_countdown(total_secs)`, `update(seconds_left)`, and `close()`.
  - [x] Use `notify-rust` as the default implementation (Linux now; Windows later via crate support).
  - [x] Wire fallback to log-only behavior if notifications are unsupported/unavailable.
- [ ] Linux implementation (initial)
  - [x] Implement `NotificationBackend` using `notify-rust` (DBus org.freedesktop.Notifications) with replace ID to update the same toast each second; set critical urgency.
  - [x] Fallback path: if session bus not available, attempt `zenity` or skip with a warning.
  - [ ] Verify operation under systemd user service (session DBus); document any needed unit tweaks.
- [ ] Integration with locking
  - [x] During countdown, enforce screen lock when the countdown reaches zero using existing `LockBackend`.
  - [ ] Cancel countdown on early lock or process shutdown to avoid stale toasts.
- [ ] Config & docs
  - [ ] Document `warn_before_lock_secs` in `gamiscreen-client/config.example.yaml` and `docs/INSTALL.md`.
  - [ ] Add short “How it works” note in `README.md` (10s pre‑lock warning and visible countdown).
- [ ] Manual test plan
  - [ ] Set `interval_secs` to a small value (e.g., 5s) for local testing; simulate near‑zero to validate countdown and lock.
  - [ ] Grant minutes during countdown to ensure it cancels correctly.
  - [ ] Offline scenario: ensure no duplicate countdowns if the failsafe lock triggers.
- [ ] Windows (planned)
  - [ ] Reuse `notify-rust` Windows backend; document caveats (AppUserModelID/Start Menu shortcut requirements for toasts), and fall back to log-only when unavailable.
  - [ ] Parity plan for session lock integration on Windows.

## Nice-to-Haves (post-MVP)

- [ ] Multi-device coordination per child (don’t double-decrement)
- [ ] Admin UI for CRUD on children/tasks (instead of static config)
- [ ] Per-task caps and expiry windows
- [ ] Parent auth beyond shared token (e.g., local accounts)
- [ ] Android and Windows clients
- [ ] Graphs: rewards and usage history

## Acceptance Criteria (MVP)

- [ ] Parent can reward minutes via Web App; server updates child balance
- [ ] React app builds to `web/dist` and is served from Rust server
- [ ] Linux client decrements minutes every minute while running
- [ ] Client logs out the child when minutes hit zero or server is down >5 min
- [ ] Minutes carry over to next day if unused
- [ ] All components run on local network without external services
