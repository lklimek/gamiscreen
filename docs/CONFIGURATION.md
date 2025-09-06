# Configuration

This page covers configuration for both the server and the Linux client, plus relevant environment variables.

## Server (`config.yaml`)

Path resolution
- Env `CONFIG_PATH` (default: `./config.yaml`)
- Docker default: `/etc/gamiscreen/config.yaml`

Top-level fields
- `jwt_secret` (string): long random secret for signing JWTs.
- `dev_cors_origin` (string, optional): allowed origin for development (e.g., `http://localhost:5173`).
- `listen_port` (number, optional): port to listen on if provided; otherwise `PORT` env or 5151.
- `users` (array): list of user accounts.
  - `username` (string)
  - `password_hash` (string): bcrypt hash of the password.
  - `role` (string): `parent` or `child`.
  - `child_id` (string, required for role `child`): associates the child user with a specific child record.
- `children` (array): child records.
  - `id` (string): stable identifier.
  - `display_name` (string): friendly name.
- `tasks` (array): rewardable tasks.
  - `id` (string)
  - `name` (string)
  - `minutes` (number): minutes rewarded when completed.

Example
See `gamiscreen-server/config.yaml.example` for a complete, annotated example including example bcrypt hashes and two children.

Environment variables
- `CONFIG_PATH`: path to `config.yaml` (default: `./config.yaml`).
- `DB_PATH`: SQLite database path (default: `data/app.db`).
- `PORT`: listen port (overrides `listen_port` if set). Default: 5151.
- `RUST_LOG`: log level (e.g., `info`, `debug`).
- `SKIP_WEB_BUILD`: when building the server crate, skips automatic web build; useful in CI.

Notes
- On first start, the server seeds the database with `children` and `tasks` from the config.
- Use bcrypt for `password_hash`. The example config shows commands to generate hashes with `htpasswd` or `mkpasswd`.

## Client (`~/.config/gamiscreen/client.yaml`)

Path resolution
1) `--config PATH`
2) Env `GAMISCREEN_CONFIG`
3) Default: `~/.config/gamiscreen/client.yaml`

Fields
- `server_url` (string): base URL of the server, e.g., `http://127.0.0.1:5151`.
- `child_id` (string): child identifier as configured on the server.
- `device_id` (string): unique ID for the child’s device.
- `interval_secs` (number, default 60): heartbeat interval in seconds.
- `warn_before_lock_secs` (number, default 10): show a countdown notification this many seconds before lock when time is about to run out.
- `lock_cmd` (array of strings, optional): override lock command if DBus is unavailable. Example: `["loginctl", "lock-session", "$XDG_SESSION_ID"]`.

Tokens
- `gamiscreen-client login` stores a device-bound token in the OS keyring. The agent reads it automatically based on `server_url`.

Systemd (client)
- A user service unit is provided at `gamiscreen-client/systemd/gamiscreen-client.service`. See docs/INSTALL.md for setup.

