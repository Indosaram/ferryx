-- Lesson measurement (docs/research/harness2-pstack-adoption-2026-10-30.md A2,
-- docs/design/hub-owner-reporting.md section 8). Additive: one new table.

-- Which lessons were injected into which piece of work (`goal:<goal_id>`, `report:<id>`), and
-- how that work went. A row is settled once; its outcome feeds the lesson's helped/hurt count.
CREATE TABLE IF NOT EXISTS lesson_injections (
    lesson_id TEXT NOT NULL REFERENCES supervisor_lessons(id),
    subject_ref TEXT NOT NULL,
    injected_at TEXT NOT NULL,
    outcome TEXT CHECK(outcome IS NULL OR outcome IN ('helped','hurt','neutral')),
    settled_at TEXT,
    PRIMARY KEY(lesson_id, subject_ref)
);
CREATE INDEX IF NOT EXISTS idx_lesson_injections_open
    ON lesson_injections(subject_ref, settled_at);
