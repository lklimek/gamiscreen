import type { UsageSeriesDto } from '../api'

export const MINUTES_PER_HOUR = 60
export const MINUTES_PER_DAY = 24 * MINUTES_PER_HOUR
export const MINUTES_PER_WEEK = 7 * MINUTES_PER_DAY

export function UsageChart(props: { series: UsageSeriesDto }) {
  const { series } = props
  const buckets = series.buckets
  if (!buckets.length) return null
  const bucketMinutes = series.bucket_minutes || 0
  const max = buckets.reduce((acc, bucket) => Math.max(acc, bucket.minutes), 0)
  const showValues = buckets.length <= 40
  const showBreakdown = buckets.length <= 60

  return (
    <div className="col" style={{ gap: 12 }}>
      <div className="usageChart" role="list" aria-label="Usage minutes per period">
        {buckets.map(bucket => {
          const date = new Date(bucket.start)
          const { short: shortLabel, detail: detailLabel } = getBucketLabels(date, bucketMinutes)
          const ratio = max > 0 ? bucket.minutes / max : 0
          const percent = ratio === 0 ? 0 : Math.min(100, Math.max(10, ratio * 100))
          const heightStyle = bucket.minutes === 0 ? '4px' : `${percent}%`
          return (
            <div
              key={bucket.start}
              className="usageBar"
              role="listitem"
              aria-label={`${detailLabel}: ${bucket.minutes} minutes`}
              title={`${detailLabel}: ${bucket.minutes} minutes`}
            >
              <div
                className="usageBarFill"
                data-empty={bucket.minutes === 0}
                style={{ height: heightStyle }}
              >
                {showValues && bucket.minutes > 0 && <span>{bucket.minutes}</span>}
              </div>
              <div className="usageBarLabel">{shortLabel}</div>
            </div>
          )
        })}
      </div>
      {showBreakdown && (
        <ul className="usageBreakdown">
          {buckets.map(bucket => {
            const date = new Date(bucket.start)
            const { detail } = getBucketLabels(date, bucketMinutes)
            return (
              <li key={`summary-${bucket.start}`}>
                <span>{detail}</span>
                <span>{bucket.minutes} min</span>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}

function getBucketLabels(start: Date, bucketMinutes: number): { short: string, detail: string } {
  if (Number.isNaN(start.getTime()) || bucketMinutes <= 0) {
    return { short: '—', detail: '—' }
  }
  const endExclusive = new Date(start.getTime() + bucketMinutes * 60 * 1000)
  const endInclusive = new Date(endExclusive.getTime() - 60 * 1000)

  const short = (() => {
    if (bucketMinutes < MINUTES_PER_HOUR) {
      return start.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
    }
    if (bucketMinutes < MINUTES_PER_DAY) {
      return start.toLocaleString(undefined, { weekday: 'short', hour: 'numeric' })
    }
    return start.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
  })()

  const includeTime = bucketMinutes < MINUTES_PER_DAY
  const sameYear = start.getFullYear() === endInclusive.getFullYear()
  const detailStartOptions: Intl.DateTimeFormatOptions = includeTime
    ? { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', weekday: 'short' }
    : { month: 'short', day: 'numeric', year: sameYear ? undefined : 'numeric' }
  const detailEndOptions: Intl.DateTimeFormatOptions = includeTime
    ? { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', weekday: 'short' }
    : { month: 'short', day: 'numeric', year: sameYear ? undefined : 'numeric' }
  const detail = `${start.toLocaleString(undefined, detailStartOptions)} – ${endInclusive.toLocaleString(undefined, detailEndOptions)}`

  return { short, detail }
}
