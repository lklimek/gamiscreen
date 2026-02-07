package ws.klimek.gamiscreen.core

/**
 * Central configuration shared across Android modules.
 *
 * This will eventually source values from persisted storage
 * or remote configuration; for now it provides sensible defaults
 * so the app can compile and run in a local environment.
 */
data class AppConfig(
    val apiBaseUrl: String,
    val useStagingBackend: Boolean = false
)

object AppConfigDefaults {
    val Local = AppConfig(apiBaseUrl = "https://gamiscreen.klimek.ws:443")
}
