DROP INDEX IF EXISTS idx_task_assignments_child_id;
DROP INDEX IF EXISTS idx_task_assignments_task_id;
DROP TABLE IF EXISTS task_assignments;

-- SQLite cannot DROP COLUMN in older versions; recreate the table
CREATE TABLE tasks_backup (
    id TEXT NOT NULL PRIMARY KEY,
    name TEXT NOT NULL,
    minutes INTEGER NOT NULL,
    required BOOLEAN NOT NULL DEFAULT 0
);

INSERT INTO tasks_backup (id, name, minutes, required)
SELECT id, name, minutes, required FROM tasks WHERE deleted_at IS NULL;

DROP TABLE tasks;

ALTER TABLE tasks_backup RENAME TO tasks;
