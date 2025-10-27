# Docker & Compose

This project ships a multi-stage Docker build for the server and a lean `docker-compose.yml` that runs the server container directly.

## Images

- Server image name: `gamiscreen/server:latest` (built from `gamiscreen-server/Dockerfile`).
- Architecture: Debian slim runtime with `libsqlite3-0` and a non-root `gamiscreen` user.

Build locally
```
docker compose build gamiscreen-server
```

Run with Compose
```
cp gamiscreen-server/config.yaml.example gamiscreen-server/config.yaml
# Edit gamiscreen-server/config.yaml to your needs

docker compose up -d
```

Services
- `gamiscreen-server`
  - Exposes `5151` on the host (`ports: 5151:5151`).
  - Mounts your config: `./gamiscreen-server/config.yaml:/etc/gamiscreen/config.yaml:ro`.
  - Persists data in volume `gamiscreen-data:/var/lib/gamiscreen`.
  - Environment:
    - `RUST_LOG=info`
    - `CONFIG_PATH=/etc/gamiscreen/config.yaml`
    - `DB_PATH=/var/lib/gamiscreen/app.db`

Files
- `docker-compose.yml`: service definitions and volumes.
- `gamiscreen-server/Dockerfile`: multi-stage build that compiles the web UI and the Rust server.

Production notes
- Replace the example config and user credentials. Use strong bcrypt hashes.
- Terminate TLS with your infrastructure of choice (reverse proxy, load balancer, ingress controller) in front of the container when exposing it publicly.
- Restrict exposed ports as needed (e.g., publish 5151 internally within your network VPN/VPC and terminate TLS elsewhere).
- Consider mounting a host path for database backups instead of a named volume.

Development notes
- The Dockerfile builds the web UI explicitly, then compiles the Rust server with `SKIP_WEB_BUILD=1` to avoid duplicate work.
