-- SQLite does not support DROP COLUMN before 3.35.0; recreate is needed for older versions.
-- For simplicity, just zero out account_balance (column remains but is unused).
UPDATE balances SET account_balance = 0;

DROP TABLE IF EXISTS balance_transactions;
