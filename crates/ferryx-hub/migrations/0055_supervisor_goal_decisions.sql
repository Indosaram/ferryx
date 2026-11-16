-- Goal contract decisions for worker prompts (supervisor autonomy, todo 7). Additive: one new
-- table and one index; nothing earlier changes.
--
-- A decision says, for one prompt of a goal the supervisor dispatched, which option label the
-- goal's contract supports, recorded against the goal revision that was current when the owner
-- (or the admitted contract) decided it. The decision gate answers a pane prompt from these
-- rows before it falls back to the answer history and to the engine, so a goal's own terms
-- outrank what the owner picked in some earlier session. A decision of an older revision is
-- never used: an owner correction supersedes it.
CREATE TABLE IF NOT EXISTS supervisor_goal_decisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    goal_id TEXT NOT NULL CHECK(length(goal_id) = 36),
    goal_revision INTEGER NOT NULL CHECK(goal_revision >= 1),
    prompt_id TEXT NOT NULL CHECK(length(prompt_id) BETWEEN 1 AND 256),
    option_label TEXT NOT NULL CHECK(length(option_label) > 0),
    source TEXT NOT NULL CHECK(source IN ('owner','contract')),
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_supervisor_goal_decisions_prompt
    ON supervisor_goal_decisions(goal_id, goal_revision, prompt_id);
