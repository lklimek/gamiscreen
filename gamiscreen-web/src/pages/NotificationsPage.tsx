import { useEffect, useState } from 'react'
import { approveSubmission, discardSubmission, listNotifications, NotificationItemDto } from '../api'

export function NotificationsPage() {
  const [items, setItems] = useState<NotificationItemDto[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function load() {
    setLoading(true)
    setError(null)
    try {
      const data = await listNotifications()
      setItems(data)
    } catch (e: any) {
      setError(e.message || 'Failed to load notifications')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])

  async function onApprove(item: NotificationItemDto) {
    try {
      await approveSubmission(item.id)
      // Remove from local list immediately
      setItems(prev => prev.filter(x => x.id !== item.id))
      // Refresh header badge count
      window.dispatchEvent(new Event('gamiscreen:notif-refresh'))
      // Navigate to the child's page so the parent can see reward history updated
      window.location.hash = `child/${encodeURIComponent(item.child_id)}`
    } catch { }
  }
  async function onDiscard(id: number) {
    try {
      await discardSubmission(id)
      setItems(prev => prev.filter(x => x.id !== id))
      // Refresh header badge count
      window.dispatchEvent(new Event('gamiscreen:notif-refresh'))
    } catch { }
  }

  return (
    <section className="col" style={{ gap: 12 }}>
      <header className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
        <h2 className="title" style={{ margin: 0 }}>Notifications</h2>
        <div className="row" style={{ gap: 8 }}>
          <button className="secondary outline iconButton" onClick={load} disabled={loading} title={loading ? 'Refreshing…' : 'Refresh'} aria-label="Refresh">↻</button>
        </div>
      </header>
      {error && <p className="error">{error}</p>}
      <div className="card" style={{ padding: '12px' }}>
        {items.length === 0 && <p className="subtitle">No pending notifications</p>}
        {items.length > 0 && (
          <div className="col" style={{ gap: 8 }}>
            {items.map(item => (
              <div key={item.id} className="row" style={{ justifyContent: 'space-between', alignItems: 'center', borderBottom: '1px solid #eee', paddingBottom: 8 }}>
                <div className="col" style={{ gap: 2 }}>
                  <div><strong>{item.child_display_name}</strong> submitted: {item.task_name}</div>
                  <div className="subtitle">{new Date(item.submitted_at).toLocaleString()}</div>
                </div>
                <div className="row" style={{ gap: 8 }}>
                  <button className="secondary outline iconButton" onClick={() => onApprove(item)} title="Approve" aria-label="Approve">✔️</button>
                  <button className="secondary outline iconButton" onClick={() => onDiscard(item.id)} title="Discard" aria-label="Discard">✖️</button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
      <p>
        <a href="#status" className="subtitle">← Back to list</a>
      </p>
    </section>
  )
}
