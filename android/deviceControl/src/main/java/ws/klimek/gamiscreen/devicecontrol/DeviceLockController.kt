package ws.klimek.gamiscreen.devicecontrol

import kotlinx.coroutines.delay

/**
 * Placeholder for the device policy manager integration.
 *
 * The implementation will eventually call into Android's DevicePolicyManager
 * after the app is enrolled as a device owner. For now it provides async stubs
 * so callers can be wired without blocking.
 */
class DeviceLockController {
    suspend fun lockScreen() {
        // TODO: replace with DevicePolicyManager#lockNow()
        delay(0)
    }

    suspend fun unlockScreen() {
        // TODO: replace with DevicePolicyManager APIs when available.
        delay(0)
    }
}
