-- Owner memory seeded from earlier agent sessions (supervisor setup). Owner-authored turns of
-- recent Claude/Codex/omo sessions are sent to owner ingest once, with provenance adapter
-- 'import'. supervisor_memory_sources is rebuilt only to widen its source_adapter CHECK;
-- every row is copied unchanged and its one index is recreated exactly as 0025 made it.
CREATE TABLE supervisor_memory_sources_next (
    source_id TEXT PRIMARY KEY CHECK(length(source_id) BETWEEN 1 AND 512),
    supervisor_id TEXT NOT NULL REFERENCES supervisor_identity(supervisor_id),
    owner_principal TEXT NOT NULL,
    source_adapter TEXT NOT NULL CHECK(source_adapter IN ('slack','discord','desktop','phone','cli','mcp','import')),
    source_message_id TEXT NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('teach','correct','forget','ingest')),
    project_id TEXT REFERENCES supervisor_projects(project_id),
    body TEXT NOT NULL,
    body_sha256 TEXT NOT NULL CHECK(length(body_sha256) = 64),
    target_receipt_id TEXT,
    receipt_id TEXT,
    state TEXT NOT NULL,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK((action IN ('correct','forget')) = (target_receipt_id IS NOT NULL))
);
INSERT INTO supervisor_memory_sources_next(source_id, supervisor_id, owner_principal,
    source_adapter, source_message_id, action, project_id, body, body_sha256,
    target_receipt_id, receipt_id, state, last_error, created_at, updated_at)
SELECT source_id, supervisor_id, owner_principal, source_adapter, source_message_id, action,
    project_id, body, body_sha256, target_receipt_id, receipt_id, state, last_error,
    created_at, updated_at
FROM supervisor_memory_sources ORDER BY rowid;
DROP TABLE supervisor_memory_sources;
ALTER TABLE supervisor_memory_sources_next RENAME TO supervisor_memory_sources;
CREATE INDEX IF NOT EXISTS idx_supervisor_memory_sources_receipt
    ON supervisor_memory_sources(receipt_id) WHERE receipt_id IS NOT NULL;

-- One row per imported provider session: what its owner turns hashed to when they were last
-- imported, so an unchanged session is never sent again and a grown one is.
CREATE TABLE IF NOT EXISTS supervisor_memory_imports (
    provider TEXT NOT NULL,
    provider_session_id TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    turns INTEGER NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('imported','partial','failed')),
    imported_at TEXT NOT NULL,
    PRIMARY KEY(provider, provider_session_id)
);
