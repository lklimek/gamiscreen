export type PermissionState = NotificationPermission | 'unsupported';

const notifiedChildren = new Map<string, boolean>();
let requestingPermission = false;

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

  if (minutes <= 0 || minutes > 5) {
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
