import { useCallback, useEffect, useState } from 'react'
import { getAuthClaims, getServerVersion, getToken, notificationsCount, renewToken, setToken } from './api'

const API_V1_PREFIX = '/api/v1'
import { ChildDetailsPage } from './pages/ChildDetailsPage'
import { LoginPage } from './pages/LoginPage'
import { NotificationsPage } from './pages/NotificationsPage'
import { StatusPage } from './pages/StatusPage'

type Route = 'status' | 'login' | 'child' | 'notifications'

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
  const [route, nav, params] = useHashRoute()
  const [token, setTokenState] = useState<string | null>(() => getToken())
  const loggedIn = !!token
  const claims = getAuthClaims()
  const isChild = claims?.role === 'child'
  const [menuOpen, setMenuOpen] = useState(false)
  // PWA install prompt handling
  const [installEvt, setInstallEvt] = useState<null | (Event & { prompt: () => Promise<void> })>(null)
  const [installed, setInstalled] = useState<boolean>(() => {
    const isStandalone = window.matchMedia && window.matchMedia('(display-mode: standalone)').matches
    const isIOSStandalone = (navigator as any).standalone === true
    return isStandalone || isIOSStandalone
  })
  const [serverVersion, setServerVersion] = useState<string | null>(null)

  const logout = useCallback(() => {
    setToken(null)
    setTokenState(null)
    nav('login')
  }, [nav])

  useEffect(() => {
    const handler = () => logout()
    window.addEventListener('gamiscreen:token-invalid', handler)
    return () => window.removeEventListener('gamiscreen:token-invalid', handler)
  }, [logout])

  useEffect(() => {
    let cancelled = false
    const current = getToken()
    if (!current) return
    renewToken()
      .then(({ token: newToken }) => {
        if (cancelled) return
        setToken(newToken)
        setTokenState(newToken)
      })
      .catch((err: any) => {
        if (cancelled) return
        console.warn('Token renewal failed', err)
        const msg = String(err?.message || err || '')
        if (/401/.test(msg)) logout()
      })
    return () => { cancelled = true }
  }, [])

  // Notifications polling (parent)
  const [notifCount, setNotifCount] = useState<number>(0)
  useEffect(() => {
    let timer: any
    const tick = async () => {
      try {
        if (getAuthClaims()?.role === 'parent') {
          const { count } = await notificationsCount()
          setNotifCount(count)
        } else {
          setNotifCount(0)
        }
      } catch { }
      timer = setTimeout(tick, 30000)
    }
    tick()
    return () => { if (timer) clearTimeout(timer) }
  }, [token])
  // Server-Sent Events push for notifications and child remaining updates
  useEffect(() => {
    const tenantId = claims?.tenant_id
    if (!token) return
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
  }, [token, claims?.tenant_id])
  // Immediate refresh when notifications change (approve/discard)
  useEffect(() => {
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
  }, [token])

  useEffect(() => {
    if (!loggedIn && route !== 'login') nav('login')
    // If child is logged in but URL is not child route, redirect.
    if (loggedIn) {
      const cl = getAuthClaims()
      if (cl?.role === 'child' && cl.child_id && route !== 'child') {
        nav('child', { childId: cl.child_id })
      }
    }
  }, [loggedIn])

  useEffect(() => {
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
  }, [])

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
          {loggedIn && (
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
        <section>
          {(!loggedIn || route === 'login') && (
            <LoginPage onLogin={(t) => {
              setTokenState(t)
              const cl = getAuthClaims()
              if (cl?.role === 'child' && cl.child_id) {
                nav('child', { childId: cl.child_id })
              } else {
                nav('status')
              }
            }} />
          )}
          {loggedIn && isChild && claims?.child_id && (
            <ChildDetailsPage childId={claims.child_id} />
          )}
          {loggedIn && !isChild && route === 'status' && <StatusPage />}
          {loggedIn && !isChild && route === 'child' && params.childId && (
            <ChildDetailsPage childId={params.childId} />
          )}
          {loggedIn && !isChild && route === 'notifications' && (
            <NotificationsPage />
          )}
        </section>
      </article>
      <footer style={{ textAlign: 'center', marginTop: 12 }}>
        <p style={{ margin: 0, fontSize: 12 }}>
          <small>Use responsibly. Reward healthy habits.</small>
        </p>
        <p style={{ margin: 0, fontSize: 10, color: 'var(--muted-color, #666)' }}>
          Server&nbsp;v{serverVersion ?? '…'} · Tenant&nbsp;{claims?.tenant_id ?? '—'}
        </p>
      </footer>
      {!installed && installEvt && (
        <footer style={{ textAlign: 'center', fontSize: 12, marginTop: 12 }}>
          <a href="#install" onClick={async (e) => { e.preventDefault(); const ev = installEvt; try { await ev.prompt(); } catch { } }}>Install app</a>
        </footer>
      )}
    </main>
  )
}
