gamiscreen-client

Linux session agent that heartbeats to the GamiScreen server and locks the screen when time runs out or the server is unreachable for >5 minutes.

Build

- `cargo build -p gamiscreen-client`

Config

- Resolution order:
  1) `--config` / `-c` path
  2) `$GAMISCREEN_CONFIG`
  3) XDG: `~/.config/gamiscreen/client.yaml`
- Example: see `gamiscreen-client/config.example.yaml`.
- Fields:
  - `server_url`: Base URL to GamiScreen server (e.g., http://127.0.0.1:5151)
  - `child_id`: Child identifier configured on the server
  - `device_id`: Arbitrary identifier for this device
  - `interval_secs`: Optional, default 60
  - `lock_cmd`: Optional command array; if not provided, the client locks via DBus (GNOME → login1).

Run as systemd user service

1) Copy `gamiscreen-client/systemd/gamiscreen-client.service` to `~/.config/systemd/user/gamiscreen-client.service`.
2) Ensure binary is in PATH (e.g., `~/.cargo/bin/gamiscreen-client`).
3) Ensure config exists at `~/.config/gamiscreen/client.yaml`.
4) `systemctl --user daemon-reload`
5) `systemctl --user enable --now gamiscreen-client`

Logs: `journalctl --user -u gamiscreen-client -f`

Notes

- On startup, the client detects an available DBus lock backend and logs the selection.
- Default lock path is DBus: org.gnome.ScreenSaver → org.freedesktop.login1 Manager.
- Token handling: Use `gamiscreen-client login` to authenticate; the token is stored in your system keyring keyed by the server URL. The agent reads the token from the keyring automatically.
- Heartbeats: every `interval_secs` the client posts `/api/children/{child_id}/device/{device_id}/heartbeat` with a list of UTC minute timestamps covering all minutes since the last successful heartbeat. The server deduplicates across devices, so simultaneous usage is counted once.

Login helper

- `gamiscreen-client login [--server <URL>] [--username <USER>]`
  - Prompts for password, calls `/api/auth/login`.
  - If logged in as Parent, prompts for `child_id` to provision.
  - Generates a `device_id`, calls `/api/children/{child_id}/register` to obtain a device‑bound child token, stores it in keyring.
  - Writes the config file.
- You can force a custom command via `lock_cmd` when DBus isn’t available.
- Backoff/failsafe: locks the screen after ~5 minutes of continuous failures.
- Agent reads the device token from the keyring (keyed by server URL). Heartbeats use `POST /api/children/{child_id}/device/{device_id}/heartbeat`.
