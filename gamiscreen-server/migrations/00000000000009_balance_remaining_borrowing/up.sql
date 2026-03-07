-- Add is_borrowed flag to rewards table
ALTER TABLE rewards ADD COLUMN is_borrowed INTEGER NOT NULL DEFAULT 0;

-- Add required flag to tasks table
ALTER TABLE tasks ADD COLUMN required INTEGER NOT NULL DEFAULT 0;

-- Populate balances for children that already have rows (from init migration)
UPDATE balances
SET minutes_remaining = (
  SELECT COALESCE(
    (SELECT SUM(minutes) FROM rewards WHERE rewards.child_id = balances.child_id), 0
  ) - COALESCE(
    (SELECT COUNT(DISTINCT minute_ts) FROM usage_minutes WHERE usage_minutes.child_id = balances.child_id), 0
  )
);

-- Ensure every child has a balances row (some may not if created after init migration)
INSERT OR IGNORE INTO balances (child_id, minutes_remaining)
SELECT id, COALESCE(
  (SELECT SUM(minutes) FROM rewards WHERE rewards.child_id = children.id), 0
) - COALESCE(
  (SELECT COUNT(DISTINCT minute_ts) FROM usage_minutes WHERE usage_minutes.child_id = children.id), 0
)
FROM children;
