package ws.klimek.gamiscreen.pwashell

import android.webkit.JavascriptInterface
import ws.klimek.gamiscreen.core.SessionStore

class SessionTokenBridge(
    private val sessionStore: SessionStore
) {
    @JavascriptInterface
    fun getAuthToken(): String? = sessionStore.currentAuthToken()

    @JavascriptInterface
    fun setAuthToken(token: String?) {
        sessionStore.persistAuthToken(token?.takeIf { it.isNotBlank() })
    }
}
