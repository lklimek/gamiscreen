# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- `balance_transactions` table — audit trail for debt-affecting operations (lending, auto-repayment)
- `account_balance` stored column on `balances` table — replaces computed balance
- Auto-repayment feedback in web UI when earning time during debt
- "Account Balance" label and "No debt" display in child details

### Breaking Changes
- `balance` field in `RemainingDto`, `RewardResp`, and `HeartbeatResp` now represents the virtual bank account balance (0 = no debt, negative = debt from borrowing), replacing the previous `earned - used` computation. This is a semantic change -- the field name is unchanged but its meaning has changed.
