import { useEffect, useState } from 'react'
import { ChildDto, getAuthClaims, getRemaining, listChildren } from '../api'

export function StatusPage() {
  const [rows, setRows] = useState<Array<{ child: ChildDto, remaining: number }>>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  async function load() {
    setLoading(true)
    setError(null)
    try {
      const claims = getAuthClaims()
      if (claims?.role === 'child') {
        if (!claims.child_id) throw new Error('Missing child id in token')
        const rem = await getRemaining(claims.child_id)
        setRows([{ child: { id: claims.child_id, display_name: claims.child_id }, remaining: rem.remaining_minutes }])
      } else {
        const cs = await listChildren()
        const rems = await Promise.all(cs.map(c => getRemaining(c.id)))
        setRows(cs.map((c, i) => ({ child: c, remaining: rems[i].remaining_minutes })))
      }
    } catch (e: any) {
      setError(e.message || 'Failed to load status')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { load() }, [])
  useEffect(() => {
    const id = setInterval(() => { load() }, 60_000)
    return () => clearInterval(id)
  }, [])

  return (
    <section className="col" style={{ gap: 12 }}>
      <header className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
        <h2 className="title" style={{ margin: 0 }}>Remaining Minutes</h2>
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
      <div className="grid">
        {rows.map(({ child, remaining }) => (
          <div key={child.id} className="card">
            <div className="row" style={{ justifyContent: 'space-between', alignItems: 'center' }}>
              <strong>{child.display_name}</strong>
              <a href={`#child/${encodeURIComponent(child.id)}`} role="button" className="secondary">Details</a>
            </div>
            <div className="spacer" />
            <div className="col" style={{ gap: 4 }}>
              <span className="subtitle">Remaining</span>
              <div
                style={{
                  fontSize: '1.25rem',
                  fontWeight: 700,
                  color: remaining < 0 ? '#d00' : undefined,
                }}
              >
                {remaining} min
              </div>
            </div>
          </div>
        ))}
      </div>
    </section>
  )
}
