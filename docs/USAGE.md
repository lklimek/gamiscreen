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
- If Parent, prompts for `child_id` to provision; generates a `device_id` and calls `/api/client/register`.
- Stores a device token in the system keyring and writes `~/.config/gamiscreen/client.yaml` with `server_url`, `child_id`, and `device_id`.

See also: docs/INSTALL.md for full installation and systemd setup.

