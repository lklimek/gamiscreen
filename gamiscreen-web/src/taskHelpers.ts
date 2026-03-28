/** Day bitmask constants: Mon=1, Tue=2, Wed=4, Thu=8, Fri=16, Sat=32, Sun=64 */
export const DAY_BITS = [1, 2, 4, 8, 16, 32, 64] as const;
export const DAY_LABELS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"] as const;
export const DAY_SHORT_LABELS = ["M", "T", "W", "T", "F", "S", "S"] as const;

export const ALL_DAYS = 127; // 1+2+4+8+16+32+64
export const WEEKDAYS = 31; // 1+2+4+8+16
export const WEEKENDS = 96; // 32+64

/**
 * Format a mandatory_days bitmask into a human-readable string.
 * - 127 -> "Every day"
 * - 31 -> "Mon-Fri"
 * - 96 -> "Sat-Sun"
 * - Otherwise list individual days: "Mon, Wed, Fri"
 */
export function formatMandatoryDays(bitmask: number): string {
  if (bitmask === ALL_DAYS) return "Every day";
  if (bitmask === WEEKDAYS) return "Mon\u2013Fri";
  if (bitmask === WEEKENDS) return "Sat\u2013Sun";
  if (bitmask === 0) return "";

  const days: string[] = [];
  for (let i = 0; i < 7; i++) {
    if (bitmask & DAY_BITS[i]) {
      days.push(DAY_LABELS[i]);
    }
  }
  return days.join(", ");
}

/** Priority label from numeric value */
export function priorityLabel(priority: number): string {
  switch (priority) {
    case 1:
      return "High";
    case 3:
      return "Low";
    default:
      return "Medium";
  }
}

/** Priority CSS color for dots */
export function priorityColor(priority: number): string {
  switch (priority) {
    case 1:
      return "#dc2626"; // red
    case 3:
      return "#9ca3af"; // gray
    default:
      return "#eab308"; // yellow
  }
}
