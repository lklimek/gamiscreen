package ws.klimek.gamiscreen.pwashell

import android.content.Context
import android.content.res.AssetManager
import androidx.webkit.WebViewAssetLoader

data class EmbeddedPwaContent(
    val rootUrl: String,
    val host: String,
    val pathPrefix: String,
    val assetLoader: WebViewAssetLoader
) {
    companion object {
        private const val HOST = "gamiscreen.klimek.ws"
        private const val PATH_PREFIX = "/android-assets/"
        private const val INDEX_FILE = "index.html"

        fun fromAssets(context: Context): EmbeddedPwaContent? {
            if (!assetExists(context.assets, INDEX_FILE)) {
                return null
            }
            val loader = WebViewAssetLoader.Builder()
                .setDomain(HOST)
                .addPathHandler(PATH_PREFIX, WebViewAssetLoader.AssetsPathHandler(context))
                .build()

            return EmbeddedPwaContent(
                rootUrl = "https://$HOST$PATH_PREFIX$INDEX_FILE",
                host = HOST,
                pathPrefix = PATH_PREFIX,
                assetLoader = loader
            )
        }

        private fun assetExists(assetManager: AssetManager, path: String): Boolean =
            runCatching {
                assetManager.open(path).close()
                true
            }.getOrDefault(false)
    }
}
