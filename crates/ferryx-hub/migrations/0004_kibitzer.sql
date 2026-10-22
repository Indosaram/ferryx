CREATE TABLE IF NOT EXISTS kibitzer_state (
    lead TEXT PRIMARY KEY,
    generation INTEGER NOT NULL DEFAULT 0,
    consumed INTEGER NOT NULL DEFAULT 0,
    admitted_ms INTEGER
);
