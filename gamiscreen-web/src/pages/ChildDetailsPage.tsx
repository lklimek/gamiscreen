import { useEffect, useState } from 'react'
import { getAuthClaims, getRemaining, listChildren, listChildRewards, listChildTasks, RewardHistoryItemDto, rewardMinutes, TaskWithStatusDto, submitTask } from '../api'

export function ChildDetailsPage(props: { childId: string }) {
  const { childId } = props
  const [displayName, setDisplayName] = useState<string>(childId)
  const [remaining, setRemaining] = useState<number | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const claims = getAuthClaims()
  const isParent = claims?.role === 'parent'
  const isChild = claims?.role === 'child'
  const [tasks, setTasks] = useState<TaskWithStatusDto[]>([])
  const [confirm, setConfirm] = useState<null | { mode: 'task', task: TaskWithStatusDto } | { mode: 'custom', minutes: number }>(null)
  const [customMinutes, setCustomMinutes] = useState('')
  const [customLabel, setCustomLabel] = useState('')
  const [rewards, setRewards] = useState<RewardHistoryItemDto[]>([])
  const [page, setPage] = useState(1)
  const perPage = 10
  const [rewardsOpen, setRewardsOpen] = useState(true)
  const [rewardsLoading, setRewardsLoading] = useState(false)
  // Track locally submitted tasks to avoid duplicate submissions until page reload or approval
  const [submitted, setSubmitted] = useState<Set<string>>(new Set())

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
  async function loadRewards(nextPage = page) {
    try {
      setRewardsLoading(true)
      const rh = await listChildRewards(childId, nextPage, perPage)
      setRewards(rh)
    } catch { }
    finally { setRewardsLoading(false) }
  }

  useEffect(() => { loadRewards(page) }, [childId, page])
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
        const resp = await rewardMinutes({ child_id: childId, task_id: confirm.task.id })
        setRemaining(resp.remaining_minutes)
        // Update last_done locally for immediate UI feedback
        const nowIso = new Date().toISOString()
        setTasks(prev => prev.map(t => t.id === confirm.task.id ? { ...t, last_done: nowIso } : t))
      } else {
        const mins = confirm.minutes
        const description = customLabel.trim() || undefined
        const resp = await rewardMinutes({ child_id: childId, minutes: mins, description })
        setRemaining(resp.remaining_minutes)
      }
      setConfirm(null)
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
            className="secondary outline"
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
      <div className="card" style={{ padding: '12px' }}>
        <h3 className="title" style={{ fontSize: 16, marginBottom: 8 }}>Tasks</h3>
        <div className="col" style={{ gap: 6 }}>
          {tasks.map(t => {
            const last = t.last_done ? new Date(t.last_done) : null
            const todayStr = new Date().toISOString().slice(0, 10)
            const isDoneToday = last ? last.toISOString().slice(0, 10) === todayStr : false
            const wasSubmitted = submitted.has(t.id)
            const canClick = isParent || (isChild && !wasSubmitted && !isDoneToday)
            return (
              <div className="row" key={t.id} style={{ justifyContent: 'space-between', alignItems: 'center' }}>
                <div>
                  {t.name}
                  {isDoneToday && (
                    <mark title={last?.toLocaleString() || ''} style={{ marginLeft: 8 }}>Done</mark>
                  )}
                </div>
                <div className="row" style={{ gap: 8, alignItems: 'center' }}>
                  <span className="subtitle">+{t.minutes} min</span>
                  {isParent && (
                    <button className={isDoneToday ? 'contrast' : undefined} onClick={() => setConfirm({ mode: 'task', task: t })}>Accept</button>
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
              if (Number.isFinite(n) && n > 0) {
                setConfirm({ mode: 'custom', minutes: n });
              }
            }}
            className="col"
            style={{ gap: 8 }}
          >
            <div className="row" style={{ gap: 8, alignItems: 'center', flexWrap: 'wrap' }}>
              <input
                type="number"
                min={1}
                step={1}
                placeholder="15"
                aria-label="Minutes"
                value={customMinutes}
                onChange={e => setCustomMinutes(e.target.value)}
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
          <h3 className="title" style={{ fontSize: 16, margin: 0 }}>Reward History</h3>
          <div className="row" style={{ gap: 8 }}>
            <button
              className="secondary outline"
              onClick={() => loadRewards(page)}
              disabled={rewardsLoading}
              aria-label="Refresh reward history"
              title={rewardsLoading ? 'Refreshing…' : 'Refresh'}
            >
              ↻
            </button>
            <button
              className="secondary"
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
                    <td>+{r.minutes}</td>
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
          <article>
            <header>
              <strong>Confirm</strong>
            </header>
            <p className="subtitle">
              {confirm.mode === 'task'
                ? (<>
                  Add <strong>{confirm.task.minutes}</strong> minutes for <strong>{displayName}</strong> by task "{confirm.task.name}"?
                </>)
                : (<>
                  Add <strong>{confirm.minutes}</strong> custom minutes for <strong>{displayName}</strong>?
                </>)}
            </p>
            <footer className="row" style={{ gap: 8, justifyContent: 'flex-end' }}>
              <button onClick={doConfirm} disabled={loading}>Accept</button>
              <button className="secondary" onClick={() => setConfirm(null)} disabled={loading}>Cancel</button>
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
