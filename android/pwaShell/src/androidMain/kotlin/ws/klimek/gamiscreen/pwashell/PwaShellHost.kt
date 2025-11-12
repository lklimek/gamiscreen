package ws.klimek.gamiscreen.pwashell

import android.annotation.SuppressLint
import android.webkit.ServiceWorkerController
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import ws.klimek.gamiscreen.core.AppConfigDefaults
import ws.klimek.gamiscreen.core.SessionStore
import org.json.JSONObject

private sealed interface ShellUiState {
    data object Loading : ShellUiState
    data object Content : ShellUiState
    data class Error(val description: String) : ShellUiState
}

object PwaShellDefaults {
    val defaultPwaUrl: String =
        buildString {
            append(AppConfigDefaults.Local.apiBaseUrl.trimEnd('/'))
            append('/')
        }
}

@SuppressLint("SetJavaScriptEnabled")
@Composable
fun PwaShellHost(
    modifier: Modifier = Modifier,
    startUrl: String = PwaShellDefaults.defaultPwaUrl
) {
    var uiState by remember { mutableStateOf<ShellUiState>(ShellUiState.Loading) }
    var reloadToken by remember { mutableIntStateOf(0) }
    var webView by remember { mutableStateOf<WebView?>(null) }
    val inPreview = LocalInspectionMode.current
    val appContext = LocalContext.current.applicationContext
    val sessionStore = remember(appContext) { SessionStore.getInstance(appContext) }

    Box(modifier = modifier.fillMaxSize()) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                configureServiceWorkers()
                WebView(context).apply {
                    addJavascriptInterface(SessionTokenBridge(sessionStore), TOKEN_BRIDGE_JS_NAME)
                    settings.applyWebDefaults()
                    webChromeClient = object : WebChromeClient() {
                        override fun onProgressChanged(view: WebView?, newProgress: Int) {
                            if (uiState !is ShellUiState.Error) {
                                uiState = if (newProgress >= 100) {
                                    ShellUiState.Content
                                } else {
                                    ShellUiState.Loading
                                }
                            }
                        }
                    }
                    webViewClient = object : WebViewClient() {
                        override fun onPageFinished(view: WebView?, url: String?) {
                            if (uiState !is ShellUiState.Error) {
                                uiState = ShellUiState.Content
                            }
                            view?.let {
                                injectTokenSyncScript(it)
                                sessionStore.currentAuthToken()?.let { token ->
                                    seedTokenIntoWebView(it, token)
                                }
                            }
                        }

                        override fun onReceivedError(
                            view: WebView?,
                            request: WebResourceRequest?,
                            error: WebResourceError?
                        ) {
                            if (request?.isForMainFrame == false) return
                            uiState = ShellUiState.Error(
                                description = error?.description?.toString()
                                    ?: "Unable to load gamiscreen right now."
                            )
                        }
                    }
                }
            },
            update = { view ->
                webView = view
            },
            onRelease = { it.destroy() }
        )

        when (val state = uiState) {
            ShellUiState.Content -> Unit
            ShellUiState.Loading -> if (!inPreview) {
                LoadingOverlay()
            } else {
                PreviewPlaceholder()
            }

            is ShellUiState.Error -> ErrorOverlay(
                message = state.description,
                onRetry = {
                    uiState = ShellUiState.Loading
                    reloadToken++
                }
            )
        }
    }

    LaunchedEffect(webView, startUrl, reloadToken, inPreview) {
        if (inPreview) return@LaunchedEffect
        webView?.loadUrl(startUrl)
    }
}

@Composable
private fun LoadingOverlay() {
    Column(
        modifier = Modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        CircularProgressIndicator()
        Spacer(modifier = Modifier.height(16.dp))
        Text(text = "Loading gamiscreen…", style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun PreviewPlaceholder() {
    Column(
        modifier = Modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text(
            text = "Preview mode",
            style = MaterialTheme.typography.titleMedium
        )
        Spacer(modifier = Modifier.height(8.dp))
        Text(
            text = "WebView is available on device builds.",
            style = MaterialTheme.typography.bodyMedium
        )
    }
}

@Composable
private fun ErrorOverlay(
    message: String,
    onRetry: () -> Unit
) {
    Column(
        modifier = Modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text(
            text = "Couldn't reach gamiscreen.",
            style = MaterialTheme.typography.titleMedium
        )
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = message,
            style = MaterialTheme.typography.bodyMedium
        )
        Spacer(modifier = Modifier.height(16.dp))
        Button(onClick = onRetry) {
            Text(text = "Retry")
        }
    }
}

private fun WebSettings.applyWebDefaults() {
    safeBrowsingEnabled = true
    javaScriptEnabled = true
    domStorageEnabled = true
    databaseEnabled = true
    cacheMode = WebSettings.LOAD_DEFAULT
    allowFileAccess = false
    builtInZoomControls = false
    displayZoomControls = false
    useWideViewPort = true
    loadWithOverviewMode = true
    mixedContentMode = WebSettings.MIXED_CONTENT_COMPATIBILITY_MODE
    userAgentString = buildString {
        val baseAgent = userAgentString
        append(baseAgent)
        append(" gamiscreen-android-shell")
    }
}

private fun configureServiceWorkers() {
    ServiceWorkerController.getInstance()
        .serviceWorkerWebSettings.apply {
            setAllowContentAccess(true)
            setAllowFileAccess(true)
            setBlockNetworkLoads(false)
        }
}

private const val TOKEN_BRIDGE_JS_NAME = "GamiscreenTokenBridge"
private const val TOKEN_STORAGE_KEY = "gamiscreen.token"

private fun injectTokenSyncScript(webView: WebView) {
    val script = """
        (function() {
            if (window.__gamiscreenTokenSyncInstalled) return;
            var bridge = window['$TOKEN_BRIDGE_JS_NAME'];
            if (!bridge || typeof bridge.persistAuthToken !== 'function') return;
            window.__gamiscreenTokenSyncInstalled = true;
            var TOKEN_KEY = '$TOKEN_STORAGE_KEY';
            function notify() {
                try {
                    var value = localStorage.getItem(TOKEN_KEY) || '';
                    bridge.persistAuthToken(value);
                } catch (err) {
                    console.error('Token sync notify failed', err);
                }
            }
            var origSetItem = localStorage.setItem;
            localStorage.setItem = function(key, value) {
                var result = origSetItem.apply(this, arguments);
                if (key === TOKEN_KEY) notify();
                return result;
            };
            var origRemoveItem = localStorage.removeItem;
            localStorage.removeItem = function(key) {
                var result = origRemoveItem.apply(this, arguments);
                if (key === TOKEN_KEY) notify();
                return result;
            };
            notify();
        })();
    """.trimIndent()
    webView.evaluateJavascript(script, null)
}

private fun seedTokenIntoWebView(webView: WebView, token: String) {
    val script = """
        (function() {
            try {
                localStorage.setItem('$TOKEN_STORAGE_KEY', ${token.toJsStringLiteral()});
            } catch (err) {
                console.error('Unable to seed auth token', err);
            }
        })();
    """.trimIndent()
    webView.evaluateJavascript(script, null)
}

private fun String.toJsStringLiteral(): String = JSONObject.quote(this)
