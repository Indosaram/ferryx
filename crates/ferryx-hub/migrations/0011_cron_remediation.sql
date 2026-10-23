-- omo-gateway's remediation outcomes: one line per verified fix or owner escalation,
-- listed in the digest. Tokens are redacted before a row is stored.
CREATE TABLE IF NOT EXISTS cron_remediation_outcomes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    problem_id INTEGER NOT NULL,
    job_id TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT,
    UNIQUE(problem_id, message)
);
