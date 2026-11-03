-- Autonomous supervisor action log and standing rules (supervisor panel plan G6, G9).
-- Additive: two new tables; every earlier table, index and trigger is unchanged.
--
-- supervisor_actions holds one row per incident (incident_key UNIQUE): later events of the same
-- incident update the row in place. Timestamps are epoch milliseconds. undone_at is set once the
-- row's undo ran; undo_json is cleared then.
CREATE TABLE IF NOT EXISTS supervisor_actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    incident_key TEXT NOT NULL UNIQUE,
    session_ref TEXT,
    kind TEXT NOT NULL,
    summary TEXT NOT NULL,
    basis TEXT NOT NULL,
    evidence_json TEXT NOT NULL DEFAULT '[]',
    undo_json TEXT,
    live INTEGER NOT NULL DEFAULT 0 CHECK(live IN (0,1)),
    undone_at INTEGER,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS supervisor_actions_updated ON supervisor_actions(updated_at DESC);

-- supervisor_rules holds the owner's standing rules; the defaults are seeded on first use.
CREATE TABLE IF NOT EXISTS supervisor_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope_json TEXT NOT NULL DEFAULT '{}',
    kind TEXT NOT NULL,
    params_json TEXT NOT NULL DEFAULT '{}',
    created_from_message_id TEXT,
    created_at INTEGER NOT NULL
);
