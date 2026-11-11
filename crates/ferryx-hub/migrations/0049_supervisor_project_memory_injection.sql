-- Per-project switch for giving owner-taught project decisions to hooked agent sessions
-- (agent-attention-parity todo 8). Keyed by the project's repository identity, never a host
-- path. A repository without a row is off: injection happens only after the owner turned it on.
CREATE TABLE IF NOT EXISTS supervisor_project_memory_injection (
    repo_identity TEXT PRIMARY KEY CHECK(length(repo_identity) BETWEEN 1 AND 1024),
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
    updated_at TEXT NOT NULL
);
