import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getAuthClaims, getConfig, getRemaining, listChildren, listChildRewards, listChildTasks, listChildUsage, pushSubscribe, RewardHistoryItemDto, rewardMinutes, submitTask, TaskWithStatusDto, UsageSeriesDto } from '../api'
import { base64UrlToUint8Array, currentNotificationPermission, getNotificationSettings, getVapidPublicKey, maybeNotifyRemaining, saveNotificationSettings, requestNotificationPermission, supportsNotifications, type NotificationSettings, type PermissionState } from '../notifications'
import { MINUTES_PER_DAY, MINUTES_PER_HOUR, MINUTES_PER_WEEK, UsageChart } from '../components/UsageChart'

const USAGE_BASE_PRESETS = [
  { key: '1h', label: '1 hour', bucketMinutes: MINUTES_PER_HOUR },
  { key: '1d', label: '1 day', bucketMinutes: MINUTES_PER_DAY },
  { key: '1w', label: '1 week', bucketMinutes: MINUTES_PER_WEEK },
] as const

type UsageBasePreset = (typeof USAGE_BASE_PRESETS)[number]
type UsagePresetKey = UsageBasePreset['key']
type UsageOption = UsageBasePreset & { windowMinutes: number }
type ViewportVariant = 'mobilePortrait' | 'mobileLandscape' | 'desktop'

const VARIANT_WINDOWS: Record<ViewportVariant, Record<UsagePresetKey, number>> = {
  mobilePortrait: {
    '1h': 6 * MINUTES_PER_HOUR,
    '1d': 7 * MINUTES_PER_DAY,
    '1w': 8 * MINUTES_PER_WEEK,
  },
  mobileLandscape: {
    '1h': 12 * MINUTES_PER_HOUR,
    '1d': 14 * MINUTES_PER_DAY,
    '1w': 16 * MINUTES_PER_WEEK,
  },
  desktop: {
    '1h': 24 * MINUTES_PER_HOUR,
    '1d': 14 * MINUTES_PER_DAY,
    '1w': 16 * MINUTES_PER_WEEK,
  },
} as const

function detectViewportVariant(): ViewportVariant {
  if (typeof window === 'undefined') return 'desktop'
  const width = window.innerWidth
  if (width >= 1024) return 'desktop'
  const isLandscape = typeof window.matchMedia === 'function'
    ? window.matchMedia('(orientation: landscape)').matches
    : width > window.innerHeight
  return isLandscape ? 'mobileLandscape' : 'mobilePortrait'
}

export function ChildDetailsPage(props: { childId: string }) {
  const { childId } = props
  const [displayName, setDisplayName] = useState<string>(childId)
  const [remaining, setRemaining] = useState<number | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const claims = getAuthClaims()
  const isParent = claims?.role === 'parent'
  const isChild = claims?.role === 'child'
  const notificationsSupported = supportsNotifications()
  const [notificationPermission, setNotificationPermission] = useState<PermissionState>(() => currentNotificationPermission())
  const [notificationPrefs, setNotificationPrefs] = useState<NotificationSettings>(() => getNotificationSettings())
  const alarmTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const [tasks, setTasks] = useState<TaskWithStatusDto[]>([])
  const [confirm, setConfirm] = useState<null | { mode: 'task', task: TaskWithStatusDto } | { mode: 'custom', minutes: number }>(null)
  const [taskNote, setTaskNote] = useState('')
  const [customMinutes, setCustomMinutes] = useState('')
  const [customLabel, setCustomLabel] = useState('')
  const [rewards, setRewards] = useState<RewardHistoryItemDto[]>([])
  const [usage, setUsage] = useState<UsageSeriesDto | null>(null)
  const [usageLoading, setUsageLoading] = useState(false)
  const [usageError, setUsageError] = useState<string | null>(null)
  const [usagePresetKey, setUsagePresetKey] = useState<UsagePresetKey>('1d')
  const [viewportVariant, setViewportVariant] = useState<ViewportVariant>(() => detectViewportVariant())
  const usageOptions = useMemo<UsageOption[]>(() => {
    const windows = VARIANT_WINDOWS[viewportVariant]
    return USAGE_BASE_PRESETS.map(p => ({ ...p, windowMinutes: windows[p.key] }))
  }, [viewportVariant])
  const usagePreset = useMemo<UsageOption>(() => {
    const found = usageOptions.find(p => p.key === usagePresetKey)
    return (found ?? usageOptions[0])!
  }, [usageOptions, usagePresetKey])
  const [page, setPage] = useState(1)
  const perPage = 10
  const [rewardsOpen, setRewardsOpen] = useState(true)
  const [rewardsLoading, setRewardsLoading] = useState(false)
  // Track locally submitted tasks to avoid duplicate submissions until page reload or approval
  const [submitted, setSubmitted] = useState<Set<string>>(new Set())
  const usageRequestIdRef = useRef(0)

  const handleEnableNotifications = useCallback(async () => {
    try {
      const permission = await requestNotificationPermission()
      setNotificationPermission(permission)
      if (permission !== 'granted') return

      const config = await getConfig().catch(() => null)
      const vapid = config?.push_public_key || getVapidPublicKey()
      if (!vapid) {
        console.warn('push notifications unavailable: missing public key')
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

      const claims = getAuthClaims()
      const activeChild = claims?.child_id || childId
      if (activeChild) {
        await pushSubscribe(activeChild, subscription)
      }

      const prefs = getNotificationSettings()
      const nextPrefs = { ...prefs, enabled: true }
      saveNotificationSettings(nextPrefs)
      setNotificationPrefs(nextPrefs)

      if (typeof remaining === 'number') {
        void maybeNotifyRemaining(childId, remaining, displayName)
      }
    } catch (err) {
      console.warn('Failed to enable notifications', err)
    }
  }, [childId, displayName, remaining])

  useEffect(() => {
    if (!usageOptions.length) return
    if (!usageOptions.some(p => p.key === usagePresetKey)) {
      setUsagePresetKey(usageOptions[0].key)
    }
  }, [usageOptions, usagePresetKey])

  useEffect(() => {
    if (!notificationsSupported) {
      setNotificationPermission('unsupported')
      return
    }
    setNotificationPermission(currentNotificationPermission())
    setNotificationPrefs(getNotificationSettings())
  }, [notificationsSupported])

  useEffect(() => {
    const handler = (e: any) => setNotificationPrefs(e?.detail || getNotificationSettings())
    window.addEventListener('gamiscreen:notification-settings-changed', handler as EventListener)
    return () => window.removeEventListener('gamiscreen:notification-settings-changed', handler as EventListener)
  }, [])

  useEffect(() => {
    if (typeof window === 'undefined') return
    const updateVariant = () => setViewportVariant(detectViewportVariant())
    updateVariant()
    window.addEventListener('resize', updateVariant)
    let orientationQuery: MediaQueryList | null = null
    let orientationCleanup: (() => void) | null = null
    if (typeof window.matchMedia === 'function') {
      orientationQuery = window.matchMedia('(orientation: landscape)')
      const orientationListener = () => updateVariant()
      if (orientationQuery.addEventListener) {
        orientationQuery.addEventListener('change', orientationListener)
        orientationCleanup = () => orientationQuery?.removeEventListener('change', orientationListener)
      } else if (orientationQuery.addListener) {
        orientationQuery.addListener(orientationListener)
        orientationCleanup = () => orientationQuery?.removeListener(orientationListener)
      }
    }
    return () => {
      window.removeEventListener('resize', updateVariant)
      if (orientationCleanup) orientationCleanup()
    }
  }, [])

  async function load() {
    setLoading(true)
    setError(null)
    try {
      if (isParent) {
        try {
          const children = await listChildren()
          const found = children.find(c => c.id === childId)
          if (found) setDisplayName(found.display_name)
        } catch {
          // Ignore; child token may not access list
        }
      }
      // Anyone (parent or child) may list tasks for this child (includes last_done)
      try {
        const ts = await listChildTasks(childId)
        setTasks(ts)
      } catch (e) {
        // Non-fatal for this view
      }
      const rem = await getRemaining(childId)
      setRemaining(rem.remaining_minutes)
    } catch (e: any) {
      setError(e.message || 'Failed to load')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [childId])
  // Live update remaining via SSE events (relayed by App via window event)
  useEffect(() => {
    const handler = (e: any) => {
      if (e?.detail?.child_id === childId && typeof e.detail.remaining_minutes === 'number') {
        setRemaining(e.detail.remaining_minutes)
      }
    }
    window.addEventListener('gamiscreen:remaining-updated', handler as EventListener)
    return () => window.removeEventListener('gamiscreen:remaining-updated', handler as EventListener)
  }, [childId])

  useEffect(() => {
    if (!isChild) return
    if (notificationPermission !== 'granted') return
    if (typeof remaining !== 'number') return
    const prefs = getNotificationSettings()
    setNotificationPrefs(prefs)
    void maybeNotifyRemaining(childId, remaining, displayName)
  }, [isChild, notificationPermission, remaining, childId, displayName])

  useEffect(() => {
    if (!isChild) return
    if (notificationPermission !== 'granted') {
      if (alarmTimerRef.current) {
        clearTimeout(alarmTimerRef.current)
        alarmTimerRef.current = null
      }
      return
    }
    if (typeof remaining !== 'number' || remaining <= 0) {
      if (alarmTimerRef.current) {
        clearTimeout(alarmTimerRef.current)
        alarmTimerRef.current = null
      }
      return
    }

    const prefs = notificationPrefs
    if (!prefs.enabled) {
      if (alarmTimerRef.current) {
        clearTimeout(alarmTimerRef.current)
        alarmTimerRef.current = null
      }
      return
    }

    if (remaining > prefs.thresholdMinutes) {
      if (alarmTimerRef.current) {
        clearTimeout(alarmTimerRef.current)
        alarmTimerRef.current = null
      }
      return
    }

    const now = new Date()
    const millisUntilNextMinute = (60 - now.getSeconds()) * 1000 - now.getMilliseconds()
    const delay = Math.max(0, millisUntilNextMinute + (remaining - 1) * 60 * 1000)

    if (alarmTimerRef.current) clearTimeout(alarmTimerRef.current)
    alarmTimerRef.current = setTimeout(() => {
      maybeNotifyRemaining(childId, 0, displayName)
      alarmTimerRef.current = null
    }, delay)

    return () => {
      if (alarmTimerRef.current) {
        clearTimeout(alarmTimerRef.current)
        alarmTimerRef.current = null
      }
    }
  }, [childId, displayName, isChild, notificationPermission, notificationPrefs, remaining])
  async function loadRewards(nextPage = page) {
    try {
      setRewardsLoading(true)
      const rh = await listChildRewards(childId, nextPage, perPage)
      setRewards(rh)
    } catch { }
    finally { setRewardsLoading(false) }
  }

  useEffect(() => { loadRewards(page) }, [childId, page])
  const loadUsageData = useCallback(async () => {
    if (!usagePreset) return
    const fetchDays = Math.max(1, Math.ceil(usagePreset.windowMinutes / MINUTES_PER_DAY))
    const targetBuckets = Math.max(1, Math.ceil(usagePreset.windowMinutes / usagePreset.bucketMinutes))
    const requestId = ++usageRequestIdRef.current
    setUsageLoading(true)
    setUsageError(null)
    try {
      const data = await listChildUsage(childId, { days: fetchDays, bucket_minutes: usagePreset.bucketMinutes })
      if (usageRequestIdRef.current === requestId) {
        const trimmedBuckets = data.buckets.slice(-targetBuckets)
        const trimmedTotal = trimmedBuckets.reduce((acc, bucket) => acc + bucket.minutes, 0)
        setUsage({
          ...data,
          buckets: trimmedBuckets,
          total_minutes: trimmedTotal,
        })
      }
    } catch (e: any) {
      if (usageRequestIdRef.current === requestId) {
        const msg = e?.message || 'Failed to load usage'
        setUsageError(typeof msg === 'string' ? msg : 'Failed to load usage')
      }
    } finally {
      if (usageRequestIdRef.current === requestId) {
        setUsageLoading(false)
      }
    }
  }, [childId, usagePreset])

  useEffect(() => {
    setUsage(null)
    loadUsageData()
  }, [loadUsageData])
  useEffect(() => {
    const id = setInterval(() => { load() }, 60_000)
    return () => clearInterval(id)
  }, [childId])

  async function doConfirm() {
    if (!confirm) return
    setLoading(true)
    setError(null)
    try {
      if (confirm.mode === 'task') {
        const description = taskNote.trim() || null
        const resp = await rewardMinutes({
          child_id: childId,
          task_id: confirm.task.id,
          minutes: null,
          description,
        })
        setRemaining(resp.remaining_minutes)
        // Update last_done locally for immediate UI feedback
        const nowIso = new Date().toISOString()
        setTasks(prev => prev.map(t => t.id === confirm.task.id ? { ...t, last_done: nowIso } : t))
      } else {
        const mins = confirm.minutes
        const description = customLabel.trim() || null
        const resp = await rewardMinutes({
          child_id: childId,
          task_id: null,
          minutes: mins,
          description,
        })
        setRemaining(resp.remaining_minutes)
      }
      setConfirm(null)
      setTaskNote('')
      setCustomMinutes('')
      setCustomLabel('')
      // Refresh reward history (show newest on first page)
      setPage(1)
      await loadRewards(1)
    } catch (e: any) {
      setError(e.message || 'Failed to add time')
    } finally {
      setLoading(false)
    }
  }

  return (
    <section className="col" style={{ gap: 12 }}>
      <header className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
        <h2 className="title" style={{ margin: 0 }}>{displayName}</h2>
        <div className="row" style={{ gap: 8 }}>
          <button
            className="secondary outline iconButton"
            onClick={load}
            disabled={loading}
            aria-label="Refresh"
            title={loading ? 'Refreshing…' : 'Refresh'}
          >
            ↻
          </button>
        </div>
      </header>
      {error && <p className="error">{error}</p>}
      <div className="card" style={{ padding: '12px' }}>
        <div className="row" style={{ justifyContent: 'space-between' }}>
          <div>Remaining</div>
          <div style={{ color: (typeof remaining === 'number' && remaining < 0) ? '#d00' as const : undefined }}>
            {remaining ?? '—'} min
          </div>
        </div>
      </div>
      {isChild && notificationsSupported && notificationPermission !== 'granted' && (
        <div className="card" style={{ padding: '12px' }}>
          <h3 className="title" style={{ fontSize: 16, marginBottom: 8 }}>Notifications</h3>
          <p className="subtitle">Enable notifications to get an alert 5 minutes before time runs out.</p>
          {notificationPermission === 'default' ? (
            <button type="button" className="acceptButton" onClick={handleEnableNotifications}>
              Enable notifications
            </button>
          ) : (
            <p className="error">Notifications are blocked in this browser. Update browser settings to enable them.</p>
          )}
        </div>
      )}
      <div className="card" style={{ padding: '12px' }}>
        <h3 className="title" style={{ fontSize: 16, marginBottom: 8 }}>Tasks</h3>
        <div className="col" style={{ gap: 6 }}>
          {tasks.map(t => {
            const last = t.last_done ? new Date(t.last_done) : null
            const todayStr = new Date().toISOString().slice(0, 10)
            const isDoneToday = last ? last.toISOString().slice(0, 10) === todayStr : false
            const wasSubmitted = submitted.has(t.id)
            const canClick = isParent || (isChild && !wasSubmitted && !isDoneToday)
            const isNegative = t.minutes < 0
            return (
              <div className={`row taskRow${isNegative ? ' taskRowNegative' : ''}`} key={t.id}>
                <div className="row taskRowHeader">
                  <span>{t.name}</span>
                  {isDoneToday && (
                    <mark title={last?.toLocaleString() || ''}>Done</mark>
                  )}
                </div>
                <div className="row taskRowActions">
                  <span className={`subtitle${isNegative ? ' negativeMinutes' : ''}`}>{t.minutes > 0 ? '+' : ''}{t.minutes} min</span>
                  {isParent && (
                    <button
                      className={isDoneToday ? 'contrast' : undefined}
                      onClick={() => {
                        setTaskNote('')
                        setConfirm({ mode: 'task', task: t })
                      }}
                    >Accept</button>
                  )}
                  {isChild && (
                    wasSubmitted || isDoneToday ? (
                      <button className="secondary" disabled title={isDoneToday ? 'Already done today' : 'Submitted for approval'}>
                        {isDoneToday ? 'Done' : 'Submitted'}
                      </button>
                    ) : (
                      <button onClick={async () => {
                        try {
                          await submitTask(childId, t.id)
                          setError(null)
                          setSubmitted(prev => {
                            const next = new Set(prev)
                            next.add(t.id)
                            return next
                          })
                        } catch (e: any) {
                          setError(e.message || 'Failed to submit task')
                        }
                      }}>Submit</button>
                    )
                  )}
                </div>
              </div>
            )
          })}
          {tasks.length === 0 && <p className="subtitle">No tasks</p>}
        </div>
      </div>
      {isParent && (
        <div className="card" style={{ padding: '12px' }}>
          <h3 className="title" style={{ fontSize: 16, marginBottom: 8 }}>Custom</h3>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              const n = parseInt(customMinutes, 10);
              if (Number.isFinite(n) && n !== 0) {
                setConfirm({ mode: 'custom', minutes: n });
              }
            }}
            className="col"
            style={{ gap: 8 }}
          >
            <div className="row" style={{ gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
              <input
                type="number"
                min={-100000}
                step={1}
                placeholder="15 or -15"
                aria-label="Minutes"
                value={customMinutes}
                onChange={e => setCustomMinutes(e.target.value)}
                inputMode="numeric"
                pattern="-?[0-9]*"
                style={{ width: '14ch', textAlign: 'right' }}
              />
              <span className="subtitle" style={{ whiteSpace: 'nowrap', alignSelf: 'center' }}>minutes</span>
            </div>
            <label className="col">
              <span>Description</span>
              <input
                type="text"
                placeholder="Optional description"
                value={customLabel}
                onChange={e => setCustomLabel(e.target.value)}
              />
            </label>
            <button type="submit" className="acceptButton">Accept</button>
          </form>
        </div>
      )}
      <div className="card" style={{ padding: '12px' }}>
        <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
          <h3 className="title" style={{ fontSize: 16, margin: 0 }}>Usage</h3>
          <button
            className="secondary outline iconButton"
            onClick={loadUsageData}
            disabled={usageLoading}
            aria-label="Refresh usage"
            title={usageLoading ? 'Refreshing…' : 'Refresh'}
          >
            ↻
          </button>
        </div>
        <div className="row usageControls" style={{ gap: 8, flexWrap: 'wrap', marginTop: 8 }}>
          {usageOptions.map(preset => {
            const active = preset.key === usagePreset.key
            return (
              <button
                key={preset.key}
                className={active ? 'contrast' : 'secondary'}
                onClick={() => {
                  if (!active) setUsagePresetKey(preset.key)
                }}
                disabled={usageLoading && active}
                aria-pressed={active}
              >
                {preset.label}
              </button>
            )
          })}
        </div>
        {usageError && <p className="error">{usageError}</p>}
        {usageLoading && !usage && <p className="subtitle">Loading usage…</p>}
        {usage && usage.buckets.length > 0 && (
          <UsageChart series={usage} />
        )}
        {!usageLoading && usage && usage.buckets.length === 0 && (
          <p className="subtitle">No usage recorded for this period.</p>
        )}
      </div>
      <div className="card" style={{ padding: '12px' }}>
        <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
          <h3 className="title" style={{ fontSize: 16, margin: 0 }}>Reward History</h3>
          <div className="row" style={{ gap: 8 }}>
            <button
              className="secondary outline iconButton"
              onClick={() => loadRewards(page)}
              disabled={rewardsLoading}
              aria-label="Refresh reward history"
              title={rewardsLoading ? 'Refreshing…' : 'Refresh'}
            >
              ↻
            </button>
            <button
              className="secondary outline iconButton"
              aria-expanded={rewardsOpen}
              aria-controls="reward-history"
              onClick={() => setRewardsOpen(v => !v)}
              title={rewardsOpen ? 'Collapse' : 'Expand'}
            >
              {rewardsOpen ? '▾' : '▸'}
            </button>
          </div>
        </div>
        {rewardsOpen && (
          <div id="reward-history" className="col" style={{ gap: 8, marginTop: 8 }}>
            <table role="grid">
              <thead>
                <tr><th>Time</th><th>Description</th><th>Minutes</th></tr>
              </thead>
              <tbody>
                {rewards.map((r, idx) => (
                  <tr key={idx}>
                    <td>{new Date(r.time).toLocaleString()}</td>
                    <td>{r.description ?? 'Additional time'}</td>
                    <td>{r.minutes > 0 ? '+' : ''}{r.minutes}</td>
                  </tr>
                ))}
                {rewards.length === 0 && (
                  <tr><td colSpan={3}><em>No rewards yet</em></td></tr>
                )}
              </tbody>
            </table>
            <div className="row" style={{ justifyContent: 'space-between' }}>
              <button className="secondary" disabled={page <= 1} onClick={() => setPage(p => Math.max(1, p - 1))}>Previous</button>
              <button className="secondary" disabled={rewards.length < perPage} onClick={() => setPage(p => p + 1)}>Next</button>
            </div>
          </div>
        )}
      </div>
      {confirm && (
        <dialog open>
          <article className="col" style={{ gap: 12 }}>
            <header>
              <strong>Confirm</strong>
            </header>
            <p className="subtitle">
              {confirm.mode === 'task'
                ? (() => {
                  const m = confirm.task.minutes
                  const isNegative = m < 0
                  return (
                    <>
                      {isNegative ? 'Apply ' : 'Add '}<strong>{m > 0 ? '+' : ''}{m}</strong> minutes for <strong>{displayName}</strong> by task "{confirm.task.name}"?
                    </>
                  )
                })()
                : (() => {
                  const m = confirm.minutes
                  const isNegative = m < 0
                  return (
                    <>
                      {isNegative ? 'Apply ' : 'Add '}<strong>{m > 0 ? '+' : ''}{m}</strong> custom minutes for <strong>{displayName}</strong>?
                    </>
                  )
                })()}
            </p>
            {confirm.mode === 'task' && (
              <label className="col" style={{ gap: 4 }}>
                <span>Note (optional)</span>
                <input
                  type="text"
                  placeholder="Add a note for this completion"
                  value={taskNote}
                  onChange={e => setTaskNote(e.target.value)}
                />
              </label>
            )}
            <footer className="row" style={{ gap: 8, justifyContent: 'flex-end' }}>
              <button onClick={doConfirm} disabled={loading}>Accept</button>
              <button className="secondary" onClick={() => { setConfirm(null); setTaskNote('') }} disabled={loading}>Cancel</button>
            </footer>
          </article>
        </dialog>
      )}
      {isParent && (
        <p>
          <a href="#status" className="subtitle">← Back to list</a>
        </p>
      )}
    </section>
  )
}
