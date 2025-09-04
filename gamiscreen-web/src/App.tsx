import { useEffect, useState } from 'react'
import { getAuthClaims, getToken, setToken } from './api'
import { ChildDetailsPage } from './pages/ChildDetailsPage'
import { LoginPage } from './pages/LoginPage'
import { StatusPage } from './pages/StatusPage'

type Route = 'status' | 'login' | 'child'

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
            <button className="secondary outline" onClick={logout}>Logout</button>
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
        </section>
      </article>
      {!installed && installEvt && (
        <footer style={{ textAlign: 'center', fontSize: 12, marginTop: 12 }}>
          <a href="#install" onClick={async (e) => { e.preventDefault(); const ev = installEvt; try { await ev.prompt(); } catch {} }}>Install app</a>
        </footer>
      )}
    </main>
  )
}
