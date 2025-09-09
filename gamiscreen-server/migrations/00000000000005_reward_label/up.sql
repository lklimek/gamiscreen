-- Optional text label for rewards
-- Set default to 'Additional time' so custom rewards without label still have a value
ALTER TABLE rewards ADD COLUMN label TEXT NULL DEFAULT 'Additional time';

-- Backfill historical rewards: if task-based, copy the task name; else use default
UPDATE rewards
SET label = (
  SELECT name FROM tasks WHERE tasks.id = rewards.task_id
)
WHERE label IS NULL AND task_id IS NOT NULL;

UPDATE rewards
SET label = 'Additional time'
WHERE label IS NULL AND task_id IS NULL;
