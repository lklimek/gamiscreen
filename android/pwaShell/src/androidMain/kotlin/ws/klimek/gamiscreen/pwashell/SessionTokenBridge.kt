package ws.klimek.gamiscreen.pwashell

import android.util.Log
import android.webkit.JavascriptInterface
import ws.klimek.gamiscreen.core.SessionStore

class SessionTokenBridge(
    private val sessionStore: SessionStore,
    private val embeddedMode: Boolean,
    serverBaseUrl: String
) {
    private val normalizedServerBase = serverBaseUrl.trimEnd('/')
    private val tag = "SessionTokenBridge"

    @JavascriptInterface
    fun getAuthToken(): String? = sessionStore.currentAuthToken()

    @JavascriptInterface
    fun setAuthToken(token: String?) {
        Log.d(tag, "Persisting auth token from WebView (present=${!token.isNullOrBlank()})")
        sessionStore.persistAuthToken(token?.takeIf { it.isNotBlank() })
    }

    @JavascriptInterface
    fun isEmbeddedMode(): Boolean {
        Log.d(tag, "isEmbeddedMode -> $embeddedMode")
        return embeddedMode
    }

    @JavascriptInterface
    fun getServerBaseUrl(): String {
        Log.d(tag, "Providing server base URL $normalizedServerBase to WebView")
        return normalizedServerBase
    }
}
