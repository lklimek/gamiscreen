# Windows Client Architecture

This document explains the redesigned Windows deployment strategy for `gamiscreen-client`. The goal is to provide a reliable, admin-installable agent that still stores credentials in each child's user context.

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
  - `gamiscreen-client install`: admin command that stages binaries, installs/starts the service, and logs progress (Event Log + console).
  - `gamiscreen-client uninstall`: admin command that stops/deletes the service and removes staged artifacts.

## Lifecycle

1. **Installation (admin)**
   - Copy release payload to `%ProgramFiles%\GamiScreen\Client` (or another writable program directory).
   - Register `GamiScreen Agent Service` with startup type `Automatic (Delayed Start)` and description metadata.
   - Configure the service to run `gamiscreen-client.exe service run` under `LocalSystem`.
   - Add an Event Log source for service diagnostics.
2. **First-run provisioning (per child)**
   - Each child (or parent impersonating them) signs into Windows and runs `gamiscreen-client login`.
   - The command stores the device token in the user's Windows Credential Manager entry under `gamiscreen-client`.
   - No privileged action is required.
3. **Session activation**
   - The service receives `SESSION_LOGON`, `SESSION_UNLOCK`, or `CONSOLE_CONNECT` notifications.
   - For each interactive session with an accessible user token, the service spawns `session-agent`.
   - The session agent loads the per-user config/token and starts heartbeats.
4. **Shutdown**
   - On `SESSION_LOGOFF`, `SESSION_LOCK`, or service stop, the supervisor terminates the session agent.
   - Each agent also exits gracefully when its parent service dies (inheritance detection) or when it loses access to the user's desktop.

## Token Handling

- Tokens remain scoped to `HKCU\Software\Microsoft\Credentials` (via the `keyring` crate) for each user.
- If a session agent starts without a token/config, it reports the error through the Event Log and exits; the service logs the failure and retries on the next session change.
- Shared PCs supporting multiple child accounts simply rely on each account running `login` once.

## Updates

- Automatic updates continue to use the existing self-update flow.
- When an update is staged, the session agent replaces its executable and restarts itself, leaving the Windows Service untouched.
- The service periodically probes the binary timestamp/checksum; if it detects an update failure, it can relaunch workers or restart itself.

## Logging & Diagnostics

- Service logs critical events to the Windows Event Log (e.g., failed worker spawn, missing token, repeated crashes).
- Session agents continue to stream logs via `tracing` to rotating files under `%ProgramData%\GamiScreen\Logs` and to stderr when run interactively.
- Admins can inspect the service using `Get-Service GamiScreen` or `sc query "GamiScreen Agent Service"`.

## Uninstall

- Stop and delete the Windows Service.
- Remove `%ProgramFiles%\GamiScreen\Client` (if empty) and the Event Log source.
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
