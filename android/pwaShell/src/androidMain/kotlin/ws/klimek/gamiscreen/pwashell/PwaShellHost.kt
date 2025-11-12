package ws.klimek.gamiscreen.pwashell

import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.webkit.ServiceWorkerController
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Toast
import androidx.activity.compose.BackHandler
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalInspectionMode
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import java.util.Locale
import org.json.JSONObject
import ws.klimek.gamiscreen.core.AppConfigDefaults
import ws.klimek.gamiscreen.core.SessionStore

private sealed interface ShellUiState {
    data object Loading : ShellUiState
    data object Content : ShellUiState
    data class Error(val description: String, val isNetworkIssue: Boolean) : ShellUiState
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
    startUrl: String = PwaShellDefaults.defaultPwaUrl,
    embeddedContent: EmbeddedPwaContent? = null
) {
    var uiState by remember { mutableStateOf<ShellUiState>(ShellUiState.Loading) }
    var reloadToken by remember { mutableIntStateOf(0) }
    var webView by remember { mutableStateOf<WebView?>(null) }
    val inPreview = LocalInspectionMode.current
    var canNavigateBack by remember { mutableStateOf(false) }
    val context = LocalContext.current
    val appContext = context.applicationContext
    val sessionStore = remember(appContext) { SessionStore.getInstance(appContext) }
    val embeddedAssetsLoader = embeddedContent?.assetLoader
    val allowedHosts = remember(startUrl, embeddedContent) {
        buildSet {
            parseHost(PwaShellDefaults.defaultPwaUrl)?.let { add(it) }
            parseHost(startUrl)?.let { add(it) }
            embeddedContent?.host?.let { add(it) }
        }
    }

    Box(modifier = modifier.fillMaxSize()) {
        AndroidView(
            modifier = Modifier.fillMaxSize(),
            factory = { context ->
                configureServiceWorkers()
                WebView(context).apply {
                    addJavascriptInterface(
                        SessionTokenBridge(sessionStore),
                        NATIVE_BRIDGE_JS_NAME
                    )
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
                        override fun shouldInterceptRequest(
                            view: WebView?,
                            request: WebResourceRequest?
                        ): WebResourceResponse? {
                            val loader = embeddedAssetsLoader ?: return super.shouldInterceptRequest(view, request)
                            val uri = request?.url ?: return super.shouldInterceptRequest(view, request)
                            return if (embeddedContent?.host?.equals(uri.host ?: "", true) == true) {
                                loader.shouldInterceptRequest(uri)
                            } else {
                                super.shouldInterceptRequest(view, request)
                            }
                        }

                        override fun shouldOverrideUrlLoading(
                            view: WebView?,
                            request: WebResourceRequest?
                        ): Boolean {
                            return handleUrlOverride(context, request?.url, allowedHosts)
                        }

                        override fun shouldOverrideUrlLoading(view: WebView?, url: String?): Boolean {
                            val uri = url?.let { runCatching { Uri.parse(it) }.getOrNull() }
                            return handleUrlOverride(context, uri, allowedHosts)
                        }

                        override fun onPageFinished(view: WebView?, url: String?) {
                            if (uiState !is ShellUiState.Error) {
                                uiState = ShellUiState.Content
                            }
                            view?.let { web ->
                                if (embeddedContent != null) {
                                    seedServerBase(web, AppConfigDefaults.Local.apiBaseUrl)
                                }
                                canNavigateBack = web.canGoBack()
                            }
                        }

                        override fun onReceivedError(
                            view: WebView?,
                            request: WebResourceRequest?,
                            error: WebResourceError?
                        ) {
                            if (request?.isForMainFrame == false) return
                            canNavigateBack = view?.canGoBack() == true
                            uiState = ShellUiState.Error(
                                description = error?.description?.toString()
                                    ?: "Unable to load gamiscreen right now.",
                                isNetworkIssue = error.isConnectivityIssue()
                            )
                        }
                    }
                }
            },
            update = { view ->
                webView = view
                canNavigateBack = view.canGoBack()
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
                isOffline = state.isNetworkIssue,
                onRetry = {
                    uiState = ShellUiState.Loading
                    reloadToken++
                }
            )
        }
    }

    BackHandler(enabled = canNavigateBack) {
        webView?.goBack()
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
    isOffline: Boolean,
    onRetry: () -> Unit
) {
    Column(
        modifier = Modifier.fillMaxSize(),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text(
            text = if (isOffline) "You're offline." else "Couldn't reach gamiscreen.",
            style = MaterialTheme.typography.titleMedium
        )
        Spacer(modifier = Modifier.height(4.dp))
        Text(
            text = if (isOffline) "Check your connection and try again." else message,
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

private const val NATIVE_BRIDGE_JS_NAME = "__gamiscreenNative"
private const val SERVER_BASE_KEY = "gamiscreen.server_base"

private fun parseHost(url: String): String? = runCatching {
    Uri.parse(url).host?.lowercase(Locale.US)
}.getOrNull()

private fun handleUrlOverride(
    context: Context,
    uri: Uri?,
    allowedHosts: Set<String>
): Boolean {
    val normalizedHost = uri?.host?.lowercase(Locale.US)
    val scheme = uri?.scheme?.lowercase(Locale.US)
    if (uri != null && scheme in setOf("http", "https") && normalizedHost != null && normalizedHost in allowedHosts) {
        return false
    }

    val target = uri ?: return false
    val intent = Intent(Intent.ACTION_VIEW, target).apply {
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    return try {
        context.startActivity(intent)
        true
    } catch (ex: ActivityNotFoundException) {
        Toast.makeText(context, "No app available to open this link.", Toast.LENGTH_SHORT).show()
        true
    }
}

private fun WebResourceError?.isConnectivityIssue(): Boolean {
    val code = this?.errorCode ?: return false
    return code == WebViewClient.ERROR_CONNECT ||
        code == WebViewClient.ERROR_HOST_LOOKUP ||
        code == WebViewClient.ERROR_TIMEOUT ||
        code == WebViewClient.ERROR_UNKNOWN
}

private fun seedServerBase(webView: WebView, baseUrl: String) {
    val script = """
        (function() {
            try {
                localStorage.setItem('$SERVER_BASE_KEY', ${baseUrl.toJsStringLiteral()});
            } catch (err) {
                console.error('Unable to seed server base', err);
            }
        })();
    """.trimIndent()
    webView.evaluateJavascript(script, null)
}

private fun String.toJsStringLiteral(): String = JSONObject.quote(this)
