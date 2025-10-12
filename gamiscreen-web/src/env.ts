export function getVapidPublicKey(): string | undefined {
  const env = (import.meta as any).env ?? {};
  const fromEnv = env.VITE_VAPID_PUB_KEY;
  if (typeof fromEnv === 'string' && fromEnv.trim().length > 0) {
    return fromEnv;
  }
  if (typeof window !== 'undefined') {
    const win = window as typeof window & { gamiscreenVapidPublicKey?: string };
    const fromWindow = win.gamiscreenVapidPublicKey;
    if (typeof fromWindow === 'string' && fromWindow.trim().length > 0) {
      return fromWindow;
    }
  }
  return undefined;
}

export function isWebPushConfigured(): boolean {
  return typeof getVapidPublicKey() === 'string';
}
