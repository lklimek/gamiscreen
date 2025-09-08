## TL;DR

Gamiscreen is a self-hosted parental control tool that turns good habits into screen time. Parents grant minutes for completed tasks; a lightweight Linux agent enforces limits when time runs out.

- Simple: grant minutes with one tap; see what’s left at a glance.
- Reliable: the Linux client enforces limits, even if the server is briefly offline.
- Private: runs on your own machine; no cloud required.

## Features

- Reward minutes for tasks or custom amounts; instant updates to remaining time.
- Parent dashboard with auto-refresh to monitor all children.
- Child view shows remaining time and today’s tasks at a glance.
- Enforced limits on Linux: screen locks when time is up or offline for >5 minutes.
- Built-in web app served by the server; no extra hosting needed.
- Auto-update: server embeds a manifest of the latest client binaries; Linux client self-updates on start.

## Platforms

- Linux client: supported today.
- Android client: planned.
- Windows client: planned.

## Get Started

- Install and run the server, then set up the Linux client on your child’s device.
- Start here: docs/INSTALL.md

## Learn More

- Architecture and technology stack: docs/ARCHITECTURE.md
- Usage and workflows (parent/child, CLI): docs/USAGE.md
- Auth and access control details: docs/AUTH.md
- Configuration (server & client): docs/CONFIGURATION.md
- Docker & reverse proxy setup: docs/DOCKER.md

## Project Layout

- `gamiscreen-server/`: Rust server exposing REST API and serving the web UI. Example config: `gamiscreen-server/config.yaml.example`
  - Public update manifest at `/api/update/manifest` (embedded at build time from GitHub Releases).
- `gamiscreen-web/`: React + TypeScript SPA. See `gamiscreen-web/README.md` for dev details.
- `gamiscreen-client/`: Linux agent and CLI (Timekpr‑Next integration).
