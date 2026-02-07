import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import packageInfo from '../package.json'
import { getAuthClaims, getServerVersion, getToken, notificationsCount, pushUnsubscribe, renewToken, setToken } from './api'
const API_V1_PREFIX = '/api/v1'
const EMBEDDED_MODE_POLL_TIMEOUT_MS = 60000 // 1 minute
import { ChildDetailsPage } from './pages/ChildDetailsPage'
import { LoginPage } from './pages/LoginPage'
import { NotificationsPage } from './pages/NotificationsPage'
import { SettingsPage } from './pages/SettingsPage'
import { StatusPage } from './pages/StatusPage'
import { NotificationSettings, getNotificationSettings, saveNotificationSettings } from './notifications'
import { isEmbeddedMode } from './nativeBridge'

type Route = 'status' | 'login' | 'child' | 'notifications' | 'settings'

function useHashRoute(): [Route, (r: Route, opts?: { childId?: string }) => void, { childId?: string }] {
  const parse = () => {
    const raw = window.location.hash.replace(/^#\/?/, '')
    const [path, qs] = raw.split('?')
    const parts = path.split('/')
    const base = (parts[0] || 'status') as Route
    const childId = parts[0] === 'child' && parts[1] ? decodeURIComponent(parts[1]) : (new URLSearchParams(qs).get('child') || undefined)
    return { route: base, params: { childId } }
  }
  const init = parse()
  const [route, setRoute] = useState<Route>(init.route)
  const [params, setParams] = useState<{ childId?: string }>(init.params)
  useEffect(() => {
    const fn = () => { const p = parse(); setRoute(p.route); setParams(p.params) }
    window.addEventListener('hashchange', fn)
    return () => window.removeEventListener('hashchange', fn)
  }, [])
  const nav = (r: Route, opts?: { childId?: string }) => {
    if (r === 'child' && opts?.childId) {
      window.location.hash = `child/${encodeURIComponent(opts.childId)}`
    } else {
      window.location.hash = r
    }
    const p = parse();
    setRoute(p.route)
    setParams(p.params)
  }
  return [route, nav, params]
}

export function App() {
  const webVersion = packageInfo.version
  const [route, nav, params] = useHashRoute()
  const [token, setTokenState] = useState<string | null>(() => getToken())
  const claims = getAuthClaims()
  const isChild = claims?.role === 'child'
  const [menuOpen, setMenuOpen] = useState(false)
  const [refreshingToken, setRefreshingToken] = useState(() => !!getToken())
  const hasToken = token != null
  const authReady = hasToken && !refreshingToken
  // PWA install prompt handling
  const [installEvt, setInstallEvt] = useState<null | (Event & { prompt: () => Promise<void> })>(null)
  const [installed, setInstalled] = useState<boolean>(() => {
    const isStandalone = window.matchMedia && window.matchMedia('(display-mode: standalone)').matches
    const isIOSStandalone = (navigator as any).standalone === true
    return isStandalone || isIOSStandalone
  })
  const [serverVersion, setServerVersion] = useState<string | null>(null)
  const [notificationSettings, setNotificationSettings] = useState<NotificationSettings>(() => getNotificationSettings())
  const [embedded, setEmbedded] = useState(() => isEmbeddedMode())
  useEffect(() => {
    if (embedded) return
    let cancelled = false
    let timer: number | undefined
    const startTime = Date.now()
    const check = () => {
      if (cancelled) return
      if (isEmbeddedMode()) {
        setEmbedded(true)
        return
      }
      // Stop polling after timeout
      const elapsed = Date.now() - startTime
      if (elapsed >= EMBEDDED_MODE_POLL_TIMEOUT_MS) {
        return
      }
      timer = window.setTimeout(check, 1000)
    }
    timer = window.setTimeout(check, 0)
    return () => {
      cancelled = true
      if (timer) window.clearTimeout(timer)
    }
  }, [embedded])

  const cleanupPushSubscription = useCallback(async () => {
    try {
      if (!('serviceWorker' in navigator)) return
      const registration = await navigator.serviceWorker.ready
      const subscription = await registration.pushManager.getSubscription()
      if (!subscription) return
      const cl = getAuthClaims()
      if (cl?.child_id) {
        try {
          await pushUnsubscribe(cl.child_id, subscription.endpoint)
        } catch (err) {
          console.warn('Failed to unregister push subscription on server', err)
        }
      }
      await subscription.unsubscribe().catch(() => { })
    } catch (err) {
      console.warn('Failed to cleanup push subscription', err)
    }
  }, [])

  const logout = useCallback(() => {
    cleanupPushSubscription()
    setToken(null)
    setTokenState(null)
    setRefreshingToken(false)
    nav('login')
  }, [cleanupPushSubscription, nav])

  const handleLogin = useCallback(
    (t: string) => {
      setTokenState(t)
      const cl = getAuthClaims()
      if (cl?.role === 'child' && cl.child_id) {
        nav('child', { childId: cl.child_id })
      } else {
        nav('status')
      }
    },
    [nav],
  )

  const logoutRef = useRef(logout)
  useEffect(() => {
    logoutRef.current = logout
  }, [logout])

  useEffect(() => {
    if (!authReady) {
      setMenuOpen(false)
    }
  }, [authReady])

  useEffect(() => {
    const handler = () => logout()
    window.addEventListener('gamiscreen:token-invalid', handler)
    return () => window.removeEventListener('gamiscreen:token-invalid', handler)
  }, [logout])

  useEffect(() => {
    let cancelled = false
    const current = getToken()
    if (!current) {
      return
    }
    setRefreshingToken(true)
    renewToken()
      .then(({ token: newToken }) => {
        if (cancelled) return
        const persisted = getToken()
        if (persisted !== newToken) {
          try {
            setToken(newToken)
          } catch (err) {
            console.error('Failed to persist renewed token', err)
          }
        }
        setTokenState(getToken() ?? newToken)
      })
      .catch((err: any) => {
        if (cancelled) return
        console.warn('Token renewal failed', err)
        const msg = String(err?.message || err || '')
        if (/401/.test(msg)) logoutRef.current()
      })
      .finally(() => {
        if (!cancelled) setRefreshingToken(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  // Notifications polling (parent)
  const [notifCount, setNotifCount] = useState<number>(0)
  useEffect(() => {
    if (!authReady) {
      setNotifCount(0)
    }
  }, [authReady])
  useEffect(() => {
    if (!authReady) {
      return
    }
    let cancelled = false
    let timer: ReturnType<typeof setTimeout> | undefined
    const tick = async () => {
      try {
        if (getAuthClaims()?.role === 'parent') {
          const { count } = await notificationsCount()
          if (!cancelled) setNotifCount(count)
        } else if (!cancelled) {
          setNotifCount(0)
        }
      } catch { }
      if (!cancelled) {
        timer = setTimeout(tick, 30000)
      }
    }
    tick()
    return () => {
      cancelled = true
      if (timer) clearTimeout(timer)
    }
  }, [authReady])
  // Server-Sent Events push for notifications and child remaining updates
  useEffect(() => {
    if (!authReady) return
    const tenantId = claims?.tenant_id
    if (!tenantId) return
    const serverBase = (window as any).gamiscreenApiBase || (window.location.origin)
    const base = (() => {
      const ls = localStorage.getItem('gamiscreen.server_base') || ''
      if (ls) return ls
      return serverBase
    })()
    const sseUrl = (() => {
      const scope = `${API_V1_PREFIX}/family/${encodeURIComponent(tenantId)}`
      try {
        const u = new URL(base)
        // keep http/https, use tenant-scoped SSE
        u.pathname = (u.pathname.replace(/\/+$/, '')) + `${scope}/sse`
        u.search = '?token=' + encodeURIComponent(token)
        return u.toString()
      } catch {
        const loc = window.location
        return `${loc.protocol}//${loc.host}${scope}/sse?token=${encodeURIComponent(token)}`
      }
    })()
    let es: EventSource | null = null
    const connect = () => {
      try {
        es = new EventSource(sseUrl, { withCredentials: false })
      } catch {
        es = null
        return
      }
      es.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data)
          if (msg && msg.type === 'pending_count' && typeof msg.count === 'number') {
            setNotifCount(msg.count)
          } else if (msg && msg.type === 'remaining_updated' && msg.child_id && typeof msg.remaining_minutes === 'number') {
            window.dispatchEvent(new CustomEvent('gamiscreen:remaining-updated', { detail: { child_id: msg.child_id, remaining_minutes: msg.remaining_minutes } }))
          }
        } catch { }
      }
      es.onerror = () => {
        // Browser will auto-reconnect SSE; nothing to do.
      }
    }
    connect()
    return () => { if (es) { try { es.close() } catch { } } }
  }, [authReady, claims?.tenant_id, token])
  // Immediate refresh when notifications change (approve/discard)
  useEffect(() => {
    if (!authReady) return
    const refresh = async () => {
      try {
        if (getAuthClaims()?.role === 'parent') {
          const { count } = await notificationsCount()
          setNotifCount(count)
        }
      } catch { }
    }
    const handler = () => { refresh() }
    window.addEventListener('gamiscreen:notif-refresh', handler as EventListener)
    return () => window.removeEventListener('gamiscreen:notif-refresh', handler as EventListener)
  }, [authReady])

  useEffect(() => {
    if (!hasToken && route !== 'login') nav('login')
    if (authReady) {
      const cl = getAuthClaims()
    if (cl?.role === 'child' && cl.child_id && route !== 'child' && route !== 'settings') {
        nav('child', { childId: cl.child_id })
      }
    }
  }, [hasToken, authReady, route, nav])

  useEffect(() => {
    if (refreshingToken) return
    let cancelled = false
    getServerVersion()
      .then((version) => {
        if (!cancelled) setServerVersion(version)
      })
      .catch(() => {
        if (!cancelled) setServerVersion(null)
      })
    return () => {
      cancelled = true
    }
  }, [refreshingToken, token])

  useEffect(() => {
    const onBip = (e: Event & { preventDefault: () => void; prompt: () => Promise<void> }) => {
      e.preventDefault()
      setInstallEvt(e)
    }
    const onInstalled = () => { setInstalled(true); setInstallEvt(null) }
    window.addEventListener('beforeinstallprompt', onBip as any)
    window.addEventListener('appinstalled', onInstalled)
    const mm = window.matchMedia && window.matchMedia('(display-mode: standalone)')
    const onDisplayChange = () => setInstalled(mm.matches)
    mm && mm.addEventListener && mm.addEventListener('change', onDisplayChange)
    return () => {
      window.removeEventListener('beforeinstallprompt', onBip as any)
      window.removeEventListener('appinstalled', onInstalled)
      mm && mm.removeEventListener && mm.removeEventListener('change', onDisplayChange)
    }
  }, [])

  useEffect(() => {
    const handler = () => setNotificationSettings(getNotificationSettings())
    window.addEventListener('gamiscreen:notification-settings-changed', handler as EventListener)
    return () => window.removeEventListener('gamiscreen:notification-settings-changed', handler as EventListener)
  }, [])

  const handleNotificationSettingsChange = useCallback((next: NotificationSettings) => {
    saveNotificationSettings(next)
    setNotificationSettings(getNotificationSettings())
  }, [])

  const installAvailable = !installed && !!installEvt

  const triggerInstall = useCallback(async () => {
    if (!installEvt) return
    try {
      await installEvt.prompt()
    } catch (err) {
      console.warn('Install prompt failed', err)
    } finally {
      setInstallEvt(null)
    }
  }, [installEvt])

  const content = useMemo(() => {
    if (!hasToken) {
      return <LoginPage onLogin={handleLogin} />
    }
    if (!authReady) {
      return (
        <div className="col" style={{ alignItems: 'center', gap: 12 }}>
          <span className="subtitle">Renewing session…</span>
        </div>
      )
    }
    if (route === 'settings') {
      return (
        <SettingsPage
          installed={installed}
          installAvailable={installAvailable}
          onInstall={triggerInstall}
          notificationSettings={notificationSettings}
          onSettingsChange={handleNotificationSettingsChange}
          role={claims?.role}
          childId={claims?.child_id || undefined}
        />
      )
    }
    if (isChild && claims?.child_id) {
      return <ChildDetailsPage childId={claims.child_id} />
    }
    if (route === 'child' && params.childId) {
      return <ChildDetailsPage childId={params.childId} />
    }
    if (route === 'notifications') {
      return <NotificationsPage />
    }
    return <StatusPage />
  }, [authReady, claims?.child_id, claims?.role, handleLogin, handleNotificationSettingsChange, hasToken, installAvailable, installed, isChild, notificationSettings, params.childId, route, triggerInstall])

  return (
    <main className="container">
      <article>
        <header className="row" style={{ justifyContent: 'space-between' }}>
          <div>
            <a href="#status" style={{ textDecoration: 'none' }}>
              <h1 className="title" style={{ margin: 0 }}>Gamiscreen</h1>
            </a>
            <p className="subtitle" style={{ margin: 0 }}>Reward earned screen time</p>
          </div>
          {authReady && (
            <div className="row" style={{ alignItems: 'center', gap: 8, position: 'relative' }}>
              {claims?.role === 'parent' && (
                <button
                  className="secondary outline iconButton"
                  onClick={() => nav('notifications')}
                  aria-label={`Notifications (${notifCount})`}
                  title={`Notifications (${notifCount})`}
                  style={{ position: 'relative' }}
                >
                  🔔
                  {notifCount > 0 && (
                    <span style={{ position: 'absolute', top: -6, right: -6, background: '#d00', color: '#fff', borderRadius: 12, fontSize: 10, padding: '1px 6px' }}>{notifCount}</span>
                  )}
                </button>
              )}
              <button className="secondary outline iconButton" aria-label="Menu" title="Menu" onClick={() => setMenuOpen(v => !v)}>
                ☰
              </button>
              {menuOpen && (
                <div style={{ position: 'absolute', top: '100%', right: 0, marginTop: 8, minWidth: 160, background: 'var(--card-background-color, #fff)', border: '1px solid var(--muted-border-color, #ddd)', borderRadius: 6, boxShadow: '0 6px 24px rgba(0,0,0,0.15)', zIndex: 10 }}>
                  <a href="#status" onClick={() => setMenuOpen(false)} style={{ display: 'block', padding: '8px 12px', textDecoration: 'none' }}>
                    <span aria-hidden="true" style={{ marginRight: 8 }}>📊</span>
                    Status
                  </a>
                  <a href="#settings" onClick={() => setMenuOpen(false)} style={{ display: 'block', padding: '8px 12px', textDecoration: 'none' }}>
                    <span aria-hidden="true" style={{ marginRight: 8 }}>⚙️</span>
                    Settings
                  </a>
                  <a href="#logout" onClick={(e) => { e.preventDefault(); setMenuOpen(false); logout(); }} style={{ display: 'block', padding: '8px 12px', textDecoration: 'none' }}>
                    <span aria-hidden="true" style={{ marginRight: 8 }}>🚪</span>
                    Logout
                  </a>
                </div>
              )}
            </div>
          )}
        </header>
        {/* Navigation removed per new workflow */}
        <section>{content}</section>
      </article>
      <footer style={{ textAlign: 'center', marginTop: 12 }}>
        <p style={{ margin: 0, fontSize: 12 }}>
          <small>Use responsibly. Reward healthy habits.</small>
        </p>
        <p style={{ margin: 0, fontSize: 10, color: 'var(--muted-color, #666)' }}>
          Server&nbsp;v{serverVersion ?? '…'} · Web&nbsp;v{webVersion}{embedded ? ' · Embedded' : ''} · Tenant&nbsp;{claims?.tenant_id ?? '—'}
        </p>
      </footer>
      {installAvailable && (
        <footer style={{ textAlign: 'center', fontSize: 12, marginTop: 12 }}>
          <a href="#install" onClick={async (e) => { e.preventDefault(); await triggerInstall(); }}>Install app</a>
        </footer>
      )}
    </main>
  )
}
