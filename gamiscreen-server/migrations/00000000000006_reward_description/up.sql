-- Rename label to description; description stores standalone text even for task-based rewards
ALTER TABLE rewards RENAME COLUMN label TO description;

