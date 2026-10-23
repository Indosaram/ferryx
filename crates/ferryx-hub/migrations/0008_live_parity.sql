-- Quiet-hour release for action-loop events, owner-alert obligations, and lane worktrees.
ALTER TABLE lane_events ADD COLUMN release_at TEXT;

CREATE TABLE IF NOT EXISTS delivery_obligations (
    id TEXT PRIMARY KEY NOT NULL,
    state TEXT NOT NULL,
    body TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS lane_owned_worktrees (
    lane_key TEXT NOT NULL,
    path TEXT NOT NULL,
    state TEXT NOT NULL,
    PRIMARY KEY (lane_key, path)
);
