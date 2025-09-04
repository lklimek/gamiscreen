## Overview

Parental control tool for managing screen time and rewarding kids with additional minutes for completing tasks.

## Supported Features

- Authentication: parent and child roles via JWT.
- Remaining time tracking with per-minute deduction from client heartbeats.
- Parent dashboard: grid of children with remaining minutes; manual refresh and auto-refresh every 60s.
- Child details:
  - Remaining minutes with refresh.
  - Tasks list with per-task minutes; “Done” indicator when completed today.
  - Reward minutes: choose a task or enter custom minutes; confirmation dialog before submitting.
  - Reward history: paginated list with refresh button; collapsible section.
  - After rewarding, remaining updates immediately and reward history refreshes (page 1).
- Linux client: sends per-minute usage heartbeats and enforces time limits (logs out/locks when time is up or offline >5m).
- Server embeds and serves the built web app assets.

## Platforms

- Linux (client implemented)
- Android (planned)
- Windows (planned)

## Technology Stack

- Rust (server and clients)
- React + TypeScript + Vite (web app)
- Pico CSS (styling)
- Timekpr‑Next (Linux session control)

## Architecture

- Server: handles auth, tracks remaining/usage, reward logic, serves web assets.
  - Modular update logic to allow additional clients.
- Web app: parent/child UI to view remaining time and grant rewards.
- Clients: Linux client today; other clients (e.g., Android) planned.

## Web Workflow

Parent
1. Login on the web app.
2. Status page shows children with remaining minutes (auto‑refresh 60s; manual refresh available).
3. Open a child’s details.
4. Click a task or enter custom minutes.
5. Confirmation dialog → POST `/api/children/{id}/reward`.
6. Remaining updates and the task is marked done for today; reward history refreshes.

Child
1. Login on the web app.
2. Child Details only (tasks visible, not clickable; no custom input).

Minutes can carry over if not used in a day.

The Linux client sends a heartbeat every minute with UTC minute timestamps. Missed minutes (bounded) are sent on reconnection. The server deduplicates by child and minute across devices and subtracts one minute per unique timestamp. If time is up or the server is unreachable for >5 minutes, the session is locked/logged out.

## Auth & Access Control (MVP)

- JWTs signed by the server; sessions tracked server‑side with inactivity window.
- Roles:
  - Parent: list children/tasks, view remaining, grant rewards.
  - Child: send heartbeats for their own `child_id` and registered `device_id` only.
- Device registration:
  - `POST /api/client/register` issues a child token bound to `{ child_id, device_id }`.
  - Parents can register on behalf of a child by passing `child_id`; children can self‑register without it.
- Heartbeat enforcement:
  - `POST /api/heartbeat` requires a child token; body `{ child_id, device_id }` must match token claims.

## Linux Client Registration (CLI)

- `gamiscreen-client login`:
  - Logs in as Parent or Child.
  - If Parent, prompts for `child_id` to provision; generates a `device_id` and calls `/api/client/register`.
  - Stores device token in the system keyring and writes `~/.config/gamiscreen/client.yaml` with `server_url`, `child_id`, and `device_id`.

## Sub‑Crates and Links

- Server: [gamiscreen-server/](gamiscreen-server/) — Rust server that exposes REST endpoints, tracks usage, and serves the web UI. Example config: [`gamiscreen-server/config.yaml.example`](gamiscreen-server/config.yaml.example)
- Web: [gamiscreen-web/](gamiscreen-web/) — React + TypeScript SPA. See [`gamiscreen-web/README.md`](gamiscreen-web/README.md) for dev and build instructions.
- Linux client: [gamiscreen-client/](gamiscreen-client/) — CLI and daemon for Linux session control (Timekpr‑Next integration).

## Installation

- See docs/INSTALL.md for server and Linux client installation and quickstart.

## Web App

- Built with React + Vite + TypeScript and styled with Pico CSS.
- The Rust server embeds and serves the built assets from `gamiscreen-web/dist/`.
- Dev details and environment options are in `gamiscreen-web/README.md`.
