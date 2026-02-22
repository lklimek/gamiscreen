# CLAUDE.md

## Project Overview

Gamiscreen is a self-hosted parental control app that gamifies screen time. Parents grant
minutes for completed tasks; children see remaining time at a glance. The Linux client
enforces limits locally, even when the server is offline.

## Workflow guidelines

- Work in a team.
- Check project architecture before doing any changes, to find best place for the changes.
- When coding, follow best practices. Always lint and format your code.
- Run code quality review and security review before concluding your work, and address findings.
- Clearly communicate decisions and possible implications in a task summary.

## Tech Stack

- **Server & Clients**: Rust (edition 2024, MSRV 1.91)
- **Web UI**: React 19 + TypeScript + Vite, styled with Pico CSS
- **Android**: Kotlin + Jetpack Compose
- **Database**: SQLite via Diesel ORM (migrations embedded and applied on startup)
- **Session control**: Timekpr-Next (Linux)

## Repository Layout

- `gamiscreen-server/` — REST API + embedded web assets (Axum + Tokio)
- `gamiscreen-client/` — Linux/Windows agent
- `gamiscreen-shared/` — shared types, API definitions, TypeScript codegen (`ts-rs`)
- `gamiscreen-web/` — React SPA (built by server's `build.rs`)
- `android/` — Android app (embeds gamiscreen-web)
- `docs/` — architecture, auth, install, config, usage guides
- `scripts/` — build and CI helpers

## Build & Test

```bash
# Rust — build and test entire workspace
cargo build --workspace
cargo test --workspace

# Formatting (nightly required)
cargo +nightly fmt

# Lint
cargo clippy --workspace

# Web UI dev server
cd gamiscreen-web && npm run dev
```

System dependencies for building: `pkg-config`, `libdbus-1-dev`, `libsqlite3-dev`.

## Code Quality Checklist (before every commit)

1. `cargo +nightly fmt`
2. `cargo clippy --workspace`
3. `cargo test --workspace`

## Conventions

- Follow Rust idiomatic patterns; don't duplicate existing code.
- Use conventional commits (e.g. `feat:`, `fix:`, `refactor:`).
- Write unit tests for new functionality.
- TypeScript types are auto-generated from Rust structs via `ts-rs` — edit the Rust source, not the generated TS files in `gamiscreen-web/src/generated/`.
- Refer to `docs/` before making architectural changes.

## Resolving PR Review Threads

After addressing a review comment, resolve its thread using the `claudius:github` skill's
wrapper scripts (provided by the claudius plugin):

```bash
# 1. List unresolved threads for a PR
gh-list-review-threads.sh {PR_NUMBER}

# 2. Resolve a specific thread
gh-resolve-review-thread.sh {PR_NUMBER} {THREAD_ID}
```

## Claudius Plugin

This project uses the **claudius** plugin (`claudius@claudius` from the `lklimek/claudius` marketplace).

### Personality Skill

Always use the `claudius:personality` skill when communicating with the user. This applies
to all interactions — issue comments, PR reviews, and general conversation.

### Prefer Claudius Agents

When a task matches one of the agents provided by the claudius plugin, prefer using that
agent over a generic approach. Claudius agents are purpose-built for this project's
workflows and should be the first choice whenever they are a good fit for the work at hand.
