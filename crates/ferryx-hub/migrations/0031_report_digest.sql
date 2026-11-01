-- Preserve the actual deferred content, not just its topic identifier. One pending entry
-- per topic is replaced by its latest state until a daily delivery claims it.
CREATE TABLE report_digest_pending (
    topic_key TEXT PRIMARY KEY NOT NULL,
    report_id TEXT NOT NULL,
    source TEXT NOT NULL,
    sender TEXT,
    body TEXT NOT NULL,
    quote TEXT,
    state_hash TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE report_digest_deliveries (
    day TEXT PRIMARY KEY NOT NULL,
    outbox_id INTEGER NOT NULL REFERENCES chat_outbox(id)
);
