-- Owner answers to worker prompts (supervisor autonomy, todo 5). Additive: one new table and
-- one index; nothing earlier changes.
--
-- One row per answer the owner gave. `shape_key` is the prompt's normalized question text
-- (numbers and paths stripped) plus its sorted option set, so the same question asked again
-- with other numbers or paths in the same repo for the same agent maps to the same key. The
-- supervisor answers on the owner's behalf only from repeated, uncontradicted rows.
CREATE TABLE IF NOT EXISTS supervisor_answer_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    shape_key TEXT NOT NULL CHECK(length(shape_key) > 0),
    repo TEXT NOT NULL,
    agent TEXT NOT NULL,
    option_label TEXT NOT NULL CHECK(length(option_label) > 0),
    at_ms INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_supervisor_answer_history_shape
    ON supervisor_answer_history(shape_key, repo, agent, id);
