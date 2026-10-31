# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.14.0] - 2026-10-31

### Added
- Manage tasks in the web app: create, edit, assign to children, set priorities and schedule mandatory days/start times (#49).
- Store tasks in SQLite with an initial import from YAML; preserve task completion and reward history when deleting tasks (#49).
- Apply family-local dates and times using the configured IANA timezone (UTC by default) (#49).

### Changed
- Update Rust, web and Android dependencies and CI actions; use Rust 1.99.1 stable, Node.js 26 LTS and JDK 25 LTS (#61).
- Use maintained YAML, JWT/Web Push and desktop credential/notification backends (#61).
- Replace retired Android credential storage with Android Keystore AES-256-GCM. Native Android sessions are not migrated; users may need to sign in again. Linux/Windows keyring identifiers and JWT compatibility are retained (#61).
- Extend task APIs with assignment, priority, schedule and family-local completion/blocking status (#49).

### Fixed
- Keep task controls and history usable on narrow mobile screens (#60).
- Preserve string identifiers, names and secrets during YAML configuration upgrades, and keep configuration values out of parse-error logs (#61).

### Security
- Remove vulnerable and unmaintained Rust dependency chains, including rsa, crypto-bigint, unsafe-libyaml and legacy zvariant packages; no audit advisories are ignored (#61).

## [0.13.0] - 2026-03-29

### Added
- Windows client platform rewrite with native implementation (#41)
- Web UX hero "Time Left" display, hidden Balance, inline debt explanation (#45)
- `balance_transactions` table — audit trail for debt-affecting operations (lending, auto-repayment)
- `account_balance` stored column on `balances` table — replaces computed balance
- Auto-repayment feedback in web UI when earning time during debt
- "Account Balance" label and "No debt" display in child details

### Breaking Changes
- `balance` field in `RemainingDto`, `RewardResp`, and `HeartbeatResp` now represents the virtual bank account balance (0 = no debt, negative = debt from borrowing), replacing the previous `earned - used` computation. This is a semantic change -- the field name is unchanged but its meaning has changed.

### Fixed
- Correct balance double-counting borrowed minutes (#43)
- Show actionable error when client config file is missing
- Resolve cargo audit vulnerabilities
- Address grumpy review findings from PR #41 (#46)
