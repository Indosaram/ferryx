-- Owner reporting (docs/design/hub-owner-reporting.md). Additive: new tables and two new
-- columns; every earlier table, row, index and trigger is unchanged.

-- Durable owner objections and requests about a kind of report. The default is to send; a
-- rule only changes what the owner asked about. A newer rule closes an older overlapping one
-- (superseded_by); rows are never deleted.
CREATE TABLE IF NOT EXISTS owner_report_rules (
    id TEXT PRIMARY KEY NOT NULL,
    source TEXT NOT NULL,
    sender_key TEXT,
    topic_key TEXT,
    pattern TEXT,
    effect TEXT NOT NULL CHECK(effect IN ('mute','digest','send')),
    owner_words TEXT NOT NULL,
    origin_alert_ref TEXT,
    mhc_receipt TEXT,
    created_at TEXT NOT NULL,
    superseded_by TEXT REFERENCES owner_report_rules(id),
    closed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_owner_report_rules_active
    ON owner_report_rules(source, closed_at);

-- One row per owner-facing report decision: what the outbound gate did and which rule decided.
CREATE TABLE IF NOT EXISTS report_suppressions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    report_id TEXT NOT NULL,
    source TEXT NOT NULL,
    topic_key TEXT NOT NULL,
    salience TEXT NOT NULL CHECK(salience IN ('action','decision','result','fyi','internal')),
    decision TEXT NOT NULL CHECK(decision IN
        ('sent','digest','muted','duplicate','internal','empty','stale')),
    rule_ref TEXT,
    state_hash TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_report_suppressions_topic
    ON report_suppressions(topic_key, created_at);

-- Last delivered state of each topic: a topic is said again only when its state changes.
CREATE TABLE IF NOT EXISTS report_topics (
    topic_key TEXT PRIMARY KEY NOT NULL,
    last_state_hash TEXT NOT NULL,
    last_sent_at TEXT NOT NULL,
    salience TEXT NOT NULL
);

-- Promises the bot made in an owner conversation.
CREATE TABLE IF NOT EXISTS commitments (
    id TEXT PRIMARY KEY NOT NULL,
    thread_ref TEXT NOT NULL,
    message_ref TEXT NOT NULL,
    what TEXT NOT NULL,
    due_at TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('open','fulfilled','followed_up','abandoned')),
    fulfilled_by TEXT,
    followed_up_at TEXT,
    lane_key TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(message_ref)
);
CREATE INDEX IF NOT EXISTS idx_commitments_open_due ON commitments(state, due_at);

-- A failing job or login, reported once and again only when its state changes.
CREATE TABLE IF NOT EXISTS outages (
    id TEXT PRIMARY KEY NOT NULL,
    subject TEXT NOT NULL,
    cause TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('active','recovered')),
    first_seen TEXT NOT NULL,
    last_seen TEXT NOT NULL,
    reported_at TEXT,
    last_reported_state TEXT,
    UNIQUE(subject, cause)
);

-- The owner-directive block every agent and cron prompt receives, by content hash.
CREATE TABLE IF NOT EXISTS owner_directive_projection (
    hash TEXT PRIMARY KEY NOT NULL,
    built_at TEXT NOT NULL,
    body TEXT NOT NULL,
    superseded_json TEXT NOT NULL DEFAULT '[]'
);

-- Supervisor-derived lessons (docs/research/harness2-pstack-adoption-2026-10-30.md, A1/A2),
-- separate from owner memory.
CREATE TABLE IF NOT EXISTS supervisor_lessons (
    id TEXT PRIMARY KEY NOT NULL,
    domain TEXT NOT NULL CHECK(domain IN ('goal','report')),
    project TEXT NOT NULL,
    rule TEXT NOT NULL CHECK(length(rule) <= 200),
    rationale TEXT NOT NULL,
    keywords TEXT NOT NULL DEFAULT '',
    source_goal TEXT,
    source_revision TEXT,
    evidence_ref TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('candidate','effective','ineffective','retired')),
    pinned INTEGER NOT NULL DEFAULT 0,
    injected INTEGER NOT NULL DEFAULT 0,
    helped INTEGER NOT NULL DEFAULT 0,
    hurt INTEGER NOT NULL DEFAULT 0,
    last_injected_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_supervisor_lessons_project
    ON supervisor_lessons(domain, project, state);

ALTER TABLE delivery_obligations ADD COLUMN report_id TEXT;
ALTER TABLE judge_calls ADD COLUMN instructions_hash TEXT;
