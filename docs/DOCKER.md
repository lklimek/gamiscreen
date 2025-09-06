# Docker & Compose

This project ships a multi-stage Docker build for the server and a `docker-compose.yml` with an optional Caddy reverse proxy for HTTPS.

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
- `gamiscreen-proxy` (Caddy)
  - Terminates HTTPS using Caddy’s internal CA (self-signed).
  - Set hostname via `CERT_CN` env (default `localhost`).
  - Proxies to `gamiscreen-server:5151`.
  - Exposes `80` and `443` on the host.

Files
- `docker-compose.yml`: service definitions and volumes.
- `reverse-proxy/Caddyfile`: Caddy config. Uses `{$CERT_CN}` to set the hostname.
- `gamiscreen-server/Dockerfile`: multi-stage build that compiles the web UI and the Rust server.

Production notes
- Replace the example config and user credentials. Use strong bcrypt hashes.
- For real TLS certificates, adjust `reverse-proxy/Caddyfile` to use ACME/Let’s Encrypt for your domain instead of `tls internal`.
- Restrict exposed ports as needed (e.g., expose only 443; keep 5151 internal to the network).
- Consider mounting a host path for database backups instead of a named volume.

Development notes
- The Dockerfile builds the web UI explicitly, then compiles the Rust server with `SKIP_WEB_BUILD=1` to avoid duplicate work.
- You can run the server directly on your machine and still use the Caddy proxy container by pointing it at `host.docker.internal:5151` in the Caddyfile if preferred.

