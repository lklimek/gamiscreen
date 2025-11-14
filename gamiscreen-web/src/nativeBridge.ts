export type NativeBridge = {
  getAuthToken: () => string | null | undefined
  setAuthToken: (token: string | null) => void
  isEmbeddedMode?: () => boolean
  getServerBaseUrl?: () => string | null | undefined
}

declare global {
  interface Window {
    __gamiscreenNative?: Partial<NativeBridge>
  }
}

export function getNativeBridge(): NativeBridge | null {
  if (typeof window === 'undefined') return null
  const bridge = window.__gamiscreenNative
  if (!bridge) return null
  if (typeof bridge.getAuthToken !== 'function') return null
  if (typeof bridge.setAuthToken !== 'function') return null
  return bridge as NativeBridge
}

export function isRunningInNativeShell(): boolean {
  return getNativeBridge() !== null
}

export function isEmbeddedMode(): boolean {
  const bridge = getNativeBridge()
  if (!bridge) return false
  try {
    return !!bridge.isEmbeddedMode?.()
  } catch {
    return false
  }
}

export function getNativeServerBase(): string | null {
  const bridge = getNativeBridge()
  if (!bridge) return null
  try {
    const url = bridge.getServerBaseUrl?.()
    if (typeof url === 'string' && url.trim().length > 0) {
      return url.trim()
    }
  } catch (err) {
    console.warn('native bridge getServerBaseUrl failed', err)
  }
  return null
}
