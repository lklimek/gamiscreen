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

`Cargo.lock` at the workspace root is authoritative; the obsolete server-local lockfile is removed. `serde_yaml` remains at its last published release. The keyring 4 default `v1` API selects Windows Credential Manager or Linux Secret Service, retaining service/user credential lookup.

## Upstream audit gaps

`cargo audit` still reports `RUSTSEC-2026-0387` in `crypto-bigint` 0.5.5 (through elliptic-curve dependencies of the latest JWT/Web Push crates) and `RUSTSEC-2023-0071` in `rsa` 0.9.10 (no patched version). Both were already present in the base lockfile. These cannot be removed by a compatible lockfile refresh. There are also unmaintained transitive D-Bus/serialization packages and `unsafe-libyaml`.

TODO: migrate upstream JWT/Web Push cryptography dependencies when compatible patched releases become available, and replace the deprecated YAML parser in a separate compatibility-tested migration. Do not suppress these audit findings.

Primary references: [Rust channel manifest](https://static.rust-lang.org/dist/channel-rust-stable.toml), [Node releases](https://nodejs.org/dist/index.json), [Java LTS roadmap](https://www.oracle.com/java/technologies/java-se-support-roadmap.html), [Gradle release](https://services.gradle.org/versions/current), [AGP migration](https://developer.android.com/build/migrate-to-built-in-kotlin), [KMP plugin](https://developer.android.com/kotlin/multiplatform/plugin), [Kotlin compatibility](https://kotlinlang.org/docs/gradle-configure-project.html).
