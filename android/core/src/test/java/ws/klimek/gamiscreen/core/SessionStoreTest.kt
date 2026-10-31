package ws.klimek.gamiscreen.core

import java.io.IOException
import java.security.GeneralSecurityException
import java.security.ProviderException
import java.util.Base64
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SessionStoreTest {
    private class Fixture {
        var record: String? = null
        var secret: SecretKey? = newKey()
        val keyRequests = mutableListOf<Boolean>()
        var failures = 0
        var keyUnavailable = false
        var storageUnavailable = false
        var readUnavailable = false

        fun store() = SessionStore(
            readRecord = {
                if (readUnavailable) throw IOException("test storage unavailable")
                record
            },
            writeRecord = {
                if (storageUnavailable) throw IOException("test storage unavailable")
                record = it
            },
            key = { create ->
                keyRequests += create
                if (keyUnavailable) throw GeneralSecurityException("test key unavailable")
                secret
            },
            onFailure = { failures++ }
        )
    }

    @Test
    fun emptyStoreAndLogoutDoNotAccessTheKey() {
        val fixture = Fixture()
        val store = fixture.store()
        assertNull(store.currentAuthToken())
        store.persistAuthToken(null)
        store.persistAuthToken("  ")
        assertNull(fixture.record)
        assertTrue(fixture.keyRequests.isEmpty())
    }

    @Test
    fun tokenSurvivesReopeningOnlyAsCiphertext() {
        val fixture = Fixture()
        val token = "fake-token-秘密-with-enough-content-to-inspect"
        fixture.store().persistAuthToken(token)
        val record = assertNotNull(fixture.record)
        assertFalse(record.contains(token))
        assertFalse(Base64.getDecoder().decode(record).toString(Charsets.UTF_8).contains(token))
        assertEquals(token, fixture.store().currentAuthToken())
        assertEquals(listOf(true, false), fixture.keyRequests)
    }

    @Test
    fun eachWriteUsesAFreshNonce() {
        val fixture = Fixture()
        val store = fixture.store()
        store.persistAuthToken("fake-token")
        val first = assertNotNull(fixture.record)
        store.persistAuthToken("fake-token")
        assertNotEquals(first, fixture.record)
        val firstIv = Base64.getDecoder().decode(first).slice(1..12)
        val secondIv = Base64.getDecoder().decode(fixture.record).slice(1..12)
        assertNotEquals(firstIv, secondIv)
        assertEquals("fake-token", store.currentAuthToken())
    }

    @Test
    fun tamperedCiphertextIsClearedAndNeverReturned() {
        for (offset in listOf(1, 13, 30)) {
            val fixture = Fixture()
            fixture.store().persistAuthToken("fake-token")
            val bytes = Base64.getDecoder().decode(fixture.record)
            bytes[offset] = (bytes[offset].toInt() xor 1).toByte()
            fixture.record = Base64.getEncoder().encodeToString(bytes)
            assertNull(fixture.store().currentAuthToken())
            assertNull(fixture.record)
            assertEquals(1, fixture.failures)
        }
    }

    @Test
    fun wrongKeyCannotDecryptTheToken() {
        val fixture = Fixture()
        fixture.store().persistAuthToken("fake-token")
        fixture.secret = newKey()
        assertNull(fixture.store().currentAuthToken())
        assertNull(fixture.record)
        assertEquals(1, fixture.failures)
    }

    @Test
    fun missingKeyClearsTheTokenWithoutCreatingAReplacement() {
        val fixture = Fixture()
        fixture.store().persistAuthToken("fake-token")
        fixture.secret = null
        fixture.keyRequests.clear()
        assertNull(fixture.store().currentAuthToken())
        assertNull(fixture.record)
        assertEquals(listOf(false), fixture.keyRequests)
    }

    @Test
    fun malformedOrUnknownRecordsAreRejectedBeforeKeyAccess() {
        val malformed = listOf("not base64!", "", "AA==", Base64.getEncoder().encodeToString(ByteArray(64)))
        for (record in malformed) {
            val fixture = Fixture().apply { this.record = record }
            assertNull(fixture.store().currentAuthToken())
            assertNull(fixture.record)
            assertTrue(fixture.keyRequests.isEmpty())
            assertEquals(1, fixture.failures)
        }
    }

    @Test
    fun logoutRemovesASavedToken() {
        for (logout in listOf(null, "", "  ")) {
            val fixture = Fixture()
            val store = fixture.store()
            store.persistAuthToken("fake-token")
            fixture.keyRequests.clear()
            store.persistAuthToken(logout)
            assertNull(fixture.record)
            assertNull(store.currentAuthToken())
            assertTrue(fixture.keyRequests.isEmpty())
        }
    }

    @Test
    fun encryptionFailureClearsAnOldTokenWithoutPersistingPlaintext() {
        val fixture = Fixture()
        val store = fixture.store()
        store.persistAuthToken("old-fake-token")
        fixture.keyUnavailable = true
        store.persistAuthToken("new-fake-token")
        assertNull(fixture.record)
        assertEquals(1, fixture.failures)
    }

    @Test
    fun unavailableStorageDoesNotCrashTheBridge() {
        val fixture = Fixture().apply { storageUnavailable = true }
        fixture.store().persistAuthToken("fake-token")
        assertNull(fixture.record)
        assertEquals(1, fixture.failures)
    }

    @Test
    fun unreadableStorageReturnsNoToken() {
        val fixture = Fixture().apply { readUnavailable = true }
        assertNull(fixture.store().currentAuthToken())
        assertEquals(1, fixture.failures)
        assertTrue(fixture.keyRequests.isEmpty())
    }

    @Test
    fun providerFailureDoesNotEscapeTheBridge() {
        var record: String? = null
        var failures = 0
        val store = SessionStore(
            readRecord = { record },
            writeRecord = { record = it },
            key = { throw ProviderException("test provider failure") },
            onFailure = { failures++ }
        )
        store.persistAuthToken("fake-token")
        assertNull(record)
        assertEquals(1, failures)
    }

    companion object {
        private fun newKey(): SecretKey = KeyGenerator.getInstance("AES")
            .apply { init(256) }
            .generateKey()
    }
}
