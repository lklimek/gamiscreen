package ws.klimek.gamiscreen.pwashell

import android.content.Context
import android.content.res.AssetManager
import androidx.webkit.WebViewAssetLoader

data class EmbeddedPwaContent(
    val rootUrl: String,
    val host: String,
    val assetLoader: WebViewAssetLoader
) {
    companion object {
        private const val HOST = "appassets.androidplatform.net"
        private const val INDEX_PATH = "index.html"

        fun fromAssets(context: Context): EmbeddedPwaContent? {
            if (!assetExists(context.assets, INDEX_PATH)) {
                return null
            }
            val loader = WebViewAssetLoader.Builder()
                .setDomain(HOST)
                .addPathHandler("/", WebViewAssetLoader.AssetsPathHandler(context))
                .build()

            return EmbeddedPwaContent(
                rootUrl = "https://$HOST/$INDEX_PATH",
                host = HOST,
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
