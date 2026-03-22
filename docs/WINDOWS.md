# Windows Client Architecture

This document explains the Windows deployment strategy for `gamiscreen-client`. The goal is to provide a reliable, admin-installable agent that still stores credentials in each child's user context.

## Quick Start

All commands require an elevated (Administrator) PowerShell unless noted.

```powershell
# 1. Install and start the Windows service (auto-starts after install)
gamiscreen-client service install

# 2. Provision each child account (run from the child's Windows session, no admin needed)
gamiscreen-client login --server http://your-server:5151 --username parent

# 3. Verify
Get-Service GamiScreenAgent
```

The service handles everything else: it detects user sessions and spawns per-session agents automatically. Use `gamiscreen-client service start` if the service was stopped manually. For full setup details, see docs/INSTALL.md.

## Components

- **GamiScreen Agent Service** (`gamiscreen-client service run`)
  - Runs under `LocalSystem` (or a dedicated virtual account).
  - Registers for `SERVICE_CONTROL_SESSIONCHANGE` events via the Service Control Manager (SCM).
  - Supervises per-session workers and keeps minimal state (session map, worker PIDs, health metrics).
- **Session Agent** (`gamiscreen-client session-agent`)
  - Spawned by the service inside the interactive user's session using `WTSQueryUserToken` + `CreateProcessAsUser`.
  - Reuses the existing main loop: reads the device token from the user's keyring, performs heartbeats, countdown notifications, and locks the session when required.
  - Terminates when the user signs out, the session goes away, or the service issues a stop.
- **Interactive CLI utilities**
  - `gamiscreen-client login`: unchanged; prompts for credentials and stores per-user tokens and config.
  - `gamiscreen-client service install`: admin command that registers and starts the service (logs progress to console).
  - `gamiscreen-client service uninstall`: admin command that stops and deletes the service.

## Lifecycle

1. **Installation (admin)**
   - Place the release binary in a directory on the system `PATH` (e.g., `%ProgramFiles%\GamiScreen\Client`).
   - Run `gamiscreen-client service install` to register and start the `GamiScreen Agent` service with startup type `Automatic` under `LocalSystem`.
2. **First-run provisioning (per child)**
   - Each child (or parent impersonating them) signs into Windows and runs `gamiscreen-client login`.
   - The command stores the device token in the user's Windows Credential Manager entry under `gamiscreen-client`.
   - No privileged action is required.
3. **Session activation**
   - The service receives `SESSION_LOGON`, `CONSOLE_CONNECT`, or `REMOTE_CONNECT` notifications.
   - For each interactive session with an accessible user token, the service spawns `session-agent --session-id N`.
   - The session agent loads the per-user config/token and starts heartbeats.
   - Screen lock/unlock detection is handled internally by each session agent (not the service).
4. **Shutdown**
   - On `SESSION_LOGOFF`, `CONSOLE_DISCONNECT`, `REMOTE_DISCONNECT`, or service stop, the supervisor terminates the session agent.
   - Each agent also exits gracefully when the service issues a stop command.

## Token Handling

- Tokens remain scoped to `HKCU\Software\Microsoft\Credentials` (via the `keyring` crate) for each user.
- If a session agent starts without a token/config, it logs the error and exits; the service logs the failure and retries on the next session change.
- Shared PCs supporting multiple child accounts simply rely on each account running `login` once.

## Updates

- Automatic updates continue to use the existing self-update flow.
- When an update is staged, the session agent replaces its executable and restarts itself, leaving the Windows Service untouched.
- The service periodically probes the binary timestamp/checksum; if it detects an update failure, it can relaunch workers or restart itself.

## Logging & Diagnostics

- Service logs are written via `tracing-appender` to rotating files under `%ProgramData%\gamiscreen\logs` (daily rotation, 7-day retention).
- Session agent logs go to per-user `%LOCALAPPDATA%\gamiscreen\gamiscreen\logs`.
- Both emit to stderr when run interactively.
- Admins can inspect the service using `Get-Service GamiScreenAgent` or `sc query GamiScreenAgent`.

## Uninstall

- Stop and delete the Windows Service.
- Remove `%ProgramFiles%\GamiScreen\Client` (if empty).
- Leave per-user tokens/configs untouched unless `--purge-user-data` is requested.

## Future Enhancements

- Support running the service under a managed service account with restricted privileges.
- Provide a GUI bootstrapper that wraps `login` and `install` for non-technical parents.
- Telemetry hook for reporting worker health to the server (optional, opt-in).

## CLI Alignment

- Windows admins should use `gamiscreen-client service install`.
- Introduce explicit CLI modes to match the service/agent split:
  - `gamiscreen-client service <install|start|stop|run>` for Windows service management (admin-only).
  - `gamiscreen-client session-agent` for the per-user worker that the service launches.
  - `gamiscreen-client agent` (default) remains the cross-platform direct run path.
- Keep `login` as an interactive helper for token provisioning; it is independent of platform.
- Gate service-specific subcommands with `#[cfg(windows)]` to preserve the platform boundary enforced by the `Platform` trait.
- Share as much logic as possible by locating the long-running loop in a reusable module (e.g., `app::agent::run`).
