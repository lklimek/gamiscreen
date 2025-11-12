package ws.klimek.gamiscreen.core

import android.content.Context
import androidx.core.content.edit
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKeys

/**
 * Persists auth/session tokens obtained via the WebView PWA so that
 * native components (future lock service, workers) can reuse them.
 */
class SessionStore private constructor(context: Context) {

    private val masterKeyAlias = MasterKeys.getOrCreate(MasterKeys.AES256_GCM_SPEC)

    private val prefs = EncryptedSharedPreferences.create(
        PREFS_NAME,
        masterKeyAlias,
        context,
        EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
        EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
    )

    fun persistAuthToken(token: String?) {
        prefs.edit {
            if (token.isNullOrBlank()) {
                remove(KEY_AUTH_TOKEN)
            } else {
                putString(KEY_AUTH_TOKEN, token)
            }
        }
    }

    fun currentAuthToken(): String? = prefs.getString(KEY_AUTH_TOKEN, null)

    companion object {
        private const val PREFS_NAME = "gamiscreen_session"
        private const val KEY_AUTH_TOKEN = "auth_token"

        @Volatile
        private var instance: SessionStore? = null

        fun getInstance(context: Context): SessionStore {
            return instance ?: synchronized(this) {
                instance ?: SessionStore(context.applicationContext).also { instance = it }
            }
        }
    }
}
