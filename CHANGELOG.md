# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Breaking Changes
- `balance` field in `RemainingDto`, `RewardResp`, and `HeartbeatResp` now represents the virtual bank account balance (0 = no debt, negative = debt from borrowing), replacing the previous `earned - used` computation. This is a semantic change -- the field name is unchanged but its meaning has changed.
