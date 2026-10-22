-- cron_problems gains the omo health columns. summary keeps a default so a live
-- row that stores detail instead of summary still imports.
CREATE TABLE cron_problems_next (
    id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL,
    summary TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    owner_token TEXT,
    status TEXT NOT NULL DEFAULT 'open',
    kind TEXT NOT NULL DEFAULT '',
    signature TEXT NOT NULL DEFAULT '',
    detail TEXT NOT NULL DEFAULT '',
    run_id TEXT,
    lane_key TEXT,
    updated_at TEXT
);

INSERT INTO cron_problems_next (
    id, job_id, summary, created_at, owner_token, status
)
SELECT id, job_id, summary, created_at, owner_token, status FROM cron_problems;

DROP TABLE cron_problems;
ALTER TABLE cron_problems_next RENAME TO cron_problems;

CREATE UNIQUE INDEX IF NOT EXISTS idx_cron_problems_signature
    ON cron_problems(job_id, kind, signature)
    WHERE signature != '';
