-- omo-gateway 0048/0055: a lane pause is a throttle the lead sets; the runner does not
-- drive a paused lane. resumed_at closes the pause; a reopen clears it.
CREATE TABLE IF NOT EXISTS lane_pauses (
    lane_key TEXT PRIMARY KEY REFERENCES lanes(key),
    paused_at TEXT NOT NULL,
    paused_by TEXT NOT NULL,
    resumed_at TEXT
);
