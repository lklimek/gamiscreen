# Usage & Workflows

## Web Workflow

Parent
1) Log in to the web app.
2) Status page shows children with remaining minutes (auto‑refresh every 60s; manual refresh available).
3) Open a child’s details.
4) Click a task or enter custom minutes to reward.
5) Confirm. Remaining updates immediately; reward history refreshes.

Child
1) Log in to the web app.
2) Child view shows remaining minutes and today’s tasks (read‑only).

Notes
- Minutes can carry over if not used in a day.
- The server embeds and serves the web app build output.

## Linux Client Registration (CLI)

`gamiscreen-client login`
- Logs in as Parent or Child.
- If Parent, prompts for `child_id` to provision; generates a `device_id` and calls `/api/v1/family/{tenant}/children/{child_id}/register`. The tenant identifier is read from the login token (which mirrors the server config).
- Stores a device token in the system keyring and writes `~/.config/gamiscreen/client.yaml` with `server_url`, `child_id`, and `device_id`.

See also: docs/INSTALL.md for full installation and systemd setup.

### Running the agent manually

- `gamiscreen-client agent` (default) starts the foreground agent in the current session.
- `gamiscreen-client install`/`uninstall` manage the Linux systemd + polkit setup when run on Linux (with `--user` when invoked as root).

## Windows Service Workflow (CLI)

Full lifecycle: install the service, provision tokens, start, verify, troubleshoot.

### Setup

1) Install the service (elevated PowerShell):

```powershell
gamiscreen-client service install
```

2) Log into each child's Windows account and provision a token:

```powershell
gamiscreen-client login --server http://your-server:5151 --username parent
```

3) Start the service:

```powershell
gamiscreen-client service start
```

The service spawns a session agent per logged-in user automatically.

### Checking service status

```powershell
Get-Service GamiScreenAgent

# Detailed status via SCM
sc query GamiScreenAgent
```

### Viewing logs

Session agent logs are written to `%ProgramData%\GamiScreen\Logs`. Service-level events go to the Windows Event Log.

```powershell
# View recent log files
Get-ChildItem "$env:ProgramData\GamiScreen\Logs"

# View Event Log entries
Get-EventLog -LogName Application -Source GamiScreen -Newest 20
```

### Debugging

Run the agent directly in the foreground (bypasses the service):

```powershell
gamiscreen-client agent
```

### Common issues

- **Service won't start**: ensure `gamiscreen-client service install` was run from an elevated prompt. Check Event Log for errors.
- **No token for a child account**: the session agent exits immediately if no token is found. Log into that Windows account and run `gamiscreen-client login`.
- **Session agent crashes repeatedly**: the service uses exponential backoff (1s to 60s) before restarting a failed session agent. Check logs under `%ProgramData%\GamiScreen\Logs`.

### Stopping and uninstalling

```powershell
gamiscreen-client service stop
gamiscreen-client service uninstall
```

Notes
- `gamiscreen-client session-agent --session-id N` is spawned by the service internally. Do not run it manually unless debugging.
- The service runs under LocalSystem with auto-start. It detects session logon/logoff/unlock events and manages session agents accordingly.
