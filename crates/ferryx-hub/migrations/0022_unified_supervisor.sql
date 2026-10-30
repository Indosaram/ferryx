-- Unified supervisor: one responsible identity per owner installation, the projects and
-- channels it answers for, and the causal record of admitted work. Every table is new and
-- empty after the upgrade: an existing lane is never scheduled by this schema until an
-- explicit adoption writes its supervisor_goals row.
--
-- Requirements stay in lane_goal_requirements and turns in lane_turns; a supervisor goal
-- references its lane_goal instead of copying them. supervisor_goals.revision is the
-- authoritative revision of the new path (owner corrections and owner holds bump it);
-- lane_goal.revision keeps counting requirement-set changes for evidence writes.
--
-- Every row that names an owner principal carries it with its supervisor id, and a composite
-- foreign key pins the pair to supervisor_identity: no goal, request or channel binding can
-- name an owner other than its supervisor's.

CREATE TABLE IF NOT EXISTS supervisor_identity (
    supervisor_id TEXT PRIMARY KEY CHECK(length(supervisor_id) = 36),
    owner_principal TEXT NOT NULL UNIQUE CHECK(length(owner_principal) BETWEEN 1 AND 256),
    -- The native mhc memory name. It is never a machine or chat-thread id.
    memory_name TEXT NOT NULL DEFAULT 'ferryx-supervisor' CHECK(memory_name = 'ferryx-supervisor'),
    created_at TEXT NOT NULL,
    UNIQUE(supervisor_id, owner_principal)
);

CREATE TABLE IF NOT EXISTS supervisor_projects (
    project_id TEXT PRIMARY KEY CHECK(length(project_id) = 36),
    supervisor_id TEXT NOT NULL REFERENCES supervisor_identity(supervisor_id),
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 256),
    -- Workspace/repository identity (e.g. a normalized origin URL). Never a raw local path.
    repo_identity TEXT NOT NULL CHECK(length(repo_identity) BETWEEN 1 AND 1024),
    created_at TEXT NOT NULL,
    UNIQUE(supervisor_id, project_id),
    UNIQUE(supervisor_id, repo_identity)
);

-- Where a project lives on each host: the canonical git common dir on that host.
CREATE TABLE IF NOT EXISTS supervisor_project_hosts (
    project_id TEXT NOT NULL REFERENCES supervisor_projects(project_id),
    host_id TEXT NOT NULL CHECK(length(host_id) BETWEEN 1 AND 256),
    git_common_dir TEXT NOT NULL CHECK(length(git_common_dir) BETWEEN 1 AND 4096),
    updated_at TEXT NOT NULL,
    PRIMARY KEY(project_id, host_id),
    UNIQUE(host_id, git_common_dir)
);

CREATE TABLE IF NOT EXISTS supervisor_channel_bindings (
    binding_id TEXT PRIMARY KEY CHECK(length(binding_id) = 36),
    supervisor_id TEXT NOT NULL,
    provider TEXT NOT NULL CHECK(provider IN ('slack','discord','desktop','phone','cli','mcp')),
    workspace_id TEXT NOT NULL DEFAULT '',
    account_id TEXT NOT NULL DEFAULT '',
    channel_id TEXT NOT NULL CHECK(length(channel_id) BETWEEN 1 AND 256),
    thread_id TEXT NOT NULL DEFAULT '',
    owner_principal TEXT NOT NULL CHECK(length(owner_principal) BETWEEN 1 AND 256),
    project_id TEXT,
    goal_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(provider, workspace_id, account_id, channel_id, thread_id, owner_principal),
    FOREIGN KEY(supervisor_id, owner_principal)
        REFERENCES supervisor_identity(supervisor_id, owner_principal),
    FOREIGN KEY(supervisor_id, project_id) REFERENCES supervisor_projects(supervisor_id, project_id),
    FOREIGN KEY(supervisor_id, goal_id) REFERENCES supervisor_goals(supervisor_id, goal_id)
);

-- Persisted run state, separate from lanes.status (which is projected, never scheduled from).
CREATE TABLE IF NOT EXISTS supervisor_goals (
    goal_id TEXT PRIMARY KEY CHECK(length(goal_id) = 36),
    supervisor_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    lane_key TEXT NOT NULL UNIQUE REFERENCES lane_goal(lane_key),
    owner_principal TEXT NOT NULL CHECK(length(owner_principal) BETWEEN 1 AND 256),
    run_state TEXT NOT NULL CHECK(run_state IN ('ready','dispatching','running','waiting_children','waiting_event','awaiting_owner','verifying','completed','paused','degraded','delivery_uncertain','cancelled')),
    state_detail TEXT,
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision >= 1),
    -- Operations the admitted request was granted, a JSON array of operation classes
    -- ("edit", "test", "commit", "push", "publish", "spend", "destructive"). Fixed at
    -- admission: what the request declared, limited to the baseline and the owner's explicit
    -- choices. An empty array grants no write operation (a read-only request).
    operation_grant TEXT NOT NULL DEFAULT '[]'
        CHECK(json_valid(operation_grant) AND json_type(operation_grant) = 'array'),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(supervisor_id, goal_id),
    FOREIGN KEY(supervisor_id, owner_principal)
        REFERENCES supervisor_identity(supervisor_id, owner_principal),
    FOREIGN KEY(supervisor_id, project_id) REFERENCES supervisor_projects(supervisor_id, project_id)
);
CREATE INDEX IF NOT EXISTS idx_supervisor_goals_ready
    ON supervisor_goals(updated_at) WHERE run_state = 'ready';
CREATE INDEX IF NOT EXISTS idx_supervisor_goals_state ON supervisor_goals(run_state, updated_at);

-- An admitted owner request. One row per source message; a replay with the same body maps to
-- the same goal and a changed body is a conflict, never a second launch.
CREATE TABLE IF NOT EXISTS supervisor_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_adapter TEXT NOT NULL CHECK(source_adapter IN ('slack','discord','desktop','phone','cli','mcp')),
    source_message_id TEXT NOT NULL CHECK(length(source_message_id) BETWEEN 1 AND 512),
    body TEXT NOT NULL CHECK(length(body) >= 1),
    body_sha256 TEXT NOT NULL CHECK(length(body_sha256) = 64),
    supervisor_id TEXT NOT NULL,
    goal_id TEXT NOT NULL,
    owner_principal TEXT NOT NULL,
    channel_binding_id TEXT REFERENCES supervisor_channel_bindings(binding_id),
    admitted_revision INTEGER NOT NULL CHECK(admitted_revision >= 1),
    created_at TEXT NOT NULL,
    UNIQUE(source_adapter, source_message_id),
    FOREIGN KEY(supervisor_id, owner_principal)
        REFERENCES supervisor_identity(supervisor_id, owner_principal),
    FOREIGN KEY(supervisor_id, goal_id) REFERENCES supervisor_goals(supervisor_id, goal_id)
);
CREATE INDEX IF NOT EXISTS idx_supervisor_requests_goal ON supervisor_requests(goal_id, id);

-- The worker a goal is bound to. A typed binding: lanes.omo_thread_id is never reused for it.
-- `generation` only grows; a stale generation may neither dispatch nor settle a newer run.
CREATE TABLE IF NOT EXISTS supervisor_execution_bindings (
    goal_id TEXT PRIMARY KEY REFERENCES supervisor_goals(goal_id),
    kind TEXT NOT NULL CHECK(kind IN ('pty','mhc')),
    host_id TEXT NOT NULL CHECK(length(host_id) BETWEEN 1 AND 256),
    provider_session_id TEXT CHECK(provider_session_id IS NULL OR length(provider_session_id) >= 1),
    run_id TEXT CHECK(run_id IS NULL OR length(run_id) >= 1),
    generation INTEGER NOT NULL DEFAULT 1 CHECK(generation >= 1),
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_supervisor_execution_session
    ON supervisor_execution_bindings(kind, host_id, provider_session_id)
    WHERE provider_session_id IS NOT NULL;

-- One dispatch intent and its delivery receipts. accepted/started/settled/uncertain are
-- separate facts with separate timestamps; acceptance is never read as a start. A start or
-- settle names the run and the provider session that observed it. A dispatch held
-- `uncertain` may still be resolved to started/settled by a receipt that names its request,
-- generation and session (restart reconciliation); uncertain_code/uncertain_at stay as history.
--
-- In flight: claimed, accepted, started, or uncertain and not yet released by the owner
-- (owner_resolved_at IS NULL). At most one dispatch per goal and execution generation is in
-- flight; the partial unique index below enforces it, so a write path that forgets the check
-- fails on the constraint instead of sending a second instruction to a busy worker.
-- `released` is a claim given back before transmission: never sent, never in flight.
CREATE TABLE IF NOT EXISTS supervisor_dispatches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    goal_id TEXT NOT NULL REFERENCES supervisor_goals(goal_id),
    goal_revision INTEGER NOT NULL CHECK(goal_revision >= 1),
    ordinal INTEGER NOT NULL CHECK(ordinal >= 1),
    -- Stable transport request id, transmitted at most once.
    request_id TEXT NOT NULL UNIQUE CHECK(length(request_id) >= 1),
    generation INTEGER NOT NULL CHECK(generation >= 1),
    instruction TEXT NOT NULL CHECK(length(instruction) >= 1),
    state TEXT NOT NULL CHECK(state IN ('ready','claimed','accepted','started','settled','uncertain','superseded','released')),
    run_id TEXT,
    provider_session_id TEXT,
    uncertain_code TEXT,
    claimed_at TEXT,
    accepted_at TEXT,
    started_at TEXT,
    settled_at TEXT,
    uncertain_at TEXT,
    -- When the owner resolved this uncertain delivery and allowed a resend.
    owner_resolved_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(goal_id, goal_revision, ordinal),
    CHECK(state NOT IN ('started','settled') OR (run_id IS NOT NULL AND provider_session_id IS NOT NULL)),
    CHECK(state <> 'uncertain' OR uncertain_code IS NOT NULL),
    CHECK(owner_resolved_at IS NULL OR uncertain_at IS NOT NULL),
    CHECK(state NOT IN ('claimed','accepted','started','settled','released') OR claimed_at IS NOT NULL)
);
CREATE INDEX IF NOT EXISTS idx_supervisor_dispatches_ready
    ON supervisor_dispatches(created_at, id) WHERE state = 'ready';
-- At most one unsent intent per goal: two ready intents would be two launches.
CREATE UNIQUE INDEX IF NOT EXISTS uq_supervisor_dispatches_one_ready
    ON supervisor_dispatches(goal_id) WHERE state = 'ready';
-- At most one dispatch in flight per goal and execution generation.
CREATE UNIQUE INDEX IF NOT EXISTS uq_supervisor_dispatches_one_in_flight
    ON supervisor_dispatches(goal_id, generation)
    WHERE state IN ('claimed','accepted','started')
       OR (state = 'uncertain' AND owner_resolved_at IS NULL);
CREATE INDEX IF NOT EXISTS idx_supervisor_dispatches_goal ON supervisor_dispatches(goal_id, state);

-- What a goal waits on: a child goal or a named external event. Both ends belong to the same
-- supervisor (and so the same owner); self-waits are refused here and cycles before commit.
CREATE TABLE IF NOT EXISTS supervisor_waits (
    wait_id TEXT PRIMARY KEY CHECK(length(wait_id) = 36),
    supervisor_id TEXT NOT NULL,
    goal_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('child_goal','external_event')),
    child_goal_id TEXT,
    event_key TEXT,
    state TEXT NOT NULL DEFAULT 'pending' CHECK(state IN ('pending','satisfied','failed','cancelled')),
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    FOREIGN KEY(supervisor_id, goal_id) REFERENCES supervisor_goals(supervisor_id, goal_id),
    FOREIGN KEY(supervisor_id, child_goal_id) REFERENCES supervisor_goals(supervisor_id, goal_id),
    CHECK(child_goal_id IS NULL OR child_goal_id <> goal_id),
    CHECK((kind = 'child_goal' AND child_goal_id IS NOT NULL AND event_key IS NULL)
       OR (kind = 'external_event' AND child_goal_id IS NULL AND event_key IS NOT NULL
           AND length(event_key) >= 1))
);
CREATE INDEX IF NOT EXISTS idx_supervisor_waits_goal ON supervisor_waits(goal_id, state);
CREATE INDEX IF NOT EXISTS idx_supervisor_waits_child
    ON supervisor_waits(child_goal_id) WHERE child_goal_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_supervisor_waits_event
    ON supervisor_waits(event_key) WHERE event_key IS NOT NULL;

-- Each event delivered to a wait, once.
CREATE TABLE IF NOT EXISTS supervisor_wait_events (
    wait_id TEXT NOT NULL REFERENCES supervisor_waits(wait_id),
    event_id TEXT NOT NULL CHECK(length(event_id) BETWEEN 1 AND 512),
    outcome TEXT NOT NULL CHECK(outcome IN ('progress','satisfied','failed')),
    evidence_json TEXT NOT NULL DEFAULT '[]',
    received_at TEXT NOT NULL,
    PRIMARY KEY(wait_id, event_id)
);

-- A wake persisted before anyone is notified. Consumed once; replays of the same cause and
-- event collapse onto one row.
CREATE TABLE IF NOT EXISTS supervisor_pending_wakes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    goal_id TEXT NOT NULL REFERENCES supervisor_goals(goal_id),
    cause TEXT NOT NULL CHECK(cause IN ('dependency_ready','dependency_failed','owner_correction','owner_control','turn_settled','deadline','report')),
    event_id TEXT NOT NULL CHECK(length(event_id) BETWEEN 1 AND 512),
    goal_revision INTEGER NOT NULL CHECK(goal_revision >= 1),
    generation INTEGER NOT NULL CHECK(generation >= 1),
    due_at TEXT NOT NULL,
    consumed_at TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(goal_id, cause, event_id)
);
CREATE INDEX IF NOT EXISTS idx_supervisor_pending_wakes_due
    ON supervisor_pending_wakes(due_at, id) WHERE consumed_at IS NULL;
