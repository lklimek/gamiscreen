import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  getAuthClaims,
  getConfig,
  getRemaining,
  listChildren,
  listChildRewards,
  listChildTasks,
  listChildUsage,
  pushSubscribe,
  RewardHistoryItemDto,
  rewardMinutes,
  submitTask,
  TaskWithStatusDto,
  UsageSeriesDto,
} from "../api";
import {
  base64UrlToUint8Array,
  currentNotificationPermission,
  getNotificationSettings,
  getVapidPublicKey,
  saveNotificationSettings,
  requestNotificationPermission,
  supportsNotifications,
  type PermissionState,
} from "../notifications";
import {
  MINUTES_PER_DAY,
  MINUTES_PER_HOUR,
  MINUTES_PER_WEEK,
  UsageChart,
} from "../components/UsageChart";
import { formatMinutes } from "../formatTime";
import { formatMandatoryDays } from "../taskHelpers";

/** Determine if a task was completed today (browser local date). */
function isDoneToday(lastDone: string | null): boolean {
  if (!lastDone) return false;
  const last = new Date(lastDone);
  const todayStr = new Date().toISOString().slice(0, 10);
  return last.toISOString().slice(0, 10) === todayStr;
}

/**
 * Classify a task into "now" or "later".
 *
 * "Now": optional tasks (mandatory_days === 0) + mandatory tasks that are
 *        currently due (is_currently_blocking) or already done today.
 * "Later": mandatory tasks not currently due and not done today.
 */
function classifyTask(t: TaskWithStatusDto): "now" | "later" {
  const isOptional = t.mandatory_days === 0;
  if (isOptional) return "now";
  // Mandatory task — show in "Now" if currently blocking or done today
  if (t.is_currently_blocking || isDoneToday(t.last_done)) return "now";
  return "later";
}

const USAGE_BASE_PRESETS = [
  { key: "1h", label: "1 hour", bucketMinutes: MINUTES_PER_HOUR },
  { key: "1d", label: "1 day", bucketMinutes: MINUTES_PER_DAY },
  { key: "1w", label: "1 week", bucketMinutes: MINUTES_PER_WEEK },
] as const;

/** Compute repayment feedback message when earning while in debt. */
function computeRepaymentFeedback(
  balanceBefore: number,
  balanceAfter: number,
  earnedMinutes: number,
): string | null {
  const repaid = Math.max(
    0,
    Math.min(
      earnedMinutes,
      Math.abs(balanceBefore) - Math.abs(Math.min(balanceAfter, 0)),
    ),
  );
  const added = earnedMinutes - repaid;
  if (repaid <= 0) return null;
  return (
    `${formatMinutes(earnedMinutes)} earned: ${formatMinutes(repaid)} repaid debt` +
    (added > 0 ? `, ${formatMinutes(added)} added to screen time` : "")
  );
}

type UsageBasePreset = (typeof USAGE_BASE_PRESETS)[number];
type UsagePresetKey = UsageBasePreset["key"];
type UsageOption = UsageBasePreset & { windowMinutes: number };
type ViewportVariant = "mobilePortrait" | "mobileLandscape" | "desktop";

const VARIANT_WINDOWS: Record<
  ViewportVariant,
  Record<UsagePresetKey, number>
> = {
  mobilePortrait: {
    "1h": 6 * MINUTES_PER_HOUR,
    "1d": 7 * MINUTES_PER_DAY,
    "1w": 8 * MINUTES_PER_WEEK,
  },
  mobileLandscape: {
    "1h": 12 * MINUTES_PER_HOUR,
    "1d": 14 * MINUTES_PER_DAY,
    "1w": 16 * MINUTES_PER_WEEK,
  },
  desktop: {
    "1h": 24 * MINUTES_PER_HOUR,
    "1d": 14 * MINUTES_PER_DAY,
    "1w": 16 * MINUTES_PER_WEEK,
  },
} as const;

function detectViewportVariant(): ViewportVariant {
  if (typeof window === "undefined") return "desktop";
  const width = window.innerWidth;
  if (width >= 1024) return "desktop";
  const isLandscape =
    typeof window.matchMedia === "function"
      ? window.matchMedia("(orientation: landscape)").matches
      : width > window.innerHeight;
  return isLandscape ? "mobileLandscape" : "mobilePortrait";
}

export function ChildDetailsPage(props: { childId: string }) {
  const { childId } = props;
  const [displayName, setDisplayName] = useState<string>(childId);
  const [remaining, setRemaining] = useState<number | null>(null);
  const [balance, setBalance] = useState<number | null>(null);
  const [blocked, setBlocked] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const claims = getAuthClaims();
  const isParent = claims?.role === "parent";
  const isChild = claims?.role === "child";
  const notificationsSupported = supportsNotifications();
  const [notificationPermission, setNotificationPermission] =
    useState<PermissionState>(() => currentNotificationPermission());
  const [notificationsEnabled, setNotificationsEnabled] = useState<boolean>(
    () => getNotificationSettings().enabled,
  );
  const [tasks, setTasks] = useState<TaskWithStatusDto[]>([]);
  const [confirm, setConfirm] = useState<
    | null
    | { mode: "task"; task: TaskWithStatusDto }
    | { mode: "custom"; minutes: number; isBorrowed: boolean }
  >(null);
  const [taskNote, setTaskNote] = useState("");
  const [customMinutes, setCustomMinutes] = useState("");
  const [customLabel, setCustomLabel] = useState("");
  const [isBorrowed, setIsBorrowed] = useState(false);
  const [rewardFeedback, setRewardFeedback] = useState<string | null>(null);
  const [rewards, setRewards] = useState<RewardHistoryItemDto[]>([]);
  const [usage, setUsage] = useState<UsageSeriesDto | null>(null);
  const [usageLoading, setUsageLoading] = useState(false);
  const [usageError, setUsageError] = useState<string | null>(null);
  const [usagePresetKey, setUsagePresetKey] = useState<UsagePresetKey>("1d");
  const [viewportVariant, setViewportVariant] = useState<ViewportVariant>(() =>
    detectViewportVariant(),
  );
  const usageOptions = useMemo<UsageOption[]>(() => {
    const windows = VARIANT_WINDOWS[viewportVariant];
    return USAGE_BASE_PRESETS.map((p) => ({
      ...p,
      windowMinutes: windows[p.key],
    }));
  }, [viewportVariant]);
  const usagePreset = useMemo<UsageOption>(() => {
    const found = usageOptions.find((p) => p.key === usagePresetKey);
    return (found ?? usageOptions[0])!;
  }, [usageOptions, usagePresetKey]);
  const [page, setPage] = useState(1);
  const perPage = 10;
  const [rewardsOpen, setRewardsOpen] = useState(true);
  const [rewardsLoading, setRewardsLoading] = useState(false);
  // Track locally submitted tasks to avoid duplicate submissions until page reload or approval
  const [submitted, setSubmitted] = useState<Set<string>>(new Set());
  const usageRequestIdRef = useRef(0);

  const handleEnableNotifications = useCallback(async () => {
    try {
      const permission = await requestNotificationPermission();
      setNotificationPermission(permission);
      if (permission !== "granted") return;

      const config = await getConfig().catch(() => null);
      const vapid = config?.push_public_key || getVapidPublicKey();
      if (!vapid) {
        console.warn("push notifications unavailable: missing public key");
        return;
      }

      const registration = await navigator.serviceWorker.ready;
      let subscription = await registration.pushManager.getSubscription();
      if (!subscription) {
        const applicationServerKey = base64UrlToUint8Array(vapid)
          .buffer as ArrayBuffer;
        subscription = await registration.pushManager.subscribe({
          userVisibleOnly: true,
          applicationServerKey,
        });
      }

      const claims = getAuthClaims();
      const activeChild = claims?.child_id || childId;
      if (activeChild) {
        await pushSubscribe(activeChild, subscription);
      }

      saveNotificationSettings({ enabled: true });
      setNotificationsEnabled(true);
    } catch (err) {
      console.warn("Failed to enable notifications", err);
    }
  }, [childId]);

  useEffect(() => {
    if (!usageOptions.length) return;
    if (!usageOptions.some((p) => p.key === usagePresetKey)) {
      setUsagePresetKey(usageOptions[0].key);
    }
  }, [usageOptions, usagePresetKey]);

  useEffect(() => {
    if (!notificationsSupported) {
      setNotificationPermission("unsupported");
      return;
    }
    setNotificationPermission(currentNotificationPermission());
    setNotificationsEnabled(getNotificationSettings().enabled);
  }, [notificationsSupported]);

  useEffect(() => {
    const handler = (e: any) =>
      setNotificationsEnabled(
        e?.detail?.enabled ?? getNotificationSettings().enabled,
      );
    window.addEventListener(
      "gamiscreen:notification-settings-changed",
      handler as EventListener,
    );
    return () =>
      window.removeEventListener(
        "gamiscreen:notification-settings-changed",
        handler as EventListener,
      );
  }, []);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const updateVariant = () => setViewportVariant(detectViewportVariant());
    updateVariant();
    window.addEventListener("resize", updateVariant);
    let orientationQuery: MediaQueryList | null = null;
    let orientationCleanup: (() => void) | null = null;
    if (typeof window.matchMedia === "function") {
      orientationQuery = window.matchMedia("(orientation: landscape)");
      const orientationListener = () => updateVariant();
      if (orientationQuery.addEventListener) {
        orientationQuery.addEventListener("change", orientationListener);
        orientationCleanup = () =>
          orientationQuery?.removeEventListener("change", orientationListener);
      } else if (orientationQuery.addListener) {
        orientationQuery.addListener(orientationListener);
        orientationCleanup = () =>
          orientationQuery?.removeListener(orientationListener);
      }
    }
    return () => {
      window.removeEventListener("resize", updateVariant);
      if (orientationCleanup) orientationCleanup();
    };
  }, []);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      if (isParent) {
        try {
          const children = await listChildren();
          const found = children.find((c) => c.id === childId);
          if (found) setDisplayName(found.display_name);
        } catch {
          // Ignore; child token may not access list
        }
      }
      // Anyone (parent or child) may list tasks for this child (includes last_done)
      try {
        const ts = await listChildTasks(childId);
        setTasks(ts);
      } catch (e) {
        // Non-fatal for this view
      }
      const rem = await getRemaining(childId);
      setRemaining(rem.remaining_minutes);
      setBalance(rem.balance);
      setBlocked(rem.blocked_by_tasks);
    } catch (e: any) {
      setError(e.message || "Failed to load");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
  }, [childId]);
  // Live update remaining via SSE events (relayed by App via window event)
  useEffect(() => {
    const handler = (e: any) => {
      if (
        e?.detail?.child_id === childId &&
        typeof e.detail.remaining_minutes === "number"
      ) {
        setRemaining(e.detail.remaining_minutes);
        if (typeof e.detail.balance === "number") setBalance(e.detail.balance);
        if (typeof e.detail.blocked_by_tasks === "boolean")
          setBlocked(e.detail.blocked_by_tasks);
      }
    };
    window.addEventListener(
      "gamiscreen:remaining-updated",
      handler as EventListener,
    );
    return () =>
      window.removeEventListener(
        "gamiscreen:remaining-updated",
        handler as EventListener,
      );
  }, [childId]);

  useEffect(() => {
    if (!isChild) return;
    if (notificationPermission !== "granted") return;
    setNotificationsEnabled(getNotificationSettings().enabled);
  }, [isChild, notificationPermission]);
  async function loadRewards(nextPage = page) {
    try {
      setRewardsLoading(true);
      const rh = await listChildRewards(childId, nextPage, perPage);
      setRewards(rh);
    } catch {
    } finally {
      setRewardsLoading(false);
    }
  }

  useEffect(() => {
    loadRewards(page);
  }, [childId, page]);
  const loadUsageData = useCallback(async () => {
    if (!usagePreset) return;
    const fetchDays = Math.max(
      1,
      Math.ceil(usagePreset.windowMinutes / MINUTES_PER_DAY),
    );
    const targetBuckets = Math.max(
      1,
      Math.ceil(usagePreset.windowMinutes / usagePreset.bucketMinutes),
    );
    const requestId = ++usageRequestIdRef.current;
    setUsageLoading(true);
    setUsageError(null);
    try {
      const data = await listChildUsage(childId, {
        days: fetchDays,
        bucket_minutes: usagePreset.bucketMinutes,
      });
      if (usageRequestIdRef.current === requestId) {
        const trimmedBuckets = data.buckets.slice(-targetBuckets);
        const trimmedTotal = trimmedBuckets.reduce(
          (acc, bucket) => acc + bucket.minutes,
          0,
        );
        setUsage({
          ...data,
          buckets: trimmedBuckets,
          total_minutes: trimmedTotal,
        });
      }
    } catch (e: any) {
      if (usageRequestIdRef.current === requestId) {
        const msg = e?.message || "Failed to load usage";
        setUsageError(typeof msg === "string" ? msg : "Failed to load usage");
      }
    } finally {
      if (usageRequestIdRef.current === requestId) {
        setUsageLoading(false);
      }
    }
  }, [childId, usagePreset]);

  useEffect(() => {
    setUsage(null);
    loadUsageData();
  }, [loadUsageData]);
  useEffect(() => {
    const id = setInterval(() => {
      load();
    }, 60_000);
    return () => clearInterval(id);
  }, [childId]);

  async function doConfirm() {
    if (!confirm) return;
    setLoading(true);
    setError(null);
    setRewardFeedback(null);
    try {
      const balanceBefore = balance ?? 0;
      let earnedMinutes = 0;

      if (confirm.mode === "task") {
        earnedMinutes = confirm.task.minutes;
        const description = taskNote.trim() || null;
        const resp = await rewardMinutes({
          child_id: childId,
          task_id: confirm.task.id,
          minutes: null,
          description,
          is_borrowed: null,
        });
        setRemaining(resp.remaining_minutes);
        setBalance(resp.balance);
        const nowIso = new Date().toISOString();
        setTasks((prev) =>
          prev.map((t) =>
            t.id === confirm.task.id ? { ...t, last_done: nowIso } : t,
          ),
        );

        // Show auto-repayment feedback when debt was partially or fully repaid
        if (balanceBefore < 0 && earnedMinutes > 0) {
          const msg = computeRepaymentFeedback(
            balanceBefore,
            resp.balance,
            earnedMinutes,
          );
          if (msg) setRewardFeedback(msg);
        }
      } else {
        const mins = confirm.minutes;
        earnedMinutes = mins;
        const description = customLabel.trim() || null;
        const resp = await rewardMinutes({
          child_id: childId,
          task_id: null,
          minutes: mins,
          description,
          is_borrowed: confirm.isBorrowed || null,
        });
        setRemaining(resp.remaining_minutes);
        setBalance(resp.balance);

        // Show auto-repayment feedback for non-borrowed positive rewards during debt
        if (balanceBefore < 0 && mins > 0 && !confirm.isBorrowed) {
          const msg = computeRepaymentFeedback(
            balanceBefore,
            resp.balance,
            mins,
          );
          if (msg) setRewardFeedback(msg);
        }
      }
      setConfirm(null);
      setTaskNote("");
      setCustomMinutes("");
      setCustomLabel("");
      setIsBorrowed(false);
      // Refresh reward history (show newest on first page)
      setPage(1);
      await loadRewards(1);
    } catch (e: any) {
      setError(e.message || "Failed to add time");
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="col" style={{ gap: 12 }}>
      <header
        className="row"
        style={{ justifyContent: "space-between", alignItems: "center" }}
      >
        <h2 className="title" style={{ margin: 0 }}>
          {displayName}
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
      {rewardFeedback && (
        <div
          role="status"
          style={{
            padding: "8px 12px",
            borderRadius: 8,
            fontSize: 14,
            background: "#ecfdf5",
            color: "#065f46",
            border: "1px solid #a7f3d0",
          }}
        >
          {rewardFeedback}
        </div>
      )}
      {/* R-1: Hero "Time Left" display */}
      <div
        className="card"
        style={{ padding: "16px 12px", textAlign: "center" }}
      >
        <div style={{ fontSize: 14, color: "var(--muted-color, #666)" }}>
          Time Left
        </div>
        <div
          style={{
            fontSize: "2.25rem",
            fontWeight: 700,
            lineHeight: 1.2,
            color:
              typeof remaining === "number" && remaining <= 0
                ? "#d00"
                : undefined,
          }}
        >
          {typeof remaining === "number" ? formatMinutes(remaining) : "—"}
        </div>
        <div
          style={{
            marginTop: 4,
            fontSize: 14,
            color: "var(--muted-color, #666)",
          }}
        >
          {blocked ? "Locked (tasks needed)" : "Active"}
        </div>
        {/* R-3: Inline debt explanation — visible without expanding details */}
        {!blocked && typeof balance === "number" && balance < 0 && (
          <div
            role="status"
            style={{
              marginTop: 12,
              padding: "8px 12px",
              borderRadius: 8,
              fontSize: 14,
              textAlign: "left",
              background: "#fffbeb",
              color: "#92400e",
              border: "1px solid #fde68a",
            }}
          >
            {displayName} owes {formatMinutes(Math.abs(balance))}. Earned time
            pays off the debt first.
          </div>
        )}
        {blocked && (
          <div
            role="alert"
            style={{
              marginTop: 12,
              padding: "8px 12px",
              borderRadius: 8,
              fontSize: 14,
              textAlign: "left",
              background: "#fef2f2",
              color: "#dc2626",
              border: "1px solid #fecaca",
            }}
          >
            Complete required tasks to unlock screen time
          </div>
        )}
        {/* R-2: Balance in collapsible details section */}
        <details style={{ marginTop: 12, textAlign: "left" }}>
          <summary
            style={{
              cursor: "pointer",
              fontSize: 14,
              color: "var(--muted-color, #666)",
            }}
          >
            Details
          </summary>
          <div style={{ marginTop: 8, fontSize: 14 }}>
            <div className="row" style={{ justifyContent: "space-between" }}>
              <span>Account Balance</span>
              <span
                style={{
                  color:
                    typeof balance === "number" && balance < 0
                      ? "#b91c1c"
                      : undefined,
                }}
                aria-label={
                  typeof balance === "number" && balance < 0
                    ? `Account balance: negative ${Math.abs(balance)} minutes`
                    : undefined
                }
              >
                {typeof balance === "number"
                  ? balance === 0
                    ? "No debt"
                    : formatMinutes(balance)
                  : "—"}
              </span>
            </div>
            <div
              className="row"
              style={{ justifyContent: "space-between", marginTop: 4 }}
            >
              <span>Remaining</span>
              <span>
                {typeof remaining === "number" ? formatMinutes(remaining) : "—"}
              </span>
            </div>
          </div>
        </details>
      </div>
      {isChild &&
        notificationsSupported &&
        (!notificationsEnabled || notificationPermission !== "granted") && (
          <div className="card" style={{ padding: "12px" }}>
            <h3 className="title" style={{ fontSize: 16, marginBottom: 8 }}>
              Notifications
            </h3>
            <p className="subtitle">
              Enable notifications to get alerts when your remaining time
              changes.
            </p>
            {notificationPermission === "default" ? (
              <button
                type="button"
                className="acceptButton"
                onClick={handleEnableNotifications}
              >
                Enable notifications
              </button>
            ) : (
              <p className="error">
                Notifications are blocked in this browser. Update browser
                settings to enable them.
              </p>
            )}
          </div>
        )}
      <div className="card" style={{ padding: "12px" }}>
        <h3 className="title" style={{ fontSize: 16, marginBottom: 8 }}>
          Tasks
        </h3>
        {blocked && (
          <p
            role="alert"
            style={{ fontSize: 14, color: "#dc2626", margin: "0 0 8px" }}
          >
            Complete all starred (*) tasks to unlock screen time.
          </p>
        )}
        {(() => {
          const nowTasks: TaskWithStatusDto[] = [];
          const laterTasks: TaskWithStatusDto[] = [];
          for (const t of tasks) {
            if (classifyTask(t) === "now") {
              nowTasks.push(t);
            } else {
              laterTasks.push(t);
            }
          }
          // API returns pre-sorted by priority then name; preserve that order.

          const renderTaskRow = (
            t: TaskWithStatusDto,
            muted: boolean,
          ) => {
            const doneToday = isDoneToday(t.last_done);
            const last = t.last_done ? new Date(t.last_done) : null;
            const wasSubmitted = submitted.has(t.id);
            const isNegative = t.minutes < 0;
            const isBlocking =
              t.is_currently_blocking && !doneToday;

            // Style: blue left border for currently-blocking mandatory tasks
            const taskRowStyle: React.CSSProperties = isBlocking
              ? {
                  borderLeft: "4px solid #2563eb",
                  background: "#eff6ff",
                  borderRadius: 8,
                  paddingLeft: 12,
                }
              : {};

            // Muted style for "Later" tasks
            if (muted) {
              taskRowStyle.opacity = 0.55;
            }

            // Schedule info for mandatory tasks
            const scheduleInfo =
              t.mandatory_days > 0 && t.mandatory_start_time
                ? `Due at ${t.mandatory_start_time}`
                : t.mandatory_days > 0
                  ? formatMandatoryDays(t.mandatory_days)
                  : null;

            return (
              <div
                className={`row taskRow${isNegative ? " taskRowNegative" : ""}`}
                key={t.id}
                style={taskRowStyle}
                aria-label={
                  isBlocking
                    ? `Required task: ${t.name}, ${t.minutes} minutes, currently blocking`
                    : t.mandatory_days > 0
                      ? `Mandatory task: ${t.name}, ${t.minutes} minutes`
                      : `Task: ${t.name}, ${t.minutes} minutes`
                }
              >
                <div className="row taskRowHeader">
                  <span>
                    {isBlocking ? (
                      <strong>* {t.name}</strong>
                    ) : (
                      t.name
                    )}
                  </span>
                  {doneToday && (
                    <mark
                      style={{
                        background: "#dcfce7",
                        color: "#166534",
                        padding: "2px 8px",
                        borderRadius: 4,
                        fontSize: 12,
                        fontWeight: 600,
                      }}
                      title={last?.toLocaleString() || ""}
                    >
                      Done
                    </mark>
                  )}
                  {scheduleInfo && !doneToday && (
                    <span
                      style={{
                        fontSize: 12,
                        color: "var(--muted-color, #666)",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {scheduleInfo}
                    </span>
                  )}
                </div>
                <div className="row taskRowActions">
                  <span
                    className={`subtitle${isNegative ? " negativeMinutes" : ""}`}
                  >
                    {t.minutes > 0 ? "+" : ""}
                    {t.minutes} min
                  </span>
                  {isParent && (
                    <button
                      className={doneToday ? "contrast" : undefined}
                      onClick={() => {
                        setTaskNote("");
                        setConfirm({ mode: "task", task: t });
                      }}
                    >
                      Accept
                    </button>
                  )}
                  {isChild &&
                    (wasSubmitted || doneToday ? (
                      <button
                        className="secondary"
                        disabled
                        title={
                          doneToday
                            ? "Already done today"
                            : "Submitted for approval"
                        }
                      >
                        {doneToday ? "Done" : "Submitted"}
                      </button>
                    ) : (
                      <button
                        onClick={async () => {
                          try {
                            await submitTask(childId, t.id);
                            setError(null);
                            setSubmitted((prev) => {
                              const next = new Set(prev);
                              next.add(t.id);
                              return next;
                            });
                          } catch (e: any) {
                            setError(e.message || "Failed to submit task");
                          }
                        }}
                      >
                        Submit
                      </button>
                    ))}
                </div>
              </div>
            );
          };

          return (
            <>
              {/* "Now" section — always shown */}
              <div
                className="col"
                style={{ gap: 6 }}
                role="list"
                aria-label="Current tasks"
              >
                {nowTasks.length > 0 ? (
                  nowTasks.map((t) => renderTaskRow(t, false))
                ) : tasks.length === 0 ? (
                  <p className="subtitle">No tasks</p>
                ) : (
                  <p className="subtitle">All tasks are scheduled for later</p>
                )}
              </div>

              {/* "Later" section — hidden when empty */}
              {laterTasks.length > 0 && (
                <div style={{ marginTop: 16 }}>
                  <h4
                    className="subtitle"
                    style={{
                      fontSize: 13,
                      fontWeight: 600,
                      textTransform: "uppercase",
                      letterSpacing: "0.05em",
                      marginBottom: 6,
                      color: "var(--muted-color, #888)",
                    }}
                  >
                    Later
                  </h4>
                  <div
                    className="col"
                    style={{ gap: 6 }}
                    role="list"
                    aria-label="Upcoming tasks"
                  >
                    {laterTasks.map((t) => renderTaskRow(t, true))}
                  </div>
                </div>
              )}
            </>
          );
        })()}
      </div>
      {isParent && (
        <div className="card" style={{ padding: "12px" }}>
          <h3 className="title" style={{ fontSize: 16, marginBottom: 8 }}>
            Custom
          </h3>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              const n = parseInt(customMinutes, 10);
              if (isBorrowed && (!Number.isFinite(n) || n <= 0)) return;
              if (Number.isFinite(n) && n !== 0) {
                setConfirm({ mode: "custom", minutes: n, isBorrowed });
              }
            }}
            className="col"
            style={{ gap: 8 }}
          >
            <div
              className="row"
              style={{ gap: 8, alignItems: "center", flexWrap: "wrap" }}
            >
              <input
                type="number"
                min={isBorrowed ? 1 : -100000}
                step={1}
                placeholder={isBorrowed ? "30" : "15 or -15"}
                aria-label="Minutes"
                value={customMinutes}
                onChange={(e) => setCustomMinutes(e.target.value)}
                inputMode="numeric"
                pattern={isBorrowed ? "[0-9]*" : "-?[0-9]*"}
                style={{ width: "14ch", textAlign: "right" }}
              />
              <span
                className="subtitle"
                style={{ whiteSpace: "nowrap", alignSelf: "center" }}
              >
                minutes
              </span>
            </div>
            <label className="col">
              <span>Description</span>
              <input
                type="text"
                placeholder="Optional description"
                value={customLabel}
                onChange={(e) => setCustomLabel(e.target.value)}
              />
            </label>
            <label
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                cursor: "pointer",
              }}
            >
              <input
                type="checkbox"
                checked={isBorrowed}
                onChange={(e) => setIsBorrowed(e.target.checked)}
                style={{ margin: 0, width: "auto" }}
              />
              <span>Borrow (adds to remaining, creates debt)</span>
            </label>
            <button type="submit" className="acceptButton">
              {isBorrowed ? "Lend Time" : "Accept"}
            </button>
          </form>
        </div>
      )}
      <div className="card" style={{ padding: "12px" }}>
        <div
          className="row"
          style={{ justifyContent: "space-between", alignItems: "center" }}
        >
          <h3 className="title" style={{ fontSize: 16, margin: 0 }}>
            Usage
          </h3>
          <button
            className="secondary outline iconButton"
            onClick={loadUsageData}
            disabled={usageLoading}
            aria-label="Refresh usage"
            title={usageLoading ? "Refreshing…" : "Refresh"}
          >
            ↻
          </button>
        </div>
        <div
          className="row usageControls"
          style={{ gap: 8, flexWrap: "wrap", marginTop: 8 }}
        >
          {usageOptions.map((preset) => {
            const active = preset.key === usagePreset.key;
            return (
              <button
                key={preset.key}
                className={active ? "contrast" : "secondary"}
                onClick={() => {
                  if (!active) setUsagePresetKey(preset.key);
                }}
                disabled={usageLoading && active}
                aria-pressed={active}
              >
                {preset.label}
              </button>
            );
          })}
        </div>
        {usageError && <p className="error">{usageError}</p>}
        {usageLoading && !usage && <p className="subtitle">Loading usage…</p>}
        {usage && usage.buckets.length > 0 && <UsageChart series={usage} />}
        {!usageLoading && usage && usage.buckets.length === 0 && (
          <p className="subtitle">No usage recorded for this period.</p>
        )}
      </div>
      <div className="card" style={{ padding: "12px" }}>
        <div
          className="row"
          style={{ justifyContent: "space-between", alignItems: "center" }}
        >
          <h3 className="title" style={{ fontSize: 16, margin: 0 }}>
            Reward History
          </h3>
          <div className="row" style={{ gap: 8 }}>
            <button
              className="secondary outline iconButton"
              onClick={() => loadRewards(page)}
              disabled={rewardsLoading}
              aria-label="Refresh reward history"
              title={rewardsLoading ? "Refreshing…" : "Refresh"}
            >
              ↻
            </button>
            <button
              className="secondary outline iconButton"
              aria-expanded={rewardsOpen}
              aria-controls="reward-history"
              onClick={() => setRewardsOpen((v) => !v)}
              title={rewardsOpen ? "Collapse" : "Expand"}
            >
              {rewardsOpen ? "▾" : "▸"}
            </button>
          </div>
        </div>
        {rewardsOpen && (
          <div
            id="reward-history"
            className="col"
            style={{ gap: 8, marginTop: 8 }}
          >
            <table role="grid">
              <thead>
                <tr>
                  <th>Time</th>
                  <th>Description</th>
                  <th>Minutes</th>
                </tr>
              </thead>
              <tbody>
                {rewards.map((r, idx) => (
                  <tr
                    key={idx}
                    style={
                      r.is_borrowed ? { background: "#fffbeb" } : undefined
                    }
                  >
                    <td>{new Date(r.time).toLocaleString()}</td>
                    <td>{r.description ?? "Additional time"}</td>
                    <td>
                      {r.minutes > 0 ? "+" : ""}
                      {r.minutes}
                      {r.is_borrowed ? " (lent)" : ""}
                    </td>
                  </tr>
                ))}
                {rewards.length === 0 && (
                  <tr>
                    <td colSpan={3}>
                      <em>No rewards yet</em>
                    </td>
                  </tr>
                )}
              </tbody>
            </table>
            <div className="row" style={{ justifyContent: "space-between" }}>
              <button
                className="secondary"
                disabled={page <= 1}
                onClick={() => setPage((p) => Math.max(1, p - 1))}
              >
                Previous
              </button>
              <button
                className="secondary"
                disabled={rewards.length < perPage}
                onClick={() => setPage((p) => p + 1)}
              >
                Next
              </button>
            </div>
          </div>
        )}
      </div>
      {confirm && (
        <dialog open>
          <article className="col" style={{ gap: 12 }}>
            <header>
              <strong>
                {confirm.mode === "custom" && confirm.isBorrowed
                  ? "Confirm Borrow"
                  : "Confirm"}
              </strong>
            </header>
            <p className="subtitle">
              {confirm.mode === "task"
                ? (() => {
                    const m = confirm.task.minutes;
                    const isNegative = m < 0;
                    return (
                      <>
                        {isNegative ? "Apply " : "Add "}
                        <strong>
                          {m > 0 ? "+" : ""}
                          {m}
                        </strong>{" "}
                        minutes for <strong>{displayName}</strong> by task "
                        {confirm.task.name}"?
                      </>
                    );
                  })()
                : (() => {
                    const m = confirm.minutes;
                    if (confirm.isBorrowed) {
                      return (
                        <>
                          Lend <strong>+{m}</strong> minutes to{" "}
                          <strong>{displayName}</strong>?
                        </>
                      );
                    }
                    const isNegative = m < 0;
                    return (
                      <>
                        {isNegative ? "Apply " : "Add "}
                        <strong>
                          {m > 0 ? "+" : ""}
                          {m}
                        </strong>{" "}
                        custom minutes for <strong>{displayName}</strong>?
                      </>
                    );
                  })()}
            </p>
            {confirm.mode === "custom" && confirm.isBorrowed && (
              <p style={{ fontSize: 14, color: "var(--muted-color, #666)" }}>
                This adds {confirm.minutes} min of screen time and creates a
                debt of {confirm.minutes} min. Earned time will pay off the debt
                before adding to remaining.
              </p>
            )}
            {confirm.mode === "task" && (
              <label className="col" style={{ gap: 4 }}>
                <span>Note (optional)</span>
                <input
                  type="text"
                  placeholder="Add a note for this completion"
                  value={taskNote}
                  onChange={(e) => setTaskNote(e.target.value)}
                />
              </label>
            )}
            <footer
              className="row"
              style={{ gap: 8, justifyContent: "flex-end" }}
            >
              <button onClick={doConfirm} disabled={loading}>
                {confirm.mode === "custom" && confirm.isBorrowed
                  ? "Lend Time"
                  : "Accept"}
              </button>
              <button
                className="secondary"
                onClick={() => {
                  setConfirm(null);
                  setTaskNote("");
                }}
                disabled={loading}
              >
                Cancel
              </button>
            </footer>
          </article>
        </dialog>
      )}
      {isParent && (
        <p>
          <a href="#status" className="subtitle">
            ← Back to list
          </a>
        </p>
      )}
    </section>
  );
}
