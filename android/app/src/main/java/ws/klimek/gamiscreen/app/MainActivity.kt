package ws.klimek.gamiscreen.app

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.runtime.collectAsState
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import kotlinx.coroutines.flow.MutableStateFlow
import ws.klimek.gamiscreen.pwashell.PwaShellDefaults
import ws.klimek.gamiscreen.pwashell.PwaShellHost

class MainActivity : ComponentActivity() {

    private val incomingDeepLinks = MutableStateFlow<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        handleIntent(intent)
        setContent {
            val pendingDeepLink = incomingDeepLinks.collectAsState().value
            GamiscreenApp(pendingDeepLink = pendingDeepLink)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handleIntent(intent)
    }

    private fun handleIntent(intent: Intent?) {
        val data = intent?.dataString ?: return
        incomingDeepLinks.value = data
    }
}

@Composable
fun GamiscreenApp(pendingDeepLink: String? = null) {
    MaterialTheme {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = MaterialTheme.colorScheme.background
        ) {
            PwaShellHost(startUrl = pendingDeepLink ?: PwaShellDefaults.defaultPwaUrl)
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun GamiscreenAppPreview() {
    GamiscreenApp()
}
