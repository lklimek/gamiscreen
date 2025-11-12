package ws.klimek.gamiscreen.pwashell

import android.webkit.JavascriptInterface
import ws.klimek.gamiscreen.core.SessionStore

/**
 * Exposed to the WebView so the PWA can notify native code about
 * updated auth tokens.
 */
class SessionTokenBridge(
    private val sessionStore: SessionStore
) {
    @JavascriptInterface
    fun persistAuthToken(token: String?) {
        sessionStore.persistAuthToken(token?.takeIf { it.isNotBlank() })
    }
}
