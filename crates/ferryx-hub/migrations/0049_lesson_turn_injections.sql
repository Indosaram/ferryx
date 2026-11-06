CREATE TABLE IF NOT EXISTS lesson_turn_injections (
    turn_ref TEXT NOT NULL,
    lesson_id TEXT NOT NULL REFERENCES supervisor_lessons(id),
    injected_at TEXT NOT NULL,
    outcome TEXT CHECK(outcome IS NULL OR outcome IN ('helped','hurt','neutral')),
    settled_at TEXT,
    PRIMARY KEY(turn_ref, lesson_id)
);
CREATE INDEX IF NOT EXISTS idx_lesson_turn_injections_open ON lesson_turn_injections(turn_ref, settled_at);
