import React, { useState } from 'react'
import { login } from '../api'

export function LoginPage(props: { onLogin: (token: string) => void }) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault()
    setError(null)
    setLoading(true)
    try {
      const resp = await login(username, password)
      props.onLogin(resp.token)
    } catch (e: any) {
      setError(e.message || 'Login failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <form className="col" onSubmit={onSubmit}>
      <label className="col">
        <span>Username</span>
        <input value={username} onChange={e=>setUsername(e.target.value)} placeholder="parent" />
      </label>
      <label className="col">
        <span>Password</span>
        <input type="password" value={password} onChange={e=>setPassword(e.target.value)} placeholder="••••••" />
      </label>
      {error && <div className="error">{error}</div>}
      <div className="row">
        <button type="submit" disabled={loading}>{loading? 'Logging in…':'Login'}</button>
      </div>
    </form>
  )
}
