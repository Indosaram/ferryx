-- Lane evidence reports, step records and quiet-lane oversight (todo 7).
-- One oversight row per lane, reset when the lane starts a new run. Progress moves only on a new
-- evidence item (a file hash or a hub-verified remote receipt not yet recorded for the run).
CREATE TABLE IF NOT EXISTS lane_oversight (
    lane_key TEXT PRIMARY KEY,
    run INTEGER NOT NULL,
    run_started_at_ms INTEGER NOT NULL,
    last_progress_at_ms INTEGER NOT NULL,
    backend TEXT NOT NULL DEFAULT 'unknown',
    reminders_sent INTEGER NOT NULL DEFAULT 0 CHECK(reminders_sent BETWEEN 0 AND 2),
    stall_alerted INTEGER NOT NULL DEFAULT 0,
    ceiling_reported INTEGER NOT NULL DEFAULT 0,
    waiting_on_owner INTEGER NOT NULL DEFAULT 0,
    ended INTEGER NOT NULL DEFAULT 0,
    deadline_at_ms INTEGER,
    wall_ceiling_min INTEGER DEFAULT 240 CHECK(wall_ceiling_min IS NULL OR wall_ceiling_min > 0),
    token_ceiling INTEGER CHECK(token_ceiling IS NULL OR token_ceiling > 0),
    cost_ceiling_micros INTEGER CHECK(cost_ceiling_micros IS NULL OR cost_ceiling_micros > 0)
);

CREATE TABLE IF NOT EXISTS lane_evidence (
    lane_key TEXT NOT NULL,
    run INTEGER NOT NULL,
    -- sha256:<hex> of a file's bytes, or commit:<sha> / pr:<url> of a verified receipt.
    identity TEXT NOT NULL,
    label TEXT NOT NULL,
    recorded_at_ms INTEGER NOT NULL,
    PRIMARY KEY (lane_key, run, identity)
);

CREATE TABLE IF NOT EXISTS lane_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    lane_key TEXT NOT NULL,
    run INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('progress','stuck','waiting','stopped','finished')),
    text TEXT NOT NULL,
    new_items INTEGER NOT NULL,
    reporter TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lane_reports_lane ON lane_reports(lane_key, run, id);

-- Worker usage per lane run. A NULL total is an unknown total, never a zero.
CREATE TABLE IF NOT EXISTS lane_usage (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    lane_key TEXT NOT NULL,
    run INTEGER NOT NULL,
    tokens INTEGER CHECK(tokens IS NULL OR tokens >= 0),
    cost_micros INTEGER CHECK(cost_micros IS NULL OR cost_micros >= 0),
    recorded_at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lane_usage_lane ON lane_usage(lane_key, run);
