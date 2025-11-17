import { TOKEN_KEY, PWA_VERSION_KEY } from './storageKeys'
const CACHE_PREFIXES = ['gamiscreen-', 'workbox-']

export function ensurePwaFreshness(currentVersion: string): void {
  if (typeof window === 'undefined') return

  let previousVersion: string | null = null
  try {
    previousVersion = window.localStorage.getItem(PWA_VERSION_KEY)
  } catch {
    // ignore storage access errors
  }

  if (previousVersion === currentVersion) {
    return
  }

  let preservedToken: string | null = null
  try {
    preservedToken = window.localStorage.getItem(TOKEN_KEY)
  } catch {
    preservedToken = null
  }

  try {
    window.localStorage.clear()
  } catch (err) {
    console.warn('[pwa] failed to clear storage during version refresh', err)
  }

  try {
    if (preservedToken) {
      window.localStorage.setItem(TOKEN_KEY, preservedToken)
    }
    window.localStorage.setItem(PWA_VERSION_KEY, currentVersion)
  } catch (err) {
    console.warn('[pwa] failed to restore preserved data', err)
  }

  if ('caches' in window) {
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => CACHE_PREFIXES.some((prefix) => key.startsWith(prefix)))
            .map((key) => caches.delete(key))
        )
      )
      .catch((err) => console.warn('[pwa] cache cleanup failed', err))
  }

  if ('serviceWorker' in navigator) {
    navigator.serviceWorker
      .getRegistrations()
      .then((regs) => regs.forEach((reg) => reg.update()))
      .catch((err) => console.warn('[pwa] service worker refresh failed', err))
  }
}
