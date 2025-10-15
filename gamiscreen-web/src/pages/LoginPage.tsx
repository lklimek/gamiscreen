import React, { useMemo, useState } from 'react'
import { getServerBase, login, setServerBase } from '../api'

export function LoginPage(props: { onLogin: (token: string) => void }) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [serverUrl, setServerUrl] = useState<string>(() => getServerBase() || '')

  const showServerInput = useMemo(() => {
    const env = (import.meta as any).env || {}
    if (env.VITE_ALLOW_CUSTOM_SERVER_URL === '1') return true
    try {
      return typeof window !== 'undefined' && window.location.hostname.endsWith('github.io')
    } catch {
      return false
    }
  }, [])

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    setLoading(true)
    try {
      // Persist server URL if shown (GH Pages use-case)
      if (showServerInput) {
        const v = serverUrl.trim().replace(/\/+$/, '')
        setServerBase(v || null)
      }
      const resp = await login(username, password)
      props.onLogin(resp.token)
    } catch (e: any) {
      setError(e.message || 'Login failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <form
      className="col"
      onSubmit={onSubmit}
      autoComplete="on"
      method="post"
      action="/login"
      id="login-form"
      name="login-form"
    >
      {showServerInput && (
        <label className="col" htmlFor="server-url">
          <span>Server URL (API)</span>
          <input
            inputMode="url"
            name="server-url"
            id="server-url"
            data-lpignore="true"
            placeholder="https://your-server.example.com"
            value={serverUrl}
            onChange={e => setServerUrl(e.target.value)}
            />
          <small>Only needed when using GitHub Pages.</small>
        </label>
      )}
      <label className="col" htmlFor="username">
        <span>Username</span>
        <input
          type="text"
          id="username"
          name="username"
          autoComplete="username"
          autoCapitalize="none"
          autoCorrect="off"
          value={username}
          onChange={e=>setUsername(e.target.value)}
          placeholder="parent"
        />
      </label>
      <label className="col" htmlFor="password">
        <span>Password</span>
        <input
          type="password"
          id="password"
          name="password"
          autoComplete="current-password"
          value={password}
          onChange={e=>setPassword(e.target.value)}
          placeholder="••••••"
        />
      </label>
      {error && <div className="error">{error}</div>}
      <div className="row">
        <button type="submit" disabled={loading}>{loading? 'Logging in…':'Login'}</button>
      </div>
    </form>
  )
}
