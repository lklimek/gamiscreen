import { useEffect, useState } from 'react'
import { getAuthClaims, getToken, notificationsCount, setToken } from './api'
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
  // PWA install prompt handling
  const [installEvt, setInstallEvt] = useState<null | (Event & { prompt: () => Promise<void> })>(null)
  const [installed, setInstalled] = useState<boolean>(() => {
    const isStandalone = window.matchMedia && window.matchMedia('(display-mode: standalone)').matches
    const isIOSStandalone = (navigator as any).standalone === true
    return isStandalone || isIOSStandalone
  })

  const logout = () => {
    setToken(null)
    setTokenState(null)
    nav('login')
  }

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
  // WebSocket push for notifications and child remaining updates
  useEffect(() => {
    const cl = getAuthClaims()
    if (!token) return
    const serverBase = (window as any).gamiscreenApiBase || (window.location.origin)
    const base = (() => {
      const ls = localStorage.getItem('gamiscreen.server_base') || ''
      if (ls) return ls
      return serverBase
    })()
    const wsUrl = (() => {
      try {
        const u = new URL(base)
        u.protocol = u.protocol === 'https:' ? 'wss:' : 'ws:'
        u.pathname = (u.pathname.replace(/\/+$/, '')) + '/api/ws'
        u.search = '?token=' + encodeURIComponent(token)
        return u.toString()
      } catch {
        const loc = window.location
        const proto = loc.protocol === 'https:' ? 'wss' : 'ws'
        return `${proto}://${loc.host}/api/ws?token=${encodeURIComponent(token)}`
      }
    })()
    let ws: WebSocket | null = null
    let retryT: any
    const connect = () => {
      ws = new WebSocket(wsUrl)
      ws.onmessage = (ev) => {
        try {
          const msg = JSON.parse(ev.data)
          if (msg && msg.type === 'pending_count' && typeof msg.count === 'number') {
            setNotifCount(msg.count)
          } else if (msg && msg.type === 'remaining_updated' && msg.child_id && typeof msg.remaining_minutes === 'number') {
            // Broadcast to any listeners (e.g., child page) to update remaining
            window.dispatchEvent(new CustomEvent('gamiscreen:remaining-updated', { detail: { child_id: msg.child_id, remaining_minutes: msg.remaining_minutes } }))
          }
        } catch { }
      }
      ws.onclose = () => {
        retryT = setTimeout(connect, 5000)
      }
      ws.onerror = () => {
        try { ws && ws.close() } catch { }
      }
    }
    connect()
    return () => { if (ws) { try { ws.close() } catch { } } if (retryT) clearTimeout(retryT) }
  }, [token])
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
            <div className="row" style={{ alignItems: 'center', gap: 8 }}>
              {claims?.role === 'parent' && (
                <button className="secondary outline" onClick={() => nav('notifications')} aria-label={`Notifications (${notifCount})`} title={`Notifications (${notifCount})`} style={{ position: 'relative' }}>
                  🔔
                  {notifCount > 0 && (
                    <span style={{ position: 'absolute', top: -6, right: -6, background: '#d00', color: '#fff', borderRadius: 12, fontSize: 10, padding: '1px 6px' }}>{notifCount}</span>
                  )}
                </button>
              )}
              <button className="secondary outline" onClick={logout}>Logout</button>
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
      {!installed && installEvt && (
        <footer style={{ textAlign: 'center', fontSize: 12, marginTop: 12 }}>
          <a href="#install" onClick={async (e) => { e.preventDefault(); const ev = installEvt; try { await ev.prompt(); } catch { } }}>Install app</a>
        </footer>
      )}
    </main>
  )
}
