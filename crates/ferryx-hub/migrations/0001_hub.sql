-- Hub store. Ported from the omo-gateway tables the in-process hub still owns.
-- SSH, inbox, autonomy, voice, and the dashboard tables are not created.

CREATE TABLE IF NOT EXISTS hub_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS session_watches (
    target TEXT PRIMARY KEY,
    added_at TEXT NOT NULL,
    reason TEXT NOT NULL DEFAULT 'manual'
);

CREATE TABLE IF NOT EXISTS session_watch_notices (
    dedupe_key TEXT PRIMARY KEY,
    target TEXT NOT NULL,
    kind TEXT NOT NULL,
    sent_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS lanes (
    key TEXT PRIMARY KEY CHECK(length(key) BETWEEN 1 AND 48 AND key NOT GLOB '*[^a-z0-9-]*'),
    title TEXT NOT NULL,
    lead_session_key TEXT NOT NULL,
    cwd TEXT NOT NULL,
    brief_path TEXT NOT NULL DEFAULT '',
    session_key TEXT NOT NULL,
    omo_thread_id TEXT,
    status TEXT NOT NULL CHECK(status IN ('queued','working','blocked','done_claimed','not_done','verified_done','closed','failed')),
    done_signals TEXT NOT NULL,
    claim_evidence TEXT,
    claim_at TEXT,
    turns_run INTEGER NOT NULL DEFAULT 0,
    last_activity_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS lane_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    lane_key TEXT NOT NULL REFERENCES lanes(key),
    at TEXT NOT NULL,
    from_status TEXT,
    to_status TEXT NOT NULL,
    reason TEXT
);

CREATE TABLE IF NOT EXISTS lane_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    lane_key TEXT,
    lead_session_key TEXT NOT NULL,
    kind TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT
);

CREATE TABLE IF NOT EXISTS external_sessions (
    lane_key TEXT PRIMARY KEY REFERENCES lanes(key),
    goal TEXT NOT NULL,
    send_key TEXT NOT NULL,
    cursor_revision TEXT,
    cursor_text TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS external_session_items (
    lane_key TEXT NOT NULL,
    item_id TEXT NOT NULL,
    PRIMARY KEY (lane_key, item_id)
);

CREATE TABLE IF NOT EXISTS cron_jobs (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    target TEXT NOT NULL CHECK(target IN ('shell','meta_pane','new_pane')),
    script TEXT NOT NULL,
    schedule TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tracker_issues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'open',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chat_outbox (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_key TEXT NOT NULL,
    platform TEXT NOT NULL,
    text TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_lanes_status ON lanes(status);
CREATE INDEX IF NOT EXISTS idx_lane_events_delivered ON lane_events(delivered_at, lead_session_key);
CREATE INDEX IF NOT EXISTS idx_chat_outbox_undelivered ON chat_outbox(delivered_at);
