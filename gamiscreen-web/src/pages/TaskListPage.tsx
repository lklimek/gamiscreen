import { useEffect, useState } from "react";
import { listTasksManagement, type TaskManagementDto, listChildren, type ChildDto } from "../api";
import {
  formatMandatoryDays,
  priorityColor,
  priorityLabel,
} from "../taskHelpers";

export function TaskListPage() {
  const [tasks, setTasks] = useState<TaskManagementDto[]>([]);
  const [children, setChildren] = useState<ChildDto[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const [taskList, childList] = await Promise.all([
        listTasksManagement(),
        listChildren(),
      ]);
      setTasks(taskList);
      setChildren(childList);
    } catch (e: any) {
      setError(e.message || "Failed to load tasks");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load();
  }, []);

  function formatAssignment(task: TaskManagementDto): string {
    if (!task.assigned_children) return "All";
    const names = task.assigned_children.map((cid) => {
      const child = children.find((c) => c.id === cid);
      return child ? child.display_name : cid;
    });
    return names.join(", ");
  }

  function formatSchedule(task: TaskManagementDto): string {
    if (task.mandatory_days === 0) return "Optional";
    const days = formatMandatoryDays(task.mandatory_days);
    const time = task.mandatory_start_time || "00:00";
    return `${days}, ${time}`;
  }

  if (loading) {
    return (
      <section className="col" style={{ gap: 12 }}>
        <header
          className="row"
          style={{ justifyContent: "space-between", alignItems: "center" }}
        >
          <h2 className="title" style={{ margin: 0 }}>
            Tasks
          </h2>
        </header>
        <p className="subtitle" aria-busy="true">
          Loading tasks...
        </p>
      </section>
    );
  }

  if (error) {
    return (
      <section className="col" style={{ gap: 12 }}>
        <header
          className="row"
          style={{ justifyContent: "space-between", alignItems: "center" }}
        >
          <h2 className="title" style={{ margin: 0 }}>
            Tasks
          </h2>
        </header>
        <p className="error">{error}</p>
        <button onClick={load}>Retry</button>
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
          Tasks
        </h2>
        <a href="#tasks/new">
          <button type="button">+ Add Task</button>
        </a>
      </header>

      {tasks.length === 0 && (
        <div
          className="card"
          style={{ padding: 24, textAlign: "center" }}
        >
          <p className="subtitle" style={{ marginBottom: 12 }}>
            No tasks yet. Create your first task.
          </p>
          <a href="#tasks/new">
            <button type="button">+ Add Task</button>
          </a>
        </div>
      )}

      <ul style={{ listStyle: "none", padding: 0, margin: 0 }}>
        {tasks.map((task) => (
          <li key={task.id} style={{ marginBottom: 8 }}>
            <a
              href={`#tasks/${encodeURIComponent(task.id)}`}
              style={{ textDecoration: "none", color: "inherit", display: "block" }}
              aria-label={`${priorityLabel(task.priority)} priority task: ${task.name}, ${task.minutes > 0 ? "+" : ""}${task.minutes} minutes`}
            >
              <article style={{ padding: "12px 16px", cursor: "pointer" }}>
                <div
                  className="row"
                  style={{
                    justifyContent: "space-between",
                    alignItems: "flex-start",
                    gap: 8,
                  }}
                >
                  <div
                    className="row"
                    style={{ alignItems: "center", gap: 8, minWidth: 0, flex: 1 }}
                  >
                    <span
                      aria-hidden="true"
                      style={{
                        width: 10,
                        height: 10,
                        borderRadius: "50%",
                        background: priorityColor(task.priority),
                        flexShrink: 0,
                      }}
                    />
                    <span className="sr-only">
                      {priorityLabel(task.priority)} priority
                    </span>
                    <span
                      style={{
                        fontWeight: 600,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {task.name}
                    </span>
                  </div>
                  <span
                    className="taskAssignmentPill"
                    style={{
                      fontSize: 12,
                      padding: "2px 8px",
                      borderRadius: 12,
                      background: "#f3f4f6",
                      color: "#374151",
                      whiteSpace: "nowrap",
                      flexShrink: 0,
                    }}
                  >
                    {formatAssignment(task)}
                  </span>
                </div>
                <div
                  className="row"
                  style={{
                    gap: 8,
                    marginTop: 4,
                    fontSize: 14,
                    color: "var(--muted-color, #6b7280)",
                    flexWrap: "wrap",
                  }}
                >
                  <span
                    style={{
                      color: task.minutes < 0 ? "#b91c1c" : undefined,
                    }}
                  >
                    {task.minutes > 0 ? "+" : ""}
                    {task.minutes} min
                  </span>
                  <span aria-hidden="true">|</span>
                  <span>{formatSchedule(task)}</span>
                </div>
              </article>
            </a>
          </li>
        ))}
      </ul>
    </section>
  );
}
