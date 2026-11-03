-- What the owner's memory directives decided about one report state under one directive
-- projection, checked at the outbound boundary (report_gate::prepare_delivery). `verdict` is the
-- judge's answer, 'unchecked' when it could not answer; `effect` is what the gate applied. An
-- 'unchecked' row is asked again on the next check instead of being reused.
CREATE TABLE IF NOT EXISTS report_directive_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    report_id TEXT NOT NULL,
    topic_key TEXT NOT NULL,
    state_hash TEXT NOT NULL,
    projection_hash TEXT NOT NULL,
    verdict TEXT NOT NULL CHECK(verdict IN ('none','send','digest','mute','unchecked')),
    effect TEXT NOT NULL CHECK(effect IN ('none','send','digest','mute')),
    directive_ref TEXT,
    note TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(report_id, state_hash, projection_hash)
);
CREATE INDEX IF NOT EXISTS idx_report_directive_decisions_topic
    ON report_directive_decisions(topic_key, projection_hash);
