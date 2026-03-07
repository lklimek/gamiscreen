ALTER TABLE rewards DROP COLUMN is_borrowed;
ALTER TABLE tasks DROP COLUMN required;
UPDATE balances SET minutes_remaining = 0;
