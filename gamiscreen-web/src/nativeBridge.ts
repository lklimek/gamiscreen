export type NativeBridge = {
  getAuthToken: () => string | null | undefined
  setAuthToken: (token: string | null) => void
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
