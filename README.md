## Problem statement

We need a parental control solution that will manage screen time limits for our children.

We want to reward our children for various different 

## Platforms

* Linux
* Android (future)
* Windows (future)


## Technology stack

* Rust
* Timekpr-Next
* Web browser

# Architecture

* Server that gets input from parents and tracks time
  * update logic is modular, so that new clients can be implemented
  * also serves web app
* Web app for parents
* Clients
  * Linux
  * other clients (like Android) coming in the future


## Workflow (Web)

Parent
1. Parent logs in on the web app.
2. Status page shows a grid of children with remaining minutes (auto-refreshes every 60s).
3. Parent selects a child to open Child Details.
4. Parent clicks a task (or enters custom minutes).
5. App shows a confirmation dialog; on accept it calls `/api/reward`.
6. Server adds the task minutes; the task is recorded as done for that child, and the UI updates immediately.

Child
1. Child logs in on the web app.
2. App shows only Child Details for that child (tasks visible but not clickable).

Minutes can be used in the next day if the child didn't use them.

When child uses the screen, the client sends a heartbeat every minute with a batch of minute timestamps (UTC, rounded to minute). If the connection is interrupted, the next heartbeat includes the missed minutes (bounded). The server deduplicates across devices per child and subtracts exactly one minute per unique timestamp.
When time is up or server is not available for more than 5 minutes, it logs the user off.


## Notes

First version of Linux client is just a wrapper around timekpr-next CLI.

## Auth & Access Control (MVP)

- Tokens are JWTs signed by the server. Sessions are tracked server‑side with an inactivity window.
- Roles:
  - Parent: may list children/tasks, view remaining for any child, and grant rewards. Cannot send heartbeats.
  - Child: may send heartbeats only for their own child_id, and only from the registered device_id.
- Device tokens:
  - Endpoint `POST /api/client/register` issues a child token bound to `{ child_id, device_id }`.
  - Parents can register on behalf of a child by passing `child_id` in the body; children can register for themselves without passing it.
- Heartbeat enforcement:
  - `POST /api/heartbeat` requires a child token; request `{ child_id, device_id }` must match the token claims.

## Registration Flow (CLI)

- `gamiscreen-client-linux login`:
  - Logs in as Parent or Child.
  - If Parent, prompts for `child_id` to provision; generates a `device_id` on the client and calls `/api/client/register`.
  - Stores the device token in the system keyring and writes `~/.config/gamiscreen/client.yaml` with `server_url`, `child_id`, and `device_id`.
## Web App

- Built with React + Vite + TypeScript and styled with Pico CSS.
- The Rust server embeds and serves the built assets from `gamiscreen-web/dist/`.
- Dev details and environment options are in `gamiscreen-web/README.md`.
