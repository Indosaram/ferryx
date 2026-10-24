-- Supervisor goal contract: one goal per lane, its ID'd requirements, owner-quoted revisions,
-- and the ledger of who drove each lane turn. A lane without a lane_goal row is never
-- touched by the goal loop.
CREATE TABLE IF NOT EXISTS lane_goal (
    lane_key TEXT PRIMARY KEY REFERENCES lanes(key),
    mode TEXT NOT NULL DEFAULT 'supervised' CHECK(mode IN ('off','supervised','full')),
    turn_limit INTEGER NOT NULL DEFAULT 20,
    turns_used INTEGER NOT NULL DEFAULT 0,
    revision INTEGER NOT NULL DEFAULT 1,
    stop_reason TEXT,
    stop_detail TEXT,
    proposal_text TEXT,
    proposal_gap_ids TEXT,
    proposal_state TEXT CHECK(proposal_state IN ('pending','awaiting_approval','sent','superseded','discarded','uncertain')),
    proposal_request_id TEXT,
    -- When the current proposal was made; a human turn after it supersedes the proposal.
    proposal_at TEXT,
    -- Consecutive `complete` decisions the blind convergence check refused. Two in a row stop
    -- the lane with completion_evidence_missing.
    converge_misses INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS lane_goal_requirements (
    lane_key TEXT NOT NULL REFERENCES lane_goal(lane_key),
    req_id TEXT NOT NULL,
    text TEXT NOT NULL,
    verifier TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','met','unmet','waived')),
    evidence_json TEXT NOT NULL DEFAULT '[]',
    updated_at TEXT NOT NULL,
    PRIMARY KEY (lane_key, req_id)
);

CREATE TABLE IF NOT EXISTS lane_goal_revisions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    lane_key TEXT NOT NULL REFERENCES lane_goal(lane_key),
    change_json TEXT NOT NULL,
    instruction_quote TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'proposed' CHECK(state IN ('proposed','accepted','rejected')),
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lane_goal_revisions_lane ON lane_goal_revisions(lane_key, state);

-- Raw keystrokes are never stored: a keystroke turn has a NULL text_excerpt.
CREATE TABLE IF NOT EXISTS lane_turns (
    lane_key TEXT NOT NULL REFERENCES lanes(key),
    seq INTEGER NOT NULL,
    origin TEXT NOT NULL CHECK(origin IN ('submitted_request','supervisor_proposal','human_correction','other')),
    text_excerpt TEXT CHECK(text_excerpt IS NULL OR length(text_excerpt) <= 240),
    screen_revision TEXT,
    at TEXT NOT NULL,
    PRIMARY KEY (lane_key, seq)
);
CREATE INDEX IF NOT EXISTS idx_lane_turns_origin ON lane_turns(lane_key, origin, at);
