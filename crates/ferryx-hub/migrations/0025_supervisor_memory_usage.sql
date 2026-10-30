-- Owner memory provenance and attributable supervisor model calls (todo 15). Additive: two new
-- tables; every table, index and trigger of 0020-0022 is unchanged.
--
-- supervisor_memory_sources keeps the original owner message behind every native memory
-- request and the opaque mhc receipt it produced. mhc owns learning, recall, forgetting and
-- suppression; this table never holds a synthesized rule, an embedding or a suppression. One
-- row per owner source message: a replay of the same source with the same body is the same
-- request (the engine call is idempotent by the same source id); a different body is refused.
CREATE TABLE IF NOT EXISTS supervisor_memory_sources (
    source_id TEXT PRIMARY KEY CHECK(length(source_id) BETWEEN 1 AND 512),
    supervisor_id TEXT NOT NULL REFERENCES supervisor_identity(supervisor_id),
    owner_principal TEXT NOT NULL,
    source_adapter TEXT NOT NULL CHECK(source_adapter IN ('slack','discord','desktop','phone','cli','mcp')),
    source_message_id TEXT NOT NULL,
    -- teach: an explicit receipt-backed teaching; correct: replaces target_receipt_id;
    -- forget: removes target_receipt_id; ingest: owner text sent to the owner-ingest session.
    action TEXT NOT NULL CHECK(action IN ('teach','correct','forget','ingest')),
    project_id TEXT REFERENCES supervisor_projects(project_id),
    -- The owner's text exactly as authored (empty for a forget).
    body TEXT NOT NULL,
    body_sha256 TEXT NOT NULL CHECK(length(body_sha256) = 64),
    target_receipt_id TEXT,
    -- The mhc receipt this request produced or closed, once the engine answered.
    receipt_id TEXT,
    -- pending (not answered yet), the mhc receipt state (accepted, persisted, applied,
    -- forgotten, corrected), ingested, or failed.
    state TEXT NOT NULL,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK((action IN ('correct','forget')) = (target_receipt_id IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS idx_supervisor_memory_sources_receipt
    ON supervisor_memory_sources(receipt_id) WHERE receipt_id IS NOT NULL;

-- One row per supervisor engine turn (decision, convergence, conversation, owner ingest),
-- whatever its outcome. Usage the provider did not report is NULL (unknown), never 0: the
-- engine reports an unreported field as zero, so a zero is recorded as unknown.
CREATE TABLE IF NOT EXISTS supervisor_model_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    -- Why the call happened: a wake (with its causes), a purpose (gap_assess, ...), a
    -- conversation, an owner ingest, an operator probe; 'unattributed' when no caller named one.
    cause TEXT NOT NULL CHECK(length(cause) BETWEEN 1 AND 256),
    purpose TEXT,
    role TEXT NOT NULL,
    goal_id TEXT,
    goal_revision INTEGER,
    wake_turn_id INTEGER,
    -- JSON array of the event ids merged into this call (a wake batch), or the owner source id.
    merged_event_ids TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(merged_event_ids) AND json_type(merged_event_ids) = 'array'),
    -- sha256 of the stable instruction prefix the prompt opened with, and the sizes of the
    -- prefix and of the delta payload after it. NULL when the caller sent no split prompt.
    prefix_sha256 TEXT CHECK(prefix_sha256 IS NULL OR length(prefix_sha256) = 64),
    prefix_bytes INTEGER,
    delta_bytes INTEGER,
    prompt_bytes INTEGER NOT NULL,
    -- What the engine reported answering with; NULL when it reported nothing.
    provider TEXT,
    model TEXT,
    input_tokens INTEGER CHECK(input_tokens IS NULL OR input_tokens > 0),
    output_tokens INTEGER CHECK(output_tokens IS NULL OR output_tokens > 0),
    cache_read_tokens INTEGER CHECK(cache_read_tokens IS NULL OR cache_read_tokens > 0),
    cache_write_tokens INTEGER CHECK(cache_write_tokens IS NULL OR cache_write_tokens > 0),
    -- 'ok' or the typed fault code.
    outcome TEXT NOT NULL,
    ms INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_supervisor_model_calls_created ON supervisor_model_calls(created_at);
CREATE INDEX IF NOT EXISTS idx_supervisor_model_calls_goal ON supervisor_model_calls(goal_id) WHERE goal_id IS NOT NULL;
