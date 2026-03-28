import { useCallback, useEffect, useRef, useState } from "react";
import {
  createTask,
  updateTask,
  deleteTask,
  getTaskManagement,
  listChildren,
  type ChildDto,
  type CreateTaskReq,
  type UpdateTaskReq,
} from "../api";
import {
  ALL_DAYS,
  DAY_BITS,
  DAY_SHORT_LABELS,
  WEEKDAYS,
} from "../taskHelpers";

interface TaskFormPageProps {
  taskId?: string;
}

export function TaskFormPage({ taskId }: TaskFormPageProps) {
  const isEdit = !!taskId;

  const [name, setName] = useState("");
  const [minutes, setMinutes] = useState("");
  const [priority, setPriority] = useState(2);
  const [mandatory, setMandatory] = useState(false);
  const [mandatoryDays, setMandatoryDays] = useState(ALL_DAYS);
  const [startTime, setStartTime] = useState("00:00");
  const [assignAll, setAssignAll] = useState(true);
  const [selectedChildren, setSelectedChildren] = useState<Set<string>>(
    new Set(),
  );

  const [children, setChildren] = useState<ChildDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [initialLoading, setInitialLoading] = useState(isEdit);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [showDeleteDialog, setShowDeleteDialog] = useState(false);
  const deleteDialogRef = useRef<HTMLDialogElement>(null);

  // Sync dialog open/close with showModal()/close() for proper modal behavior
  useEffect(() => {
    const dialog = deleteDialogRef.current;
    if (!dialog) return;
    if (showDeleteDialog && !dialog.open) {
      dialog.showModal();
    } else if (!showDeleteDialog && dialog.open) {
      dialog.close();
    }
  }, [showDeleteDialog]);

  // Validation
  const [nameError, setNameError] = useState<string | null>(null);
  const [minutesError, setMinutesError] = useState<string | null>(null);
  const [childrenError, setChildrenError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    async function init() {
      try {
        const childList = await listChildren();
        if (!cancelled) setChildren(childList);

        if (isEdit && taskId) {
          setInitialLoading(true);
          const task = await getTaskManagement(taskId);
          if (!cancelled) {
            setName(task.name);
            setMinutes(String(task.minutes));
            setPriority(task.priority);
            setMandatory(task.mandatory_days > 0);
            setMandatoryDays(task.mandatory_days > 0 ? task.mandatory_days : ALL_DAYS);
            setStartTime(task.mandatory_start_time || "00:00");
            if (task.assigned_children) {
              setAssignAll(false);
              setSelectedChildren(new Set(task.assigned_children));
            } else {
              setAssignAll(true);
              setSelectedChildren(new Set());
            }
          }
        }
      } catch (e: any) {
        if (!cancelled) setError(e.message || "Failed to load");
      } finally {
        if (!cancelled) setInitialLoading(false);
      }
    }
    init();
    return () => {
      cancelled = true;
    };
  }, [isEdit, taskId]);

  const validate = useCallback((): boolean => {
    let valid = true;

    const trimmedName = name.trim();
    if (!trimmedName) {
      setNameError("Name is required");
      valid = false;
    } else if (trimmedName.length > 100) {
      setNameError("Name must be 100 characters or less");
      valid = false;
    } else {
      setNameError(null);
    }

    const mins = parseInt(minutes, 10);
    if (!minutes || !Number.isFinite(mins) || mins === 0) {
      setMinutesError("Minutes must be non-zero");
      valid = false;
    } else {
      setMinutesError(null);
    }

    if (!assignAll && selectedChildren.size === 0) {
      setChildrenError("Select at least one child");
      valid = false;
    } else {
      setChildrenError(null);
    }

    return valid;
  }, [name, minutes, assignAll, selectedChildren]);

  async function handleSave(e: React.FormEvent) {
    e.preventDefault();
    if (!validate()) return;

    setLoading(true);
    setError(null);

    const body: CreateTaskReq | UpdateTaskReq = {
      name: name.trim(),
      minutes: parseInt(minutes, 10),
      priority,
      mandatory_days: mandatory ? mandatoryDays : 0,
      mandatory_start_time: mandatory ? startTime : null,
      assigned_children: assignAll ? null : Array.from(selectedChildren),
    };

    try {
      if (isEdit && taskId) {
        await updateTask(taskId, body);
        setToast("Task updated.");
      } else {
        await createTask(body);
        setToast("Task created.");
      }
      // Navigate back to task list after short delay for toast visibility
      setTimeout(() => {
        window.location.hash = "tasks";
      }, 600);
    } catch (e: any) {
      setError(e.message || "Could not save task. Please try again.");
    } finally {
      setLoading(false);
    }
  }

  async function handleDelete() {
    if (!taskId) return;
    setLoading(true);
    setError(null);
    try {
      await deleteTask(taskId);
      setToast("Task deleted.");
      setShowDeleteDialog(false);
      setTimeout(() => {
        window.location.hash = "tasks";
      }, 600);
    } catch (e: any) {
      setError(e.message || "Could not delete task. Please try again.");
      setShowDeleteDialog(false);
    } finally {
      setLoading(false);
    }
  }

  function toggleDay(bit: number) {
    setMandatoryDays((prev) => prev ^ bit);
  }

  function toggleChild(childId: string) {
    setSelectedChildren((prev) => {
      const next = new Set(prev);
      if (next.has(childId)) {
        next.delete(childId);
      } else {
        next.add(childId);
      }
      return next;
    });
    setChildrenError(null);
  }

  if (initialLoading) {
    return (
      <section className="col" style={{ gap: 12 }}>
        <header
          className="row"
          style={{ justifyContent: "space-between", alignItems: "center" }}
        >
          <h2 className="title" style={{ margin: 0 }}>
            {isEdit ? "Edit Task" : "New Task"}
          </h2>
        </header>
        <p className="subtitle" aria-busy="true">
          Loading...
        </p>
      </section>
    );
  }

  return (
    <section className="col" style={{ gap: 12 }}>
      <header
        className="row"
        style={{ justifyContent: "space-between", alignItems: "center" }}
      >
        <h2 className="title" style={{ margin: 0 }}>
          {isEdit ? "Edit Task" : "New Task"}
        </h2>
        <div className="row" style={{ gap: 8 }}>
          <a href="#tasks" style={{ textDecoration: "none" }}>
            <button type="button" className="secondary outline">
              Cancel
            </button>
          </a>
        </div>
      </header>

      {error && <p className="error">{error}</p>}
      {toast && (
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
          {toast}
        </div>
      )}

      <form onSubmit={handleSave} className="col" style={{ gap: 16 }}>
        {/* Name */}
        <label className="col" style={{ gap: 4 }}>
          <span>
            Name <span style={{ color: "#dc2626" }}>*</span>
          </span>
          <input
            type="text"
            value={name}
            onChange={(e) => {
              setName(e.target.value);
              setNameError(null);
            }}
            maxLength={100}
            placeholder="e.g. Brush teeth"
            aria-invalid={nameError ? true : undefined}
            aria-describedby={nameError ? "name-error" : undefined}
            required
          />
          {nameError && (
            <small id="name-error" className="error" aria-live="polite">
              {nameError}
            </small>
          )}
        </label>

        {/* Minutes */}
        <label className="col" style={{ gap: 4 }}>
          <span>
            Minutes <span style={{ color: "#dc2626" }}>*</span>
          </span>
          <div className="row" style={{ gap: 8, alignItems: "center" }}>
            <input
              type="number"
              value={minutes}
              onChange={(e) => {
                setMinutes(e.target.value);
                setMinutesError(null);
              }}
              placeholder="15 or -5"
              inputMode="numeric"
              style={{ width: "14ch", textAlign: "right" }}
              aria-invalid={minutesError ? true : undefined}
              aria-describedby={minutesError ? "minutes-error" : undefined}
              required
            />
            <span className="subtitle">min</span>
          </div>
          {minutesError && (
            <small id="minutes-error" className="error" aria-live="polite">
              {minutesError}
            </small>
          )}
        </label>

        {/* Priority */}
        <fieldset style={{ border: "none", padding: 0, margin: 0 }}>
          <legend style={{ marginBottom: 4, fontWeight: 500 }}>Priority</legend>
          <div
            role="radiogroup"
            aria-label="Priority"
            className="row"
            style={{ gap: 0 }}
          >
            {([1, 2, 3] as const).map((p) => {
              const label = p === 1 ? "High" : p === 2 ? "Medium" : "Low";
              const isSelected = priority === p;
              return (
                <button
                  key={p}
                  type="button"
                  role="radio"
                  aria-checked={isSelected}
                  className={isSelected ? "contrast" : "secondary outline"}
                  onClick={() => setPriority(p)}
                  style={{
                    borderRadius:
                      p === 1
                        ? "8px 0 0 8px"
                        : p === 3
                          ? "0 8px 8px 0"
                          : "0",
                    flex: 1,
                  }}
                >
                  {label}
                </button>
              );
            })}
          </div>
        </fieldset>

        {/* Mandatory toggle */}
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
            role="switch"
            checked={mandatory}
            onChange={(e) => setMandatory(e.target.checked)}
            style={{ margin: 0, width: "auto" }}
          />
          <span style={{ fontWeight: 500 }}>Mandatory</span>
        </label>

        {/* Mandatory schedule section */}
        {mandatory && (
          <div
            style={{
              padding: "12px 16px",
              borderRadius: 8,
              background: "#f9fafb",
              border: "1px solid #e5e7eb",
            }}
          >
            <fieldset style={{ border: "none", padding: 0, margin: 0 }}>
              <legend style={{ marginBottom: 8, fontWeight: 500 }}>
                Schedule
              </legend>

              {/* Shortcut buttons */}
              <div className="row" style={{ gap: 8, marginBottom: 8 }}>
                <button
                  type="button"
                  className={
                    mandatoryDays === WEEKDAYS
                      ? "contrast"
                      : "secondary outline"
                  }
                  onClick={() => setMandatoryDays(WEEKDAYS)}
                  style={{ fontSize: 13, padding: "4px 12px" }}
                >
                  Weekdays
                </button>
                <button
                  type="button"
                  className={
                    mandatoryDays === ALL_DAYS
                      ? "contrast"
                      : "secondary outline"
                  }
                  onClick={() => setMandatoryDays(ALL_DAYS)}
                  style={{ fontSize: 13, padding: "4px 12px" }}
                >
                  Every day
                </button>
              </div>

              {/* Day chips */}
              <div
                role="group"
                aria-label="Mandatory days"
                className="row"
                style={{ gap: 4, flexWrap: "wrap" }}
              >
                {DAY_BITS.map((bit, i) => {
                  const isActive = !!(mandatoryDays & bit);
                  return (
                    <button
                      key={i}
                      type="button"
                      aria-pressed={isActive}
                      className={isActive ? "contrast" : "secondary outline"}
                      onClick={() => toggleDay(bit)}
                      style={{
                        width: 44,
                        height: 44,
                        padding: 0,
                        fontSize: 14,
                        fontWeight: 600,
                        borderRadius: "50%",
                        display: "inline-flex",
                        alignItems: "center",
                        justifyContent: "center",
                      }}
                      aria-label={`${DAY_SHORT_LABELS[i]}${isActive ? " (selected)" : ""}`}
                    >
                      {DAY_SHORT_LABELS[i]}
                    </button>
                  );
                })}
              </div>

              {/* Start time */}
              <label className="col" style={{ gap: 4, marginTop: 12 }}>
                <span>Start time</span>
                <div
                  className="row"
                  style={{ gap: 8, alignItems: "center" }}
                >
                  <input
                    type="time"
                    value={startTime}
                    onChange={(e) => setStartTime(e.target.value)}
                    style={{ width: "14ch" }}
                  />
                  <span
                    className="subtitle"
                    style={{ fontSize: 12 }}
                  >
                    (in family timezone)
                  </span>
                </div>
              </label>
            </fieldset>
          </div>
        )}

        {/* Assignment */}
        <fieldset style={{ border: "none", padding: 0, margin: 0 }}>
          <legend style={{ marginBottom: 4, fontWeight: 500 }}>
            Assigned to
          </legend>
          <div className="col" style={{ gap: 8 }}>
            <label
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                cursor: "pointer",
              }}
            >
              <input
                type="radio"
                name="assignment"
                checked={assignAll}
                onChange={() => {
                  setAssignAll(true);
                  setChildrenError(null);
                }}
                style={{ margin: 0, width: "auto" }}
              />
              <span>All children</span>
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
                type="radio"
                name="assignment"
                checked={!assignAll}
                onChange={() => setAssignAll(false)}
                style={{ margin: 0, width: "auto" }}
              />
              <span>Specific children</span>
            </label>
          </div>

          {/* Child checkboxes */}
          {!assignAll && (
            <div
              className="col"
              style={{
                gap: 4,
                marginTop: 8,
                paddingLeft: 28,
              }}
            >
              {children.map((child) => (
                <label
                  key={child.id}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 8,
                    cursor: "pointer",
                  }}
                >
                  <input
                    type="checkbox"
                    checked={selectedChildren.has(child.id)}
                    onChange={() => toggleChild(child.id)}
                    style={{ margin: 0, width: "auto" }}
                  />
                  <span>{child.display_name}</span>
                </label>
              ))}
              {children.length === 0 && (
                <p className="subtitle">No children configured.</p>
              )}
              {childrenError && (
                <small className="error" aria-live="polite">
                  {childrenError}
                </small>
              )}
            </div>
          )}
        </fieldset>

        {/* Save button */}
        <button
          type="submit"
          disabled={loading}
          className="acceptButton"
          aria-busy={loading}
        >
          {loading ? "Saving..." : "Save"}
        </button>
      </form>

      {/* Delete button (edit mode only) */}
      {isEdit && (
        <>
          <hr style={{ margin: "8px 0" }} />
          <button
            type="button"
            className="secondary outline"
            style={{
              color: "#dc2626",
              borderColor: "#dc2626",
              width: "100%",
            }}
            onClick={() => setShowDeleteDialog(true)}
            disabled={loading}
          >
            Delete Task
          </button>
        </>
      )}

      {/* Delete confirmation dialog — uses showModal() for focus trapping & backdrop */}
      <dialog
        ref={deleteDialogRef}
        onClose={() => setShowDeleteDialog(false)}
      >
        <article className="col" style={{ gap: 12 }}>
          <header>
            <strong>Delete Task</strong>
          </header>
          <p className="subtitle">
            Delete &quot;{name}&quot;? Completion history will be kept.
          </p>
          <footer
            className="row"
            style={{ gap: 8, justifyContent: "flex-end" }}
          >
            <button
              type="button"
              onClick={handleDelete}
              disabled={loading}
              style={{
                background: "#dc2626",
                borderColor: "#dc2626",
                color: "#fff",
              }}
            >
              {loading ? "Deleting..." : "Delete"}
            </button>
            <button
              type="button"
              className="secondary"
              onClick={() => setShowDeleteDialog(false)}
              disabled={loading}
            >
              Cancel
            </button>
          </footer>
        </article>
      </dialog>
    </section>
  );
}
