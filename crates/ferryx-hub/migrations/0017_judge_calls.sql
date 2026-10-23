-- omo-gateway 0032: every judge call (jev, fallback, capped) for the usage report and daily caps.
CREATE TABLE IF NOT EXISTS judge_calls (
    id TEXT PRIMARY KEY NOT NULL,
    purpose TEXT NOT NULL,
    kind TEXT NOT NULL,
    ms INTEGER NOT NULL,
    outcome TEXT NOT NULL,
    used_fallback INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_judge_calls_created_at ON judge_calls(created_at);
CREATE INDEX IF NOT EXISTS idx_judge_calls_purpose_fallback ON judge_calls(purpose, used_fallback);
