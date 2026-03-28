-- New table for balance transaction audit trail
CREATE TABLE balance_transactions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    child_id TEXT NOT NULL REFERENCES children(id) ON DELETE CASCADE,
    amount INTEGER NOT NULL,
    description TEXT,
    related_reward_id INTEGER REFERENCES rewards(id),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Add account_balance column to balances table
ALTER TABLE balances ADD COLUMN account_balance INTEGER NOT NULL DEFAULT 0;

-- Migrate existing data: compute initial account_balance from historical rewards/usage.
-- account_balance = earned - borrowed - used (clamped to <= 0, since positive balance
-- is already reflected in minutes_remaining and we only need to track debt).
UPDATE balances SET account_balance = MIN(0,
    (SELECT COALESCE(SUM(CASE WHEN is_borrowed = 0 THEN minutes ELSE 0 END), 0)
          - COALESCE(SUM(CASE WHEN is_borrowed = 1 THEN minutes ELSE 0 END), 0)
     FROM rewards WHERE rewards.child_id = balances.child_id)
    - (SELECT COUNT(DISTINCT minute_ts) FROM usage_minutes WHERE usage_minutes.child_id = balances.child_id)
);
