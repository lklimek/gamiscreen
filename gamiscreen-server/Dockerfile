# syntax=docker/dockerfile:1.6

###############################
# Build stage
###############################
FROM rust:1-trixie AS builder

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
    pkg-config libsqlite3-dev nodejs npm ca-certificates \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Leverage Docker layer caching for Rust deps
COPY Cargo.toml Cargo.lock ./
COPY gamiscreen-server/Cargo.toml gamiscreen-server/Cargo.toml
COPY gamiscreen-client-linux/Cargo.toml gamiscreen-client-linux/Cargo.toml

# Create minimal placeholders to satisfy workspace structure
RUN mkdir -p gamiscreen-server/src gamiscreen-client-linux/src \
 && echo "fn main(){}" > gamiscreen-server/src/main.rs \
 && echo "pub fn placeholder(){}" > gamiscreen-client-linux/src/lib.rs

# Pre-build Rust dependencies without invoking web build.rs
RUN SKIP_WEB_BUILD=1 cargo build -p gamiscreen-server --release || true

# Cache web dependencies
COPY gamiscreen-web/package.json gamiscreen-web/package.json
COPY gamiscreen-web/package-lock.json gamiscreen-web/package-lock.json
RUN cd gamiscreen-web && npm ci --no-audit --no-fund

# Now copy the actual sources
COPY . .

# Build the web UI explicitly so assets exist before Rust compile (for embed)
RUN cd gamiscreen-web && npm run build

# Build release binary, skipping web build.rs step (dist already built)
RUN SKIP_WEB_BUILD=1 cargo build -p gamiscreen-server --release

###############################
# Runtime stage
###############################
FROM debian:trixie-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
    libsqlite3-0 ca-certificates \
 && rm -rf /var/lib/apt/lists/*

ENV RUST_LOG=info \
    CONFIG_PATH=/etc/gamiscreen/config.yaml \
    DB_PATH=/var/lib/gamiscreen/app.db

# Create non-root user
RUN useradd -r -s /usr/sbin/nologin -d /var/lib/gamiscreen gamiscreen \
 && mkdir -p /etc/gamiscreen /var/lib/gamiscreen \
 && chown -R gamiscreen:gamiscreen /var/lib/gamiscreen

COPY --from=builder /app/target/release/gamiscreen-server /usr/local/bin/gamiscreen-server

USER gamiscreen:gamiscreen
WORKDIR /var/lib/gamiscreen

EXPOSE 5151

VOLUME ["/var/lib/gamiscreen"]

CMD ["/usr/local/bin/gamiscreen-server"]
