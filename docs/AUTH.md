# Auth & Access Control

## Overview

- JWTs signed by the server; sessions tracked server‑side with an inactivity window.
- Roles
  - Parent: list children and tasks, view remaining minutes, grant rewards.
  - Child: send heartbeats only for their own `child_id` and registered `device_id`.

Session policy
- Server stores sessions by `jti`. Tokens are not auto-extended on API usage; clients must explicitly renew them.
- Renewal endpoint: `POST /api/v1/auth/renew` consumes the presented token, issues a new one, and invalidates the previous session.
- Inactivity window: tokens become invalid after 7 days without renewal.
- Token expiry (`exp`): 30 days from issuance.

## Device Registration

- `POST /api/v1/family/{tenant}/children/{child_id}/register` issues a child token bound to `{ child_id, device_id }`. The tenant identifier comes from the server configuration and is embedded in issued JWTs.
- Parents can register on behalf of a child by passing `child_id`; children can self‑register without it.

## Heartbeat Enforcement

- `POST /api/v1/family/{tenant}/children/{child_id}/device/{device_id}/heartbeat` requires a child token; the body `{ child_id, device_id }` must match the token claims (tenant from the token must match the configured tenant).
