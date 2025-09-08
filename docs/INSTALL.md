# Installation

The project consists of a Rust server, a React web UI (built and embedded into the server), and a Linux client. SQLite is used for storage, with migrations applied automatically on startup.

See also: docs/CONFIGURATION.md for all server/client settings and docs/DOCKER.md for containerized deployment.

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

### Lock Testing (Linux)

To diagnose screen‑lock behavior across desktops and full‑screen scenarios, use the helper CLI. It tries several strategies and reports success:

```
cargo run -p gamiscreen-client -- lock
```

Methods attempted, in order:
- GNOME ScreenSaver (DBus, session bus)
- org.freedesktop.ScreenSaver (DBus, session bus; common on KDE/others)
- systemd‑logind Manager.LockSessions (DBus, system bus)
- systemd‑logind Session.Lock (DBus, system bus; current session)
- `loginctl lock-session` (command)
- `xdg-screensaver lock` (command; mostly X11)

After each attempt, it queries lock status via GNOME ScreenSaver or login1 `LockedHint` when available.

Select a single method with `--method` (default: `all`). Examples:

```
cargo run -p gamiscreen-client -- lock --method login1-manager
cargo run -p gamiscreen-client -- lock --method loginctl
```

Interactive mode
- When run without `--method`, it runs each method sequentially and, after each attempt, prompts: "Did the screen lock? [y/N]". Unlock your screen if needed, then answer.
- A summary is printed at the end listing which methods you confirmed as working on your setup.

### Client Install Helper (polkit + systemd)

The client includes a convenience installer that:
- Creates group `gamiscreen` and adds the current user to it.
- Installs a polkit rule allowing users in `gamiscreen` to lock the session via `org.freedesktop.login1` without prompts.
- Installs and enables the user systemd service for `gamiscreen-client`.

Run:

```
gamiscreen-client install
```

This uses `sudo` for privileged steps (group, polkit). You may be prompted for your password. After installation, log out and back in so the new group takes effect.

To uninstall:

```
gamiscreen-client uninstall
```

This disables/removes the user unit and deletes the polkit rule. The `gamiscreen` group and membership are left intact.

Manual polkit rule (alternative)

Create `/etc/polkit-1/rules.d/49-gamiscreen-lock.rules` with (also in `gamiscreen-client/polkit/49-gamiscreen-lock.rules`):

```
polkit.addRule(function(action, subject) {
  if ((action.id == "org.freedesktop.login1.lock-sessions" ||
       action.id == "org.freedesktop.login1.lock-session") &&
      subject.isInGroup("gamiscreen")) {
    return polkit.Result.YES;
  }
});
```

Then add your user to the `gamiscreen` group:

```
sudo groupadd -f gamiscreen
sudo usermod -aG gamiscreen $USER
```

Relogin (or reboot) to apply group changes.
