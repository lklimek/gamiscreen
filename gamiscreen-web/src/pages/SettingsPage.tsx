import { useCallback, useEffect, useMemo, useState } from 'react'
import { Role, getConfig, pushSubscribe, pushUnsubscribe } from '../api'
import {
  NotificationSettings,
  base64UrlToUint8Array,
  getVapidPublicKey,
  requestNotificationPermission,
  supportsNotifications,
} from '../notifications'

type Props = {
  installed: boolean
  installAvailable: boolean
  onInstall: () => Promise<void>
  notificationSettings: NotificationSettings
  onSettingsChange: (next: NotificationSettings) => void
  role?: Role
  childId?: string
}

export function SettingsPage(props: Props) {
  const {
    installed,
    installAvailable,
    onInstall,
    notificationSettings,
    onSettingsChange,
    role,
    childId,
  } = props

  const [statusMessage, setStatusMessage] = useState<string | null>(null)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)
  const [localSettings, setLocalSettings] = useState<NotificationSettings>(notificationSettings)
  const [busy, setBusy] = useState(false)
  const [hasSubscription, setHasSubscription] = useState(false)

  useEffect(() => {
    setLocalSettings(notificationSettings)
  }, [notificationSettings])

  const notificationSupported = supportsNotifications()
  const permission = useMemo(() => {
    if (!notificationSupported) return 'unsupported'
    return Notification.permission
  }, [notificationSupported])

  const canConfigurePush = notificationSupported && role === 'child' && !!childId
  const notificationsDisabledReason = !notificationSupported
    ? 'This browser does not support notifications.'
    : role === 'child'
    ? (!childId ? 'Child profile not available.' : null)
    : 'Sign in as a child profile to enable push notifications.'

  useEffect(() => {
    if (!notificationSupported) return
    let cancelled = false
    navigator.serviceWorker?.ready
      .then((reg) => reg.pushManager.getSubscription())
      .then((sub) => {
        if (cancelled) return
        setHasSubscription(!!sub)
        if (!sub && localSettings.enabled) {
          setLocalSettings((prev) => ({ ...prev, enabled: false }))
        }
      })
      .catch(() => { })
    return () => {
      cancelled = true
    }
  }, [notificationSupported])

  const updateSettings = useCallback(
    (next: NotificationSettings) => {
      setLocalSettings(next)
      onSettingsChange(next)
    },
    [onSettingsChange],
  )

  const handleToggleNotifications = useCallback(async () => {
    setStatusMessage(null)
    setErrorMessage(null)
    if (busy) return
    if (!canConfigurePush) {
      setErrorMessage(notificationsDisabledReason || 'Notifications cannot be configured for this account.')
      return
    }
    const nextEnabled = !localSettings.enabled
    setBusy(true)
    try {
      if (nextEnabled) {
        const perm = await requestNotificationPermission()
        if (perm !== 'granted') {
          setErrorMessage(
            perm === 'denied'
              ? 'Browser blocked notifications. Please allow them in browser settings.'
              : 'Unable to enable notifications on this device.',
          )
          updateSettings({ ...localSettings, enabled: false })
          return
        }
        const config = await getConfig()
        const vapid = config.push_public_key || getVapidPublicKey()
        if (!vapid) {
          setErrorMessage('Push notifications are not configured on this server (missing VAPID public key).')
          updateSettings({ ...localSettings, enabled: false })
          return
        }
        const registration = await navigator.serviceWorker.ready
        let subscription = await registration.pushManager.getSubscription()
        if (!subscription) {
          const applicationServerKey = base64UrlToUint8Array(vapid).buffer as ArrayBuffer
          subscription = await registration.pushManager.subscribe({
            userVisibleOnly: true,
            applicationServerKey,
          })
        }
        if (!childId) {
          setErrorMessage('Unable to determine child profile for push registration.')
          await subscription.unsubscribe().catch(() => { })
          updateSettings({ ...localSettings, enabled: false })
          return
        }
        await pushSubscribe(childId, subscription)
        setHasSubscription(true)
        updateSettings({ ...localSettings, enabled: true })
        setStatusMessage('Notifications enabled.')
      } else {
        const registration = await navigator.serviceWorker.ready
        const subscription = await registration.pushManager.getSubscription()
        if (subscription && childId) {
          try {
            await pushUnsubscribe(childId, subscription.endpoint)
          } catch (err) {
            console.warn('Failed to unregister subscription on server', err)
          }
          await subscription.unsubscribe().catch(() => { })
        }
        setHasSubscription(false)
        updateSettings({ ...localSettings, enabled: false })
        setStatusMessage('Notifications disabled.')
      }
    } catch (err: any) {
      console.warn('Notification toggle failed', err)
      setErrorMessage(err?.message || 'Failed to update notification settings.')
      updateSettings({ ...localSettings, enabled: false })
    } finally {
      setBusy(false)
    }
  }, [busy, canConfigurePush, childId, localSettings, notificationsDisabledReason, updateSettings])

  const handleThresholdChange = useCallback(
    (value: number) => {
      const minutes = Math.max(1, Math.min(60, Math.round(value)))
      updateSettings({ ...localSettings, thresholdMinutes: minutes })
      setStatusMessage(`Notifications will alert below ${minutes} minutes.`)
      setErrorMessage(null)
    },
    [localSettings, updateSettings],
  )

  const installSection = (
    <section className="card" style={{ padding: 16 }}>
      <h2 className="title" style={{ marginTop: 0 }}>Install App</h2>
      <p className="subtitle">
        Install Gamiscreen as a standalone application for faster access.
      </p>
      {installAvailable ? (
        <button onClick={onInstall} className="acceptButton" style={{ alignSelf: 'flex-start' }}>
          📱 Install
        </button>
      ) : (
        <p className="subtitle">
          {installed
            ? 'App is already installed.'
            : 'Install option will appear when supported by this browser.'}
        </p>
      )}
    </section>
  )

  const notificationsSection = (
    <section className="card" style={{ padding: 16 }}>
      <h2 className="title" style={{ marginTop: 0 }}>Notifications</h2>
      {!notificationSupported ? (
        <p className="subtitle">This browser does not support notifications.</p>
      ) : (
        <>
          <div className="row" style={{ alignItems: 'center', justifyContent: 'space-between' }}>
            <div className="col" style={{ gap: 4 }}>
              <strong>Enable push notifications</strong>
              <span className="subtitle">
                Receive alerts when remaining time changes unexpectedly.
              </span>
            </div>
            <button
              className={localSettings.enabled ? 'contrast' : 'secondary'}
              onClick={handleToggleNotifications}
              aria-pressed={localSettings.enabled}
              disabled={busy || !canConfigurePush}
            >
              {localSettings.enabled ? 'Disable' : 'Enable'}
            </button>
          </div>
          {notificationsDisabledReason && !canConfigurePush && (
            <p className="subtitle">{notificationsDisabledReason}</p>
          )}
          {permission === 'denied' && canConfigurePush && (
            <p className="error">
              Notifications are blocked by the browser. Allow them in browser settings to enable.
            </p>
          )}
          {canConfigurePush && localSettings.enabled && hasSubscription && (
            <p className="subtitle">Notifications are active on this device.</p>
          )}
          <label className="col" style={{ marginTop: 16, gap: 6 }}>
            <span>Alert when remaining minutes fall below</span>
            <input
              type="number"
              min={1}
              max={60}
              value={localSettings.thresholdMinutes}
              disabled={!localSettings.enabled || !canConfigurePush || busy}
              onChange={(e) => handleThresholdChange(parseInt(e.target.value, 10))}
              style={{ width: '10ch' }}
            />
          </label>
          {statusMessage && <p className="subtitle">{statusMessage}</p>}
          {errorMessage && <p className="error">{errorMessage}</p>}
        </>
      )}
    </section>
  )

  return (
    <div className="col" style={{ gap: 16 }}>
      {installSection}
      {notificationsSection}
    </div>
  )
}
