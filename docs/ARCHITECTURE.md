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

- Two independent tracking mechanisms:
  - **`minutes_remaining`** (screen clock): actual usable screen time. Affected by usage (heartbeats), penalties, borrowing (increases it), and earning (surplus after debt repayment).
  - **`account_balance`** (virtual bank, stored column): 0 = no debt, negative = debt from borrowing. Only affected by borrowing (decreases) and earning (repays debt first, surplus goes to remaining).
- Usage only affects `minutes_remaining`. Penalties only affect `minutes_remaining`. Borrowing decreases `account_balance` and increases `minutes_remaining`. Earning repays debt first (increases `account_balance` toward 0), then surplus goes to `minutes_remaining`.
- **Mandatory tasks** can block screen time even with positive remaining minutes. A task blocks only its assigned children, on its scheduled days, at or after its start time, until completed that family-local day (`timezone`, default `UTC`). The server supplies completion and blocking status to the web app. Parents manage tasks in the database through the Tasks page; deletion preserves completion and reward history.
- Clients send a heartbeat every minute; the server deduplicates timestamps per child/device and decrements remaining.
- Locked clients refresh access status every minute without recording usage. Task changes publish status for affected children, and updates track both usable minutes and task blocking so scheduled blocks are delivered even when the stored credit is unchanged.
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

