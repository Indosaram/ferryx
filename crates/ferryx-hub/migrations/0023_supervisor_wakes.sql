-- Goal-aware wakes for the unified supervisor. Additive: new tables and triggers only;
-- every table and index of 0020 is unchanged.
--
-- supervisor_state_events is a durable, ordered log of goal run-state/revision changes and
-- dispatch state changes. The triggers write it inside the transaction that made the change,
-- so a change is logged exactly when it commits and no writer has to remember to notify.
-- The wake scheduler reads it from its persisted cursor (supervisor_event_cursors) and turns
-- it into idempotent wait events and pending wakes; a crash before the cursor moves only
-- replays effects that collapse on their unique keys.

CREATE TABLE IF NOT EXISTS supervisor_state_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    goal_id TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('goal','dispatch')),
    -- The dispatch's request id for source 'dispatch'.
    request_id TEXT,
    from_state TEXT,
    to_state TEXT NOT NULL,
    from_revision INTEGER,
    to_revision INTEGER,
    -- The dispatch's execution generation for source 'dispatch'.
    generation INTEGER,
    run_id TEXT,
    recorded_at TEXT NOT NULL
);

CREATE TRIGGER IF NOT EXISTS trg_supervisor_goal_state_event
AFTER UPDATE OF run_state, revision ON supervisor_goals
WHEN OLD.run_state IS NOT NEW.run_state OR OLD.revision IS NOT NEW.revision
BEGIN
    INSERT INTO supervisor_state_events(goal_id, source, from_state, to_state, from_revision,
            to_revision, recorded_at)
    VALUES(NEW.goal_id, 'goal', OLD.run_state, NEW.run_state, OLD.revision, NEW.revision,
            strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
END;

CREATE TRIGGER IF NOT EXISTS trg_supervisor_dispatch_state_event
AFTER UPDATE OF state ON supervisor_dispatches
WHEN OLD.state IS NOT NEW.state
BEGIN
    INSERT INTO supervisor_state_events(goal_id, source, request_id, from_state, to_state,
            from_revision, to_revision, generation, run_id, recorded_at)
    VALUES(NEW.goal_id, 'dispatch', NEW.request_id, OLD.state, NEW.state, NEW.goal_revision,
            NEW.goal_revision, NEW.generation, NEW.run_id,
            strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
END;

-- How far each consumer has applied supervisor_state_events.
CREATE TABLE IF NOT EXISTS supervisor_event_cursors (
    consumer TEXT PRIMARY KEY CHECK(length(consumer) BETWEEN 1 AND 64),
    last_seq INTEGER NOT NULL DEFAULT 0 CHECK(last_seq >= 0),
    updated_at TEXT NOT NULL
);

-- One handler turn over a set of pending wakes. The unique key (goal, event set, goal
-- revision, execution generation) is also the wake-judgment cache: the same set at the same
-- revision and generation is judged once. A turn stays 'pending' until the handler
-- acknowledged it; a restart re-delivers it under the same id.
CREATE TABLE IF NOT EXISTS supervisor_wake_turns (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    goal_id TEXT NOT NULL REFERENCES supervisor_goals(goal_id),
    goal_revision INTEGER NOT NULL,
    generation INTEGER NOT NULL,
    event_set_sha256 TEXT NOT NULL CHECK(length(event_set_sha256) = 64),
    -- JSON array of the wake causes in the set.
    causes TEXT NOT NULL CHECK(json_valid(causes) AND json_type(causes) = 'array'),
    urgent INTEGER NOT NULL CHECK(urgent IN (0,1)),
    state TEXT NOT NULL CHECK(state IN ('pending','delivered','skipped','superseded')),
    judge_score REAL,
    created_at TEXT NOT NULL,
    settled_at TEXT,
    UNIQUE(goal_id, event_set_sha256, goal_revision, generation)
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_supervisor_wake_turns_one_pending
    ON supervisor_wake_turns(goal_id) WHERE state = 'pending';
CREATE INDEX IF NOT EXISTS idx_supervisor_wake_turns_goal
    ON supervisor_wake_turns(goal_id, state);

-- What consumed each pending wake: the turn that delivered or skipped it, or 'closed' (no
-- turn) when its goal had already ended. One receipt per wake.
CREATE TABLE IF NOT EXISTS supervisor_wake_receipts (
    wake_id INTEGER PRIMARY KEY REFERENCES supervisor_pending_wakes(id),
    turn_id INTEGER REFERENCES supervisor_wake_turns(id),
    disposition TEXT NOT NULL CHECK(disposition IN ('delivered','skipped','closed')),
    recorded_at TEXT NOT NULL,
    CHECK((disposition = 'closed') = (turn_id IS NULL))
);
CREATE INDEX IF NOT EXISTS idx_supervisor_wake_receipts_turn
    ON supervisor_wake_receipts(turn_id) WHERE turn_id IS NOT NULL;
