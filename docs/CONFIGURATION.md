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
- `push` (object, optional): Web Push settings.
  - `enabled` (bool): turn Web Push delivery on/off (`false` by default).
  - `vapid_public` (string, optional): Base64URL-encoded VAPID public key.
  - `vapid_private` (string, optional): Base64URL-encoded VAPID private key (keep secret).
  - `contact_email` (string, optional): contact URI (e.g., `mailto:admin@example.com`) advertised in push messages.
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
- `PUSH_ENABLED`: override `push.enabled` (`true` / `false`).
- `PUSH_VAPID_PUBLIC`: override `push.vapid_public`.
- `PUSH_VAPID_PRIVATE`: override `push.vapid_private`.
- `PUSH_CONTACT_EMAIL`: override `push.contact_email`.

Secrets management
- Store long-term secrets (e.g., `jwt_secret`, `PUSH_VAPID_PRIVATE`) outside of version control—use deployment-time environment variables or secret managers.
- Provide `.env` templates per environment (e.g., `.env.production`) and load them in container/orchestrator manifests.
- Rotate VAPID keys periodically; update both config and gamiscreen-web build (public key) together to avoid mismatches.
- In multi-tenant setups, prefer per-tenant secrets managed by the orchestration layer rather than sharing a single key set.

### Generating VAPID keys

Use the [`web-push`](https://github.com/web-push-libs/web-push) CLI (ships with the npm package) to generate a VAPID key pair:

```bash
npx web-push generate-vapid-keys
```

The command prints two Base64URL strings:
- `Public Key` → copy into `config.yaml` under `push.vapid_public` and expose as `VITE_VAPID_PUB_KEY` (or `window.gamiscreenVapidPublicKey`) for the web app.
- `Private Key` → copy into `push.vapid_private` (and supply via `PUSH_VAPID_PRIVATE` in production). Keep this value secret; treat it like any other long-term credential.

Optionally provide a contact address—e.g. configure `push.contact_email: "mailto:admin@example.com"` or set `PUSH_CONTACT_EMAIL`. Some push services surface this in diagnostics.

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

Derived at runtime
- Child and device identifiers come from the JWT provisioned during `gamiscreen-client login`; they no longer appear in the config file.
- Heartbeats run every 60 seconds and a 45-second pre-lock countdown notification is always shown when remaining time drops to one minute. These values are hardcoded for now.

Tokens
- `gamiscreen-client login` stores a device-bound token in the OS keyring. The agent reads it automatically based on `server_url`.

Systemd (client)
- A user service unit is provided at `gamiscreen-client/systemd/gamiscreen-client.service`. See docs/INSTALL.md for setup.
