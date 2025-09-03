-- Track task completions (who marked done and when)
CREATE TABLE IF NOT EXISTS task_completions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  child_id TEXT NOT NULL,
  task_id TEXT NOT NULL,
  by_username TEXT NOT NULL,
  done_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  FOREIGN KEY(child_id) REFERENCES children(id),
  FOREIGN KEY(task_id) REFERENCES tasks(id)
);

CREATE INDEX IF NOT EXISTS idx_task_completions_child_task ON task_completions(child_id, task_id);
CREATE INDEX IF NOT EXISTS idx_task_completions_done_at ON task_completions(done_at);

