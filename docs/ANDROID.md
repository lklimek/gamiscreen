# Android App Foundations

This document captures the initial technical decisions and open questions for the native Android client. It aligns with the `Foundations` task group in `TODO.md` and should be updated as requirements evolve.

## Product Scope & Assumptions

- Primary objective: deliver a native shell for the existing gamiscreen PWA, then add device lock enforcement similar to the Linux agent.
- Initial release focuses on family-managed devices (phones/tablets used by children). Corporate/MDM scenarios stay out of scope for now.
- Each managed Android device is personal to a single child; parents use their own devices only for management actions such as rewarding time.
- Authentication, task management, and minute accounting continue to flow through existing backend APIs.
- Offline tolerance should match the Linux client: lock after configurable grace period when the backend is unreachable.
- Devices are dedicated family hardware; assume full administrative access (device-owner capable) with no BYOD constraints.
- Distribution starts via side-loaded APKs; Play Store submission can be revisited after field validation.
- WearOS/TV support is out of scope.
- Hardware buttons will not be locked (e.g., volume buttons).
- No Kiosk mode implementation.

## Platform Targets

- `minSdkVersion`: **31 (Android 12)** – aligns with modern WebView/privacy requirements, grants access to updated Device Policy APIs, and reduces legacy testing burden. Devices below Android 12 are out of scope.
- `targetSdkVersion`: **34 (Android 14)** – required for Play Store by 2024 and ensures compatibility with current privacy restrictions.
- Compile SDK: 34 (Android 14).
- Build variants:
  - `debug`: developer settings, mock WebView URL override, verbose logging.
  - `release`: Play-ready, ProGuard/R8 enabled, Crashlytics active.

## Device Support Matrix

| Form Factor                 | Notes                                        |
| --------------------------- | -------------------------------------------- |
| Phones (5–6.5")             | Primary target; portrait-first UI.           |
| Small tablets (7–9")        | Ensure layouts scale; allow landscape usage. |
| Large tablets / Chromebooks | Treat as stretch goal.                       |

## Project & Module Layout

```
android/
 └─ app/               # Application module; manifest, navigation, DI wiring
 ├─ pwaShell/          # WebView wrapper, Compose UI, JS bridge
 ├─ core/              # Shared Kotlin utilities, config models, logging
 └─ deviceControl/     # Future device-admin APIs, lock service, background workers
```

- Keep Gradle version catalog (`gradle/libs.versions.toml`) at the root for dependency management.
- Use Jetpack Compose for UI; rely on Material 3 components.
- Dependency injection will use **Hilt** (Dagger) for first-party Jetpack support, generated graphs, and better long-term maintainability.
- Define shared configuration (API host, feature flags) in `core`.
- Introduce strict lint/Detekt rules to match repository quality standards.

## Tooling, CI, and QA

- Add Android builds to existing CI pipeline:
  - `./gradlew lint`
  - `./gradlew test`
  - `./gradlew assembleDebug` (PR validation) and `bundleRelease` (release pipeline).
- Standardize on OpenJDK **21.0.8** for local builds and CI runners (AGP 8.6+ compatible).
- Configure static analysis: Detekt + Ktlint integration; fail the build on violations.
- Set up Firebase Crashlytics + Analytics (config placeholders until keys provided).
- Define signing strategy:
  - Debug keystore committed for local builds.
  - Release keystore stored in CI secrets; document manual signing fallback.
- Document manual QA checklists for WebView flows, lock behavior, and offline scenarios.

## Rust Integration Roadmap

- Build shared business logic as `libgamiscreen.so` via `cargo-ndk` targeting `armeabi-v7a`, `arm64-v8a`, and `x86_64`.
- Expose a minimal JNI/UniFFI surface:
  - Balance calculation utilities.
  - Countdown/lock orchestration states.
  - Request signing or crypto helpers if required.
- Kotlin coroutines should call into Rust on `Dispatchers.Default` to avoid blocking the main thread.
- Define error mapping between Rust `Result` types and Kotlin sealed results.
