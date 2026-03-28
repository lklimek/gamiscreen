-- Add new columns to tasks table
ALTER TABLE tasks ADD COLUMN priority INTEGER NOT NULL DEFAULT 2;
ALTER TABLE tasks ADD COLUMN mandatory_days INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN mandatory_start_time TEXT;
ALTER TABLE tasks ADD COLUMN created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP;
ALTER TABLE tasks ADD COLUMN updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP;
ALTER TABLE tasks ADD COLUMN deleted_at TIMESTAMP;

-- Backfill existing rows: required=true -> mandatory_days=127 (all days), start_time=00:00
UPDATE tasks SET mandatory_days = 127, mandatory_start_time = '00:00' WHERE required = 1;
UPDATE tasks SET mandatory_days = 0 WHERE required = 0;

-- Task assignments table (no rows = all children)
CREATE TABLE task_assignments (
    id INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    child_id TEXT NOT NULL REFERENCES children(id) ON DELETE CASCADE,
    UNIQUE(task_id, child_id)
);

CREATE INDEX idx_task_assignments_task_id ON task_assignments(task_id);
CREATE INDEX idx_task_assignments_child_id ON task_assignments(child_id);
