-- Per-child accounted usage minutes (dedup across devices)
CREATE TABLE IF NOT EXISTS usage_minutes (
  child_id TEXT NOT NULL,
  minute_ts BIGINT NOT NULL,
  device_id TEXT NOT NULL,
  PRIMARY KEY (child_id, minute_ts, device_id),
  FOREIGN KEY(child_id) REFERENCES children(id)
);
-- Helpful for distinct counting per child
CREATE INDEX IF NOT EXISTS idx_usage_minutes_child_minute ON usage_minutes(child_id, minute_ts);
