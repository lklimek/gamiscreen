# Dependency updates

Use the newest LTS release for toolchains offering LTS, otherwise the newest stable release. Direct dependency versions were verified against crates.io, npm, Google Maven, Maven Central and GitHub releases on 2026-10-31. Lockfiles retain upstream compatibility constraints for transitive packages.

| Tool | Selected version |
| --- | --- |
| Rust | 1.99.1 (stable; no LTS channel) |
| Node.js | 26.13.0 LTS |
| JDK | 25 LTS |
| Gradle | 9.8.2 |
| Android Gradle plugin | 9.5.0 |
| Kotlin | 2.4.21 |
| Compose Multiplatform | 1.12.2 |

Rust is pinned in `rust-toolchain.toml`; Node in `.nvmrc` and CI. The container uses the same Rust and Node versions. Android uses the committed Gradle wrapper, SDK 37.2 for compilation, target SDK 36 and JVM 21 bytecode. JDK 25 runs the build. Gradle/AGP versions exceed Kotlin's fully tested compatibility matrix; verify Android builds when changing this combination.

`Cargo.lock` at the workspace root is authoritative; the obsolete server-local lockfile is removed.

## Maintained dependency backends

- YAML uses `serde-saphyr` 2.0.0, replacing deprecated `serde_yaml` and `unsafe-libyaml`. Configuration migration preserves schema-defined string values (including numeric-looking identifiers) before updating its intermediate map; input and output remain YAML. Parser errors report a line/column without rendering source or field values.
- JWT signing/verification uses jsonwebtoken's `aws_lc_rs` backend. AWS-LC is a local cryptographic library; no AWS service is involved. Existing HS256 tokens and secrets remain compatible.
- Web Push uses Mozilla's `ece` for RFC 8291 payload encryption, jsonwebtoken/AWS-LC for RFC 8292 VAPID signatures, and reqwest for delivery. The application supplies HTTP protocol glue, not cryptographic primitives. Existing VAPID keys and subscriptions are retained. Requests keep the existing 28-day TTL, use 12-hour signatures, do not follow redirects, and expire subscriptions only after HTTP 404/410.
- The client uses `keyring-core` with Windows Credential Manager or `dbus-secret-service-keyring-store` (libdbus) on Linux. Both use the existing service/user credential identifiers. `notify-rust` also uses its maintained libdbus backend; blocking notification sends run outside Tokio's worker threads. The agent retains notification IDs rather than moving libdbus handles between threads.

These selections remove `rsa`, vulnerable `crypto-bigint`, `unsafe-libyaml`, `zvariant`, `zvariant_derive` and `zbus_names` from the lockfile. `cargo audit` reports zero vulnerabilities and zero warnings as of 2026-10-31, without ignored advisories or patched/vendored forks. Android session storage uses platform Android Keystore AES-256-GCM with a fresh preference namespace; legacy Android sessions are not migrated and may require signing in again. Re-run the audit when changing dependencies.

Validation includes a fixed independently signed HS256 token, wrong-secret/expired-token rejection, YAML round trips and legacy migration tests, mocked credential lookup, Web Push payload decryption/signature verification, and local HTTP delivery tests for success, 404/410 expiry, transient failures and redirects. Live desktop keyring/notification services and external browser push delivery require platform integration testing.

Primary references: [Rust channel manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml), [Node releases](https://nodejs.org/dist/index.json), [Java LTS roadmap](https://www.oracle.com/java/technologies/java-se-support-roadmap.html), [Gradle release](https://services.gradle.org/versions/current), [AGP migration](https://developer.android.com/build/migrate-to-built-in-kotlin), [KMP plugin](https://developer.android.com/kotlin/multiplatform/plugin), [Kotlin compatibility](https://kotlinlang.org/docs/gradle-configure-project.html).
