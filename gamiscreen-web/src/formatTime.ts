/** Format minutes as "1h 15m" when >= 60, or "47 min" when < 60. */
export function formatMinutes(minutes: number): string {
  const abs = Math.abs(minutes);
  const sign = minutes < 0 ? "-" : "";
  if (abs >= 60) {
    const h = Math.floor(abs / 60);
    const m = abs % 60;
    return m > 0 ? `${sign}${h}h ${m}m` : `${sign}${h}h`;
  }
  return `${sign}${abs} min`;
}
