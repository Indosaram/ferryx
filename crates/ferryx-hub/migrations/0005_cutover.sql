ALTER TABLE lanes ADD COLUMN thread_channel_id TEXT;

CREATE TABLE IF NOT EXISTS lane_thread_bindings (
    lane_key TEXT PRIMARY KEY,
    bot_id TEXT NOT NULL,
    bound_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

ALTER TABLE cron_problems ADD COLUMN owner_token TEXT;
ALTER TABLE cron_problems ADD COLUMN status TEXT NOT NULL DEFAULT 'open';

CREATE TABLE IF NOT EXISTS lane_transcript (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    lane_key TEXT NOT NULL,
    at TEXT NOT NULL,
    role TEXT NOT NULL,
    text TEXT NOT NULL
);
