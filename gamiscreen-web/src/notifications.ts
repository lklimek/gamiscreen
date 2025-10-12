export type PermissionState = NotificationPermission | 'unsupported';
export type NotificationSettings = {
  enabled: boolean;
  thresholdMinutes: number;
};

const notifiedChildren = new Map<string, boolean>();
let requestingPermission = false;
const SETTINGS_KEY = 'gamiscreen.notifications.settings';
const DEFAULT_SETTINGS: NotificationSettings = { enabled: false, thresholdMinutes: 5 };

let cachedSettings: NotificationSettings = readSettings();

function readSettings(): NotificationSettings {
  if (typeof window === 'undefined') return { ...DEFAULT_SETTINGS };
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (!raw) return { ...DEFAULT_SETTINGS };
    const parsed = JSON.parse(raw);
    const enabled = parsed?.enabled === true;
    const threshold = Number(parsed?.thresholdMinutes);
    return {
      enabled,
      thresholdMinutes: Number.isFinite(threshold) && threshold > 0 ? Math.round(threshold) : DEFAULT_SETTINGS.thresholdMinutes,
    };
  } catch {
    return { ...DEFAULT_SETTINGS };
  }
}

function persistSettings(settings: NotificationSettings) {
  if (typeof window === 'undefined') return;
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch { /* ignore */ }
}

if (typeof window !== 'undefined') {
  window.addEventListener('storage', (event) => {
    if (event.key === SETTINGS_KEY) {
      cachedSettings = readSettings();
      notifiedChildren.clear();
      window.dispatchEvent(new CustomEvent('gamiscreen:notification-settings-changed', { detail: getNotificationSettings() }));
    }
  });
}

export function getNotificationSettings(): NotificationSettings {
  return { ...cachedSettings };
}

export function saveNotificationSettings(settings: NotificationSettings) {
  cachedSettings = {
    enabled: !!settings.enabled,
    thresholdMinutes: Math.max(1, Math.round(settings.thresholdMinutes)),
  };
  persistSettings(cachedSettings);
  notifiedChildren.clear();
  if (typeof window !== 'undefined') {
    window.dispatchEvent(new CustomEvent('gamiscreen:notification-settings-changed', { detail: getNotificationSettings() }));
  }
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

function notificationTitle(displayName: string | undefined, minutes: number): string {
  const namePart = displayName ? `${displayName}` : 'Remaining time';
  if (minutes === 1) {
    return `${namePart}: 1 minute left`;
  }
  return `${namePart}: ${minutes} minutes left`;
}

function notificationBody(minutes: number): string {
  if (minutes === 1) {
    return 'Only 1 minute of screen time remains. Wrap up or ask for more time.';
  }
  return `Only ${minutes} minutes of screen time remain. Please get ready to finish up.`;
}

export async function maybeNotifyRemaining(
  childId: string,
  minutes: number,
  displayName?: string,
): Promise<void> {
  if (!supportsNotifications()) return;

  const settings = getNotificationSettings();
  if (!settings.enabled) {
    notifiedChildren.delete(childId);
    return;
  }

  const threshold = Math.max(1, settings.thresholdMinutes);

  if (minutes <= 0 || minutes > threshold) {
    notifiedChildren.delete(childId);
    return;
  }

  if (Notification.permission !== 'granted') return;
  if (notifiedChildren.get(childId)) return;

  const title = notificationTitle(displayName, minutes);
  const options: NotificationOptions = {
    body: notificationBody(minutes),
    tag: `remaining-${childId}`,
    requireInteraction: false,
    icon: '/icons/icon-192x192.png',
    badge: '/icons/icon-192x192.png',
  };

  try {
    if (hasNavigator() && navigator.serviceWorker) {
      const registration = await navigator.serviceWorker.getRegistration();
      if (registration) {
        await registration.showNotification(title, options);
        notifiedChildren.set(childId, true);
        return;
      }
    }
    } catch (err) {
      console.warn('Failed to show notification via service worker', err);
    }

  try {
    // Fallback do bezpośredniego Notification API
    new Notification(title, options);
    notifiedChildren.set(childId, true);
  } catch (err) {
    console.warn('Failed to show notification', err);
  }
}
export { getVapidPublicKey, isWebPushConfigured } from './env';
