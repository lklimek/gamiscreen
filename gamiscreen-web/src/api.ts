export interface AuthResp { token: string }
export interface ChildDto { id: string; display_name: string }
export interface TaskDto { id: string; name: string; minutes: number }
export interface TaskWithStatusDto { id: string; name: string; minutes: number; last_done?: string | null }
export interface RemainingDto { child_id: string; remaining_minutes: number }

const TOKEN_KEY = 'gamiscreen.token'
const SERVER_BASE_KEY = 'gamiscreen.server_base'

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY)
}

export function setToken(token: string | null) {
  if (token) localStorage.setItem(TOKEN_KEY, token)
  else localStorage.removeItem(TOKEN_KEY)
}

export function getServerBase(): string | null {
  try {
    return localStorage.getItem(SERVER_BASE_KEY)
  } catch {
    return null
  }
}

export function setServerBase(url: string | null) {
  try {
    if (url && url.trim()) localStorage.setItem(SERVER_BASE_KEY, url.replace(/\/+$/, ''))
    else localStorage.removeItem(SERVER_BASE_KEY)
  } catch {
    // ignore storage errors (private mode, etc.)
  }
}

export type Role = 'parent' | 'child'
export interface JwtClaims {
  sub: string
  jti: string
  exp: number
  role: Role
  child_id?: string
  device_id?: string
}

export function getAuthClaims(): JwtClaims | null {
  const t = getToken()
  if (!t) return null
  const parts = t.split('.')
  if (parts.length < 2) return null
  try {
    const json = atob(parts[1].replace(/-/g, '+').replace(/_/g, '/'))
    const obj = JSON.parse(json)
    return obj as JwtClaims
  } catch {
    return null
  }
}

function apiBase(): string {
  // Prefer user-configured base (for GH Pages), then env, then same-origin
  const ls = getServerBase()
  if (ls) return ls.replace(/\/+$/, '')
  const env = (import.meta as any).env || {}
  const v = env.VITE_API_BASE_URL || ''
  if (v) return v
  return ''
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const base = apiBase()
  const url = base + path
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...(init?.headers as Record<string, string> || {}),
  }
  const token = getToken()
  if (token) headers['Authorization'] = `Bearer ${token}`
  const resp = await fetch(url, { ...init, headers })
  if (!resp.ok) {
    let msg = `${resp.status} ${resp.statusText}`
    try {
      const body = await resp.json() as any
      if (body?.error) msg = body.error
    } catch {}
    throw new Error(msg)
  }
  if (resp.status === 204) return undefined as unknown as T
  return await resp.json() as T
}

export async function login(username: string, password: string) {
  const body = { username, password }
  const data = await request<AuthResp>('/api/auth/login', {
    method: 'POST',
    body: JSON.stringify(body),
  })
  setToken(data.token)
  return data
}

export async function listChildren() {
  return request<ChildDto[]>('/api/children')
}

export async function listTasks() {
  return request<TaskDto[]>('/api/tasks')
}

export async function getRemaining(childId: string) {
  return request<RemainingDto>(`/api/children/${encodeURIComponent(childId)}/remaining`)
}

export async function listChildTasks(childId: string) {
  return request<TaskWithStatusDto[]>(`/api/children/${encodeURIComponent(childId)}/tasks`)
}

export interface RewardHistoryItemDto { time: string; description?: string | null; minutes: number }
export async function listChildRewards(childId: string, page = 1, per_page = 10) {
  const p = new URLSearchParams({ page: String(page), per_page: String(per_page) })
  return request<RewardHistoryItemDto[]>(`/api/children/${encodeURIComponent(childId)}/reward?${p.toString()}`)
}

export async function rewardMinutes(opts: { child_id: string; task_id?: string; minutes?: number; description?: string | null }) {
  const path = `/api/children/${encodeURIComponent(opts.child_id)}/reward`
  return request<{ remaining_minutes: number }>(path, {
    method: 'POST',
    body: JSON.stringify(opts)
  })
}
