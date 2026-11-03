-- Owner answers to worker prompts (supervisor autonomy, todo 5). Additive: one new table and
-- one index; nothing earlier changes.
--
-- One row per answer the owner gave. `shape_key` is the prompt's normalized question text
-- (numbers and paths stripped) plus its sorted option set, so the same question asked again
-- with other numbers or paths in the same repo for the same agent maps to the same key. The
-- supervisor answers on the owner's behalf only from repeated, uncontradicted rows.
--
-- `prompt_ref` names the prompt that was answered (prompt id and screen revision). The card
-- route and the terminal observer both see the owner's one answer; the unique index makes the
-- second insert a no-op instead of a second precedent. NULL (no screen revision) never
-- collides; the code then falls back to a short same-shape, same-label time window.
CREATE TABLE IF NOT EXISTS supervisor_answer_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    shape_key TEXT NOT NULL CHECK(length(shape_key) > 0),
    repo TEXT NOT NULL,
    agent TEXT NOT NULL,
    option_label TEXT NOT NULL CHECK(length(option_label) > 0),
    at_ms INTEGER NOT NULL,
    prompt_ref TEXT
);
CREATE INDEX IF NOT EXISTS idx_supervisor_answer_history_shape
    ON supervisor_answer_history(shape_key, repo, agent, id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_supervisor_answer_history_prompt_ref
    ON supervisor_answer_history(repo, agent, prompt_ref);
