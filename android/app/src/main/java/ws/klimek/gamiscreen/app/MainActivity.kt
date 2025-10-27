package ws.klimek.gamiscreen.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import ws.klimek.gamiscreen.pwashell.PwaShellHost

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent { GamiscreenApp() }
    }
}

@Composable
fun GamiscreenApp() {
    MaterialTheme {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = MaterialTheme.colorScheme.background
        ) {
            PwaShellHost()
        }
    }
}

@Preview(showBackground = true)
@Composable
private fun GamiscreenAppPreview() {
    GamiscreenApp()
}
