export type PermissionState = NotificationPermission | 'unsupported';
export type NotificationSettings = {
  enabled: boolean;
};

const formatterGlobal = () =>
  (typeof window !== 'undefined' &&
    (window as typeof window & {
      __gamiscreenFormatNotification?: (payload: any) => { title?: string; body?: string } | null;
    }).__gamiscreenFormatNotification) || null;

const SETTINGS_KEY = 'gamiscreen.notifications.settings';
const DEFAULT_SETTINGS: NotificationSettings = { enabled: false };
let requestingPermission = false;

let cachedSettings: NotificationSettings = readSettings();

function readSettings(): NotificationSettings {
  if (typeof window === 'undefined') return { ...DEFAULT_SETTINGS };
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw);
    return {
      enabled: parsed?.enabled === true,
    };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function persistSettings(settings: NotificationSettings) {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // ignore storage errors
  }
}

if (typeof window !== 'undefined') {
  window.addEventListener('storage', (event) => {
    if (event.key === SETTINGS_KEY) {
      cachedSettings = readSettings();
      window.dispatchEvent(
        new CustomEvent('gamiscreen:notification-settings-changed', {
          detail: getNotificationSettings(),
        }),
      );
    }
  });
}

export function getNotificationSettings(): NotificationSettings {
  return { ...cachedSettings };
}

export function saveNotificationSettings(settings: NotificationSettings) {
  cachedSettings = {
    enabled: !!settings.enabled,
  };
  persistSettings(cachedSettings);
  if (typeof window !== 'undefined') {
    window.dispatchEvent(
      new CustomEvent('gamiscreen:notification-settings-changed', {
        detail: getNotificationSettings(),
      }),
    );
  }
}

export function base64UrlToUint8Array(base64String: string): Uint8Array {
  const padding = '='.repeat((4 - (base64String.length % 4)) % 4);
  const base64 = (base64String + padding).replace(/-/g, '+').replace(/_/g, '/');
  const raw = atob(base64);
  const buffer = new ArrayBuffer(raw.length);
  const output = new Uint8Array(buffer);
  for (let i = 0; i < raw.length; i += 1) {
    output[i] = raw.charCodeAt(i);
  }
  return output;
}

function hasWindow(): boolean {
  return typeof window !== 'undefined';
}

function hasNavigator(): boolean {
  return typeof navigator !== 'undefined';
}

export function supportsNotifications(): boolean {
  return hasWindow() && 'Notification' in window;
}

export function currentNotificationPermission(): PermissionState {
  if (!supportsNotifications()) return 'unsupported';
  return Notification.permission;
}

export async function requestNotificationPermission(): Promise<PermissionState> {
  if (!supportsNotifications()) return 'unsupported';
  if (Notification.permission !== 'default') {
    return Notification.permission;
  }
  if (requestingPermission) {
    return Notification.permission;
  }
  requestingPermission = true;
  try {
    const result = await Notification.requestPermission();
    return result;
  } finally {
    requestingPermission = false;
  }
}

export { getVapidPublicKey, isWebPushConfigured } from './env';
