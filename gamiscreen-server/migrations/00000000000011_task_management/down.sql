DROP INDEX IF EXISTS idx_task_assignments_child_id;
DROP INDEX IF EXISTS idx_task_assignments_task_id;
DROP TABLE IF EXISTS task_assignments;

-- Keep task IDs and dependent history intact, including soft-deleted tasks.
ALTER TABLE tasks DROP COLUMN deleted_at;
ALTER TABLE tasks DROP COLUMN updated_at;
ALTER TABLE tasks DROP COLUMN created_at;
ALTER TABLE tasks DROP COLUMN mandatory_start_time;
ALTER TABLE tasks DROP COLUMN mandatory_days;
ALTER TABLE tasks DROP COLUMN priority;
