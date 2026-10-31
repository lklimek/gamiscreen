# Android encrypted session storage

- [x] Add tests for encrypted persistence, nonce uniqueness, tampering, malformed records, missing keys, logout and unavailable storage.
- [x] Replace retired security-crypto with Android Keystore AES-GCM and a fresh storage namespace; do not migrate old tokens.
- [x] Remove the retired dependency and document re-login and device-test limits.
- [x] Run core unit tests, Android lint and debug APK assembly; self-review changes.

Validation: 12 core tests passed; core lint reported no issues; app debug APK assembled; `cargo +nightly fmt` and `git diff --check` passed. No Kotlin formatter is configured.

TODO: verify native Android Keystore persistence and provider errors on a device; host tests use ephemeral JCA keys.
