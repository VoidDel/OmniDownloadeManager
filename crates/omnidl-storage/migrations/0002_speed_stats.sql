-- Time spent downloading, pauses excluded (milliseconds): a finished download's average speed.
ALTER TABLE tasks ADD COLUMN active_ms INTEGER NOT NULL DEFAULT 0;
-- The highest download speed seen (bytes per second).
ALTER TABLE tasks ADD COLUMN peak_bps INTEGER NOT NULL DEFAULT 0;
