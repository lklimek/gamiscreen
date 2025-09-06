# Auth & Access Control

## Overview

- JWTs signed by the server; sessions tracked server‑side with an inactivity window.
- Roles
  - Parent: list children and tasks, view remaining minutes, grant rewards.
  - Child: send heartbeats only for their own `child_id` and registered `device_id`.

Session policy
- Server stores sessions by `jti` and updates last-used timestamp on each request.
- Inactivity window: tokens become invalid after 7 days without use.
- Token expiry (`exp`): 30 days from issuance.

## Device Registration

- `POST /api/client/register` issues a child token bound to `{ child_id, device_id }`.
- Parents can register on behalf of a child by passing `child_id`; children can self‑register without it.

## Heartbeat Enforcement

- `POST /api/heartbeat` requires a child token; the body `{ child_id, device_id }` must match the token claims.
