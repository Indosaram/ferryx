CREATE TABLE IF NOT EXISTS cron_monitor_states (
    job_id TEXT PRIMARY KEY NOT NULL,
    last_hash TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS action_loop_seen (
    item_key TEXT PRIMARY KEY NOT NULL,
    seen_at TEXT NOT NULL
);

-- Same statuses as omo migration 0054. `enqueued` is the lane event; the effect
-- pass moves a row to executed, verified, or failed.
CREATE TABLE IF NOT EXISTS action_loop_outcomes (
    item_key TEXT PRIMARY KEY NOT NULL,
    lane_event_id INTEGER,
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    line TEXT NOT NULL,
    link TEXT,
    status TEXT NOT NULL CHECK (status IN ('enqueued', 'executed', 'verified', 'failed')),
    external_id TEXT,
    detail TEXT,
    attempts INTEGER NOT NULL DEFAULT 1,
    enqueued_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_action_loop_outcomes_status ON action_loop_outcomes(status);

CREATE TABLE IF NOT EXISTS inbox_digests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS inbox_watcher_health (
    source TEXT PRIMARY KEY NOT NULL,
    status TEXT NOT NULL,
    detail TEXT,
    updated_at TEXT NOT NULL
);

ALTER TABLE pairing_codes ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE pairing_codes ADD COLUMN locked_until TEXT;
