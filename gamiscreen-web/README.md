Gamiscreen Web (MVP)

Overview
- React + Vite + TypeScript app styled with Pico CSS.
- Parent flow: Login → Status (children grid) → Child Details → pick task → confirm → reward minutes. Custom minutes available under tasks.
- Child flow: Login → Child Details only (tasks visible, not clickable; no custom input).
- Build outputs to `dist/`, which the Rust server embeds and serves.

Prerequisites
- Node.js LTS (>=18) and npm or pnpm

Install
```
npm install
```

Develop
- Option A (CORS): ensure server config has `dev_cors_origin: "http://localhost:5173"` and start server on your port (e.g., 3000). Then set `VITE_API_BASE_URL` to the server origin.
```
VITE_API_BASE_URL=http://localhost:3000 npm run dev
```

- Option B (Proxy): enable Vite proxy to avoid CORS. Set env vars:
```
VITE_DEV_PROXY=1 VITE_API_PROXY_TARGET=http://localhost:3000 npm run dev
```

Build
```
npm run build
```
Output goes to `dist/`. The Rust server serves files from `gamiscreen-web/dist`.

Pages / Flow
- Login: obtains JWT via `/api/v1/auth/login` and stores it in `localStorage`.
  - Navigating to `#login` logs out (token cleared).
- Status (parent): lists children and shows remaining minutes; links to child details.
  - Auto-refresh every 60s.
- Child Details (parent or child): shows remaining, tasks, and optional custom minutes.
  - Tasks come from `/api/v1/family/{tenant}/children/{id}/tasks` (tenant ID comes from the JWT claims) and include `last_done` for the child.
  - If a task was completed today, a small “Done” badge appears next to its name; hover shows the last done time.
  - Clicking a task (parent only) opens a confirmation dialog and then calls `/api/v1/family/{tenant}/children/{id}/reward`.
  - After a reward, remaining updates immediately, the task is marked done for today, and reward history refreshes (page 1).
  - Reward History: collapsible section with a refresh button and pagination.
  - Auto-refresh every 60s for remaining/tasks.

Config
- `VITE_API_BASE_URL` (optional): base URL for API (e.g., `http://localhost:3000`). When omitted, calls same-origin.
- Proxy env vars (`VITE_DEV_PROXY`, `VITE_API_PROXY_TARGET`) available for local dev convenience.

Styling
- Uses Pico defaults (no custom button/input styles). Minor layout helpers remain.
