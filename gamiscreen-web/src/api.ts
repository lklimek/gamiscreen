import type {
  AuthReq,
  AuthResp,
  ChildDto,
  ClientRegisterReq,
  ClientRegisterResp,
  HeartbeatReq,
  HeartbeatResp,
  JwtClaims,
  NotificationItemDto,
  NotificationsCountDto,
  RemainingDto,
  RewardHistoryItemDto,
  RewardReq,
  RewardResp,
  Role,
  SubmitTaskReq,
  TaskDto,
  TaskWithStatusDto,
  VersionInfoDto,
} from './generated/api-types'

export type {
  AuthReq,
  AuthResp,
  ChildDto,
  ClientRegisterReq,
  ClientRegisterResp,
  HeartbeatReq,
  HeartbeatResp,
  JwtClaims,
  NotificationItemDto,
  NotificationsCountDto,
  RemainingDto,
  RewardHistoryItemDto,
  RewardReq,
  RewardResp,
  Role,
  SubmitTaskReq,
  TaskDto,
  TaskWithStatusDto,
  VersionInfoDto,
} from './generated/api-types'

const TOKEN_KEY = 'gamiscreen.token'
const SERVER_BASE_KEY = 'gamiscreen.server_base'
const API_V1_PREFIX = '/api/v1'

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
    if (resp.status === 401) {
      setToken(null)
      try {
        window.dispatchEvent(new CustomEvent('gamiscreen:token-invalid'))
      } catch { }
    }
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

function resolveTenantId(): string {
  const claims = getAuthClaims()
  if (!claims?.tenant_id) {
    throw new Error('Tenant ID unavailable; please log in again')
  }
  return claims.tenant_id
}

function tenantPath(path: string): string {
  const tenantId = resolveTenantId()
  const scope = `${API_V1_PREFIX}/family/${encodeURIComponent(tenantId)}`
  return `${scope}/${path.replace(/^\/+/, '')}`
}

export async function login(username: string, password: string) {
  const body: AuthReq = { username, password }
  const data = await request<AuthResp>(`${API_V1_PREFIX}/auth/login`, {
    method: 'POST',
    body: JSON.stringify(body),
  })
  setToken(data.token)
  return data
}

export async function renewToken() {
  const data = await request<AuthResp>(`${API_V1_PREFIX}/auth/renew`, {
    method: 'POST',
  })
  setToken(data.token)
  return data
}

export async function listChildren() {
  return request<ChildDto[]>(tenantPath('children'))
}

export async function listTasks() {
  return request<TaskDto[]>(tenantPath('tasks'))
}

export async function getRemaining(childId: string) {
  return request<RemainingDto>(tenantPath(`children/${encodeURIComponent(childId)}/remaining`))
}

export async function listChildTasks(childId: string) {
  return request<TaskWithStatusDto[]>(tenantPath(`children/${encodeURIComponent(childId)}/tasks`))
}

export async function listChildRewards(childId: string, page = 1, per_page = 10) {
  const p = new URLSearchParams({ page: String(page), per_page: String(per_page) })
  return request<RewardHistoryItemDto[]>(`${tenantPath(`children/${encodeURIComponent(childId)}/reward`)}?${p.toString()}`)
}

export async function rewardMinutes(body: RewardReq) {
  const path = tenantPath(`children/${encodeURIComponent(body.child_id)}/reward`)
  return request<RewardResp>(path, {
    method: 'POST',
    body: JSON.stringify(body)
  })
}

// Child task submission
export async function submitTask(childId: string, taskId: string) {
  const path = tenantPath(`children/${encodeURIComponent(childId)}/tasks/${encodeURIComponent(taskId)}/submit`)
  return request<void>(path, { method: 'POST' })
}

// Notifications
export async function notificationsCount() {
  return request<NotificationsCountDto>(tenantPath('notifications/count'))
}

export async function listNotifications() {
  return request<NotificationItemDto[]>(tenantPath('notifications'))
}

export async function approveSubmission(id: number) {
  return request<void>(tenantPath(`notifications/task-submissions/${id}/approve`), { method: 'POST' })
}

export async function discardSubmission(id: number) {
  return request<void>(tenantPath(`notifications/task-submissions/${id}/discard`), { method: 'POST' })
}

export async function getServerVersion(): Promise<string> {
  const { version } = await request<VersionInfoDto>(`${API_V1_PREFIX}/version`)
  return version
}
