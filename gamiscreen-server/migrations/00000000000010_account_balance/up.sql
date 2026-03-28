-- New table for balance transaction audit trail
CREATE TABLE balance_transactions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    child_id TEXT NOT NULL REFERENCES children(id) ON DELETE CASCADE,
    amount INTEGER NOT NULL,
    description TEXT,
    related_reward_id INTEGER REFERENCES rewards(id),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_balance_transactions_child_id ON balance_transactions(child_id);

-- Add account_balance column to balances table
ALTER TABLE balances ADD COLUMN account_balance INTEGER NOT NULL DEFAULT 0;

-- Migrate existing data: compute initial account_balance from historical rewards.
-- account_balance = earned - borrowed (clamped to <= 0, since positive balance
-- is already reflected in minutes_remaining and we only need to track debt).
-- Usage does not affect account_balance — it only reduces minutes_remaining.
UPDATE balances SET account_balance = MIN(0,
    (SELECT COALESCE(SUM(CASE WHEN is_borrowed = 0 THEN minutes ELSE 0 END), 0)
          - COALESCE(SUM(CASE WHEN is_borrowed = 1 THEN minutes ELSE 0 END), 0)
     FROM rewards WHERE rewards.child_id = balances.child_id)
);

-- Seed balance_transactions for children with historical debt
INSERT INTO balance_transactions (child_id, amount, description)
SELECT child_id, account_balance, 'Migration: initial balance from historical rewards'
FROM balances WHERE account_balance != 0;
