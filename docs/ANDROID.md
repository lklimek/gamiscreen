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
- `targetSdkVersion`: **36 (Android 15)** – matches Google Play's latest requirement for 2024+ and ensures compatibility with current privacy restrictions. (We'll revisit API 37 once tooling stabilizes.)
- Compile SDK: **36** – keeps parity with target SDK and ensures WebView/Material components match Chrome 124 equivalents.
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
- Use Jetpack/Compose Multiplatform for UI; rely on Material 3 components that can later target iOS. Kotlin compose compiler plugin is applied (`org.jetbrains.kotlin.plugin.compose`) per Kotlin 2.0 requirements. Current stack: Compose Multiplatform **1.9.3**, Kotlin **2.2.21**, Material **1.13.0**, AndroidX WebKit **1.14.0**.
- `pwaShell/` is a Kotlin Multiplatform module (Compose Multiplatform 1.9.3 on Kotlin 2.2.21) so shared UI can be reused by future iOS shells.
- Dependency injection will use **Hilt** (Dagger) for first-party Jetpack support, generated graphs, and better long-term maintainability.
- Define shared configuration (API host, feature flags) in `core`.
- Introduce strict lint/Detekt rules to match repository quality standards.

## Tooling, CI, and QA

- Add Android builds to existing CI pipeline:
  - `./gradlew lint`
  - `./gradlew test`
  - `./gradlew assembleDebug` (PR validation) and `bundleRelease` (release pipeline).
  - Repository shortcut: run `scripts/android_ci.sh` (expects Gradle wrapper to be generated).
- Standardize on OpenJDK **21.0.8** with Kotlin **2.2.21** / Compose Multiplatform **1.9.3**; use Gradle **9.2** (install via SDKMAN at `~/.sdkman/candidates/gradle/current/bin/gradle` and generate the wrapper with `gradle wrapper --gradle-version 9.2`).
- Configure static analysis: Detekt + Ktlint integration; fail the build on violations.
- Set up Firebase Crashlytics + Analytics (config placeholders until keys provided).
- Define signing strategy:
  - Debug keystore committed for local builds.
  - Release keystore stored in CI secrets; document manual signing fallback.
- Document manual QA checklists for WebView flows, lock behavior, and offline scenarios.

### WebView & PWA Debugging Notes

- `WebView.setWebContentsDebuggingEnabled(true)` is called by `PwaShellHost`, so Chrome DevTools (`chrome://inspect`) works on every debug build without extra flags. Use it to inspect `<dialog>` layout, console warnings, and Service Worker failures.
- Embedded PWA assets come from `gamiscreen-web/dist` (copied into `android/app/src/debug/assets`). Run `npm run build` before `./gradlew :app:assembleDebug` to refresh them.
- Service Worker scripts (`sw.js`, `notification-format.js`) are served from the embedded asset host `https://gamiscreen.klimek.ws/android-assets/`. Any failure to import notification helpers is logged to console from the SW itself—check logcat for `Service worker failed to load notification formatter`.

### Release Signing & GitHub Actions

Release builds use a real keystore injected via CI secrets. To configure:

1. Generate (or reuse) a signing keystore:
   ```bash
   keytool -genkeypair -v \
     -keystore gamiscreen-release.keystore \
     -alias gamiscreen-release \
     -keyalg RSA -keysize 4096 -validity 10000
   ```

2. Base64-encode the keystore so it can be stored as a GitHub secret:
   ```bash
   base64 -w0 gamiscreen-release.keystore > gamiscreen-release.keystore.b64
   ```

3. Add the following repository secrets (Settings → Secrets → Actions):

| Secret name                          | Value                                               |
| ------------------------------------ | --------------------------------------------------- |
| `ANDROID_KEYSTORE_BASE64`            | Contents of `gamiscreen-release.keystore.b64`       |
| `ANDROID_SIGNING_KEYSTORE_PASSWORD`  | Keystore password                                   |
| `ANDROID_SIGNING_KEY_ALIAS`          | Alias used when generating the key (e.g., `gamiscreen-release`) |
| `ANDROID_SIGNING_KEY_ALIAS_PASSWORD` | Key password                                        |

4. The workflow `.github/workflows/android-apk.yml` automatically:
   - Decodes the keystore into `android/release-signing.keystore`.
   - Exports the signing env vars for Gradle (`ANDROID_SIGNING_KEYSTORE`, etc.).
   - Builds `./scripts/android_ci.sh release`, which picks up those env vars and signs the APK.
   - Uploads the signed APK as an artifact and attaches it to releases.

Local release builds can set the same env vars before running `./scripts/android_ci.sh release` to reuse the CI keystore.

### Play Store Internal Testing Deployments

GitHub Releases now push the generated `.aab` straight to the Google Play Internal Testing track. To enable it:

1. Create a Google Cloud service account inside the same Google Cloud project that backs the Play Console:
   1. Open [console.cloud.google.com/iam-admin/serviceaccounts](https://console.cloud.google.com/iam-admin/serviceaccounts) and select the project that owns the Play application.
   2. Click **Create service account**, name it `gamiscreen-play-publisher`, and keep the description clear (e.g., "CI releases").
   3. You do not need to grant project-wide roles; leave the role picker empty so the account only acts through Play Console.
   4. Finish creation and, from the three-dot menu, choose **Manage keys → Add key → Create new key → JSON**. Download the JSON file—this becomes the GitHub secret.
2. Link the service account to Google Play Console:
   1. In Play Console visit **Setup → Developer account → API access**, click **Link service account**, and paste the service-account email.
   2. Grant the account the **Release Manager** role, enable access to `ws.klimek.gamiscreen.app`, and allow releasing to Internal Testing.
2. Generate a JSON key for that service account and store its contents in the repository secret `GOOGLE_PLAY_SERVICE_ACCOUNT_JSON`.
3. Ensure the Internal Testing track already has at least one tester list configured so uploaded builds become available.
4. When a GitHub Release is published, `.github/workflows/android-apk.yml` runs the `Publish to Google Play internal testing` step which uploads `bundleRelease` to the `internal` track with status `completed`.

You can trigger the same upload with `workflow_dispatch` by selecting the `release` build type once the secret is in place.

## Rust Integration Roadmap

- Build shared business logic as `libgamiscreen.so` via `cargo-ndk` targeting `armeabi-v7a`, `arm64-v8a`, and `x86_64`.
- Expose a minimal JNI/UniFFI surface:
  - Balance calculation utilities.
  - Countdown/lock orchestration states.
  - Request signing or crypto helpers if required.
- Kotlin coroutines should call into Rust on `Dispatchers.Default` to avoid blocking the main thread.
- Define error mapping between Rust `Result` types and Kotlin sealed results.
