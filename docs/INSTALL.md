# Installation

The project consists of a Rust server, a React web UI (built and embedded into the server), and a Linux client. SQLite is used for storage, with migrations applied automatically on startup.

## Prerequisites

- Rust toolchain (stable) and Cargo
- Node.js (>=18) and npm (only required to build the web UI; can be skipped with `SKIP_WEB_BUILD=1`)
- Linux for the client (systemd optional for service management)

## Server

Install the server using the built‑in installer. This writes a default config and a systemd unit for you.

1) Install the server binary

```
cargo install --path gamiscreen-server
```

2) Run the installer (as root)

```
sudo gamiscreen-server install \
  --unit-path /etc/systemd/system/gamiscreen-server.service \
  --config-path /etc/gamiscreen/config.yaml \
  --db-path /var/lib/gamiscreen/app.db

sudo systemctl daemon-reload
sudo systemctl enable --now gamiscreen-server
```

Notes
- The installer generates a random JWT secret and fills it into the config template.
- Use `--force` to overwrite existing files. You can also customize `--user`, `--group`, `--working-dir`, or `--bin-path`.
- The server embeds the web app. On first build, it will run `npm install` and `npm run build` in `gamiscreen-web/` automatically. Set `SKIP_WEB_BUILD=1` to skip this behavior (useful on CI or when serving the web separately).
- HTTPS/production: the server listens on HTTP. Use a reverse proxy (e.g., Nginx, Caddy) for TLS when exposing publicly.

Uninstall

```
sudo gamiscreen-server uninstall --unit-path /etc/systemd/system/gamiscreen-server.service

# Also remove the config
sudo gamiscreen-server uninstall --unit-path /etc/systemd/system/gamiscreen-server.service \
  --remove-config --config-path /etc/gamiscreen/config.yaml

sudo systemctl daemon-reload
```

## Web (optional dev mode)

For a fast developer loop with live reload, use the Vite dev server. See `gamiscreen-web/README.md` for details.

Quick start:
```
cd gamiscreen-web
# Option A: same-origin (set server CORS)
VITE_API_BASE_URL=http://localhost:5151 npm run dev

# Option B: Vite proxy
env VITE_DEV_PROXY=1 VITE_API_PROXY_TARGET=http://localhost:5151 npm run dev
```

## Linux Client

1) Install the binary

```
# From workspace root
cargo install --path gamiscreen-client
```

2) Authenticate and bootstrap

```
# Logs in and stores a device-bound token in the keyring;
# also writes ~/.config/gamiscreen/client.yaml
gamiscreen-client login --server http://localhost:5151 --username parent
```

Alternatively, create the config manually from `gamiscreen-client/config.example.yaml` and ensure the keyring has a token for the server URL.

3) Run as a systemd user service (recommended)

```
mkdir -p ~/.config/systemd/user
cp gamiscreen-client/systemd/gamiscreen-client.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now gamiscreen-client

# View logs
journalctl --user -u gamiscreen-client -f
```

Notes
- The agent sends heartbeats every minute and locks the session when time runs out or when the server is unreachable for ~5 minutes.
- Config resolution order: `--config`, `$GAMISCREEN_CONFIG`, then `~/.config/gamiscreen/client.yaml`.
- You can override the lock mechanism with `lock_cmd` in the config if DBus is not available.
