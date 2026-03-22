import { useEffect, useState } from "react";
import { ChildDto, getAuthClaims, getRemaining, listChildren } from "../api";
import { formatMinutes } from "../formatTime";

export function StatusPage() {
  const [rows, setRows] = useState<
    Array<{
      child: ChildDto;
      remaining: number;
      balance: number;
      blocked: boolean;
    }>
  >([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const claims = getAuthClaims();
      if (claims?.role === "child") {
        if (!claims.child_id) throw new Error("Missing child id in token");
        const rem = await getRemaining(claims.child_id);
        setRows([
          {
            child: { id: claims.child_id, display_name: claims.child_id },
            remaining: rem.remaining_minutes,
            balance: rem.balance,
            blocked: rem.blocked_by_tasks,
          },
        ]);
      } else {
        const cs = await listChildren();
        const rems = await Promise.all(cs.map((c) => getRemaining(c.id)));
        setRows(
          cs.map((c, i) => ({
            child: c,
            remaining: rems[i].remaining_minutes,
            balance: rems[i].balance,
            blocked: rems[i].blocked_by_tasks,
          })),
        );
      }
    } catch (e: any) {
      setError(e.message || "Failed to load status");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
  }, []);
  useEffect(() => {
    const id = setInterval(() => {
      load();
    }, 60_000);
    return () => clearInterval(id);
  }, []);

  return (
    <section className="col" style={{ gap: 12 }}>
      <header
        className="row"
        style={{ justifyContent: "space-between", alignItems: "center" }}
      >
        <h2 className="title" style={{ margin: 0 }}>
          Screen Time
        </h2>
        <div className="row" style={{ gap: 8 }}>
          <button
            className="secondary outline iconButton"
            onClick={load}
            disabled={loading}
            aria-label="Refresh"
            title={loading ? "Refreshing…" : "Refresh"}
          >
            ↻
          </button>
        </div>
      </header>
      {error && <p className="error">{error}</p>}
      <div className="grid">
        {rows.map(({ child, remaining, balance, blocked }) => (
          <div key={child.id} className="card">
            <div
              className="row"
              style={{ justifyContent: "space-between", alignItems: "center" }}
            >
              <strong>{child.display_name}</strong>
              <a
                href={`#child/${encodeURIComponent(child.id)}`}
                role="button"
                className="secondary"
              >
                Details
              </a>
            </div>
            <div className="spacer" />
            <div
              style={{
                fontSize: "1.5rem",
                fontWeight: 700,
                color: remaining <= 0 ? "#d00" : undefined,
              }}
            >
              Time left: {formatMinutes(remaining)}
            </div>
            <div
              style={{
                marginTop: 6,
                fontSize: 14,
                color: "var(--muted-color, #666)",
              }}
            >
              {blocked ? "Locked (tasks needed)" : "Active"}
            </div>
            {/* R-3: Inline debt explanation — visible without expanding details */}
            {!blocked && balance < 0 && (
              <div
                role="status"
                style={{
                  marginTop: 8,
                  padding: "8px 12px",
                  borderRadius: 8,
                  fontSize: 14,
                  background: "#fffbeb",
                  color: "#92400e",
                  border: "1px solid #fde68a",
                }}
              >
                {child.display_name} owes {formatMinutes(Math.abs(balance))}.
                Earned time pays off the debt first.
              </div>
            )}
            {blocked && (
              <div
                role="alert"
                style={{
                  marginTop: 8,
                  padding: "8px 12px",
                  borderRadius: 8,
                  fontSize: 14,
                  background: "#fef2f2",
                  color: "#dc2626",
                  border: "1px solid #fecaca",
                }}
              >
                Complete required tasks to unlock screen time
              </div>
            )}
          </div>
        ))}
      </div>
    </section>
  );
}
