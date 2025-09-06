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

- Parents grant minutes (task-based or custom), which increase a child’s remaining balance.
- Linux client sends a heartbeat every minute; the server deduplicates timestamps per child/device and decrements remaining minutes.
- When remaining time reaches zero (or the server is unreachable for ~5 minutes), the Linux client locks the session.

