package ws.klimek.gamiscreen.core

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Log
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.KeyStoreException
import java.security.ProviderException
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Persists WebView session tokens encrypted with a non-exportable Android Keystore key. */
class SessionStore internal constructor(
    private val readRecord: () -> String?,
    private val writeRecord: (String?) -> Unit,
    private val key: (create: Boolean) -> SecretKey?,
    private val onFailure: () -> Unit
) {
    @Synchronized
    fun persistAuthToken(token: String?) {
        recover(Unit) {
            if (token.isNullOrBlank()) {
                writeRecord(null)
            } else {
                val cipher = Cipher.getInstance(TRANSFORMATION)
                cipher.init(
                    Cipher.ENCRYPT_MODE,
                    key(true) ?: throw KeyStoreException("Session key unavailable")
                )
                cipher.updateAAD(ASSOCIATED_DATA)
                require(cipher.iv.size == IV_SIZE)
                val encrypted = cipher.doFinal(token.toByteArray(Charsets.UTF_8))
                val record = byteArrayOf(FORMAT_VERSION) + cipher.iv + encrypted
                writeRecord(Base64.getEncoder().encodeToString(record))
            }
        }
    }

    @Synchronized
    fun currentAuthToken(): String? = recover(null) {
        val encoded = readRecord() ?: return@recover null
        val record = Base64.getDecoder().decode(encoded)
        require(record.size > 1 + IV_SIZE + TAG_BITS / 8 && record[0] == FORMAT_VERSION)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(
            Cipher.DECRYPT_MODE,
            key(false) ?: throw KeyStoreException("Session key unavailable"),
            GCMParameterSpec(TAG_BITS, record, 1, IV_SIZE)
        )
        cipher.updateAAD(ASSOCIATED_DATA)
        cipher.doFinal(record, 1 + IV_SIZE, record.size - 1 - IV_SIZE).toString(Charsets.UTF_8)
    }

    private fun <T> recover(fallback: T, operation: () -> T): T {
        try {
            return operation()
        } catch (_: GeneralSecurityException) {
            // Missing keys and authentication failures require a new login.
        } catch (_: IOException) {
            // Storage or the Keystore service may be unavailable.
        } catch (_: IllegalArgumentException) {
            // Reject malformed or unsupported stored records.
        } catch (_: ProviderException) {
            // Keystore provider failures must not crash the WebView bridge.
        }
        onFailure()
        try {
            writeRecord(null)
        } catch (_: IOException) {
            // The failure was reported above; never fall back to plaintext.
        }
        return fallback
    }

    companion object {
        private const val PREFS_NAME = "gamiscreen_session_v2"
        private const val KEY_AUTH_TOKEN = "auth_token"
        private const val KEY_ALIAS = "gamiscreen_session_v2"
        private const val KEYSTORE = "AndroidKeyStore"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val FORMAT_VERSION: Byte = 1
        private const val IV_SIZE = 12
        private const val TAG_BITS = 128
        private val ASSOCIATED_DATA = "$PREFS_NAME:$KEY_AUTH_TOKEN:$FORMAT_VERSION".toByteArray(Charsets.UTF_8)

        @Volatile
        private var instance: SessionStore? = null

        fun getInstance(context: Context): SessionStore = instance ?: synchronized(this) {
            instance ?: create(context.applicationContext).also { instance = it }
        }

        private fun create(context: Context): SessionStore {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            return SessionStore(
                readRecord = {
                    val value = prefs.all[KEY_AUTH_TOKEN]
                    require(value == null || value is String)
                    value
                },
                writeRecord = { record ->
                    val editor = prefs.edit()
                    if (record == null) {
                        editor.remove(KEY_AUTH_TOKEN)
                    } else {
                        editor.putString(KEY_AUTH_TOKEN, record)
                    }
                    if (!editor.commit()) throw IOException("Session storage unavailable")
                },
                key = { create -> sessionKey(create) },
                onFailure = { Log.w("SessionStore", "Unable to use the saved session; sign in again.") }
            )
        }

        private fun sessionKey(create: Boolean): SecretKey? {
            val store = KeyStore.getInstance(KEYSTORE).apply { load(null) }
            if (store.containsAlias(KEY_ALIAS)) {
                return store.getKey(KEY_ALIAS, null) as? SecretKey
                    ?: throw KeyStoreException("Session key unavailable")
            }
            if (!create) return null
            return KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE).apply {
                init(
                    KeyGenParameterSpec.Builder(
                        KEY_ALIAS,
                        KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
                    )
                        .setKeySize(256)
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .setRandomizedEncryptionRequired(true)
                        .build()
                )
            }.generateKey()
        }
    }
}
