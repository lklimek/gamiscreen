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
- Clients: Linux client available today; Android and Windows clients planned.

## Data Flow (high level)

- **Balance** = total earned rewards - total borrowed rewards - total usage minutes. Can go negative through borrowing.
- **Remaining** is stored separately and tracks actual usable screen time, including borrowed minutes.
- Parents grant minutes (task-based or custom). Earning while in debt first pays off the debt before increasing remaining.
- Borrowing adds time to remaining immediately but decreases balance (creating debt).
- **Required tasks** can block screen time even with a positive remaining value. All required tasks must be completed daily (UTC) before time is unlocked.
- Linux client sends a heartbeat every minute; the server deduplicates timestamps per child/device and decrements remaining.
- When remaining time reaches zero, tasks are blocking, or the server is unreachable for ~5 minutes, the Linux client locks the session.

