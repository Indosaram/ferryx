-- Server-owned lane lifecycle: pin / settle / snooze / archive per sidebar worktree row.
--
-- The lifecycle key is the sidebay row's WORKTREE identity, not a tab and not a lane key:
--   managed worktree  -> 'w:<wsId>/<slug>'   (from branch `orca/<wsId>/<slug>`, stable across
--                                             a checkout moving on disk)
--   anything else     -> 'p:<workspaceId>/<path>' (project roots, plain folders, unmanaged
--                                             worktrees: the path is the only identity there is)
-- Tab pins (`SET_TAB_PINNED`) are per tab and have no identity of their own, so they are FOLDED
-- into the owning worktree row as `pinned_tabs` (a per-worktree pin set) and never become keys.
-- See crates/ferryx-hub/src/lanes/lifecycle.rs for the mapping step and the read/apply paths.
CREATE TABLE IF NOT EXISTS lane_lifecycles (
    worktree_key TEXT PRIMARY KEY,
    -- The workspace the row is rendered under, so a read can order rows per project.
    workspace_id TEXT NOT NULL DEFAULT '',
    -- Last path seen for this key; '<slug>' rows keep it for display and for the tab-pin fold.
    worktree_path TEXT NOT NULL DEFAULT '',
    -- The lifecycle state. `snoozed` with an expired snoozed_until reads as `active`; the row is
    -- kept as snoozed so the expiry stays visible and needs no background job to be undone.
    state TEXT NOT NULL DEFAULT 'active'
        CHECK(state IN ('active','pinned','settled','snoozed','archived')),
    -- When each state was entered; the evidence `state` stands for (one is set per state).
    pinned_at TEXT,
    settled_at TEXT,
    snoozed_until TEXT,
    archived_at TEXT,
    -- The per-worktree pin set: JSON array of the pinned tabs' ids, folded from SET_TAB_PINNED.
    pinned_tabs TEXT NOT NULL DEFAULT '[]',
    -- Server-computed order inside the row's workspace section.
    position INTEGER NOT NULL DEFAULT 0,
    -- Monotonic per-row counter. A client undo compares it and refuses a stale write.
    revision INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- The read path lists every section in one query ordered by workspace, then position.
CREATE INDEX IF NOT EXISTS idx_lane_lifecycles_order
    ON lane_lifecycles(workspace_id, position, worktree_key);

-- Archive is the one state a read filters out of the live list.
CREATE INDEX IF NOT EXISTS idx_lane_lifecycles_state
    ON lane_lifecycles(state, worktree_key);
