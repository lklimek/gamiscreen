# Architecture

## Technology Stack

- Rust for the server and clients
- React + TypeScript + Vite for the web app
- Pico CSS for styling
- Timekpr‑Next for Linux session control/locking
- SQLite for storage (migrations applied on startup)

## System Overview

- Server: handles authentication, tracks remaining/usage, applies reward logic, and serves the web assets.
  - Modular update/usage logic to support multiple client platforms.
- Web App: parent and child UI to view remaining time and grant rewards.
- Clients: Linux and Windows clients available; Android client planned.

## Data Flow (high level)

- **Balance** = total earned rewards - total borrowed rewards - total usage minutes. Can go negative through borrowing.
- **Remaining** is stored separately and tracks actual usable screen time, including borrowed minutes.
- Parents grant minutes (task-based or custom). Earning while in debt first pays off the debt before increasing remaining.
- Borrowing adds time to remaining immediately but decreases balance (creating debt).
- **Required tasks** can block screen time even with a positive remaining value. All required tasks must be completed daily (UTC) before time is unlocked.
- Clients send a heartbeat every minute; the server deduplicates timestamps per child/device and decrements remaining.
- When remaining time reaches zero, tasks are blocking, or the server is unreachable for ~5 minutes, the client locks the session.

## Platform Clients

### Linux
- Runs as a systemd user service per child account.
- Locks the session via DBus (`org.freedesktop.login1`). Requires a polkit rule for non-interactive lock.

### Windows
- A SYSTEM-level Windows Service (`GamiScreenAgent`) runs at boot and monitors session events.
- For each logged-in user, the service spawns a session agent (`gamiscreen-client session-agent`) in the user's security context using `WTSQueryUserToken` + `CreateProcessAsUserW`.
- Each session agent reads the user's token from Windows Credential Manager, sends heartbeats, shows toast notifications, and locks the workstation via `LockWorkStation` when time runs out.
- See docs/WINDOWS.md for the full architecture.

