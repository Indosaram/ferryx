-- omo-gateway auto worktrees: a lane launched at a repository root runs in its own
-- <repo>-lanes/<key> worktree on branch lane/<key>; close removes the tree, keeps the branch.
ALTER TABLE lane_owned_worktrees ADD COLUMN repo TEXT;
ALTER TABLE lane_owned_worktrees ADD COLUMN branch TEXT;
ALTER TABLE lane_owned_worktrees ADD COLUMN start_commit TEXT;
ALTER TABLE lane_owned_worktrees ADD COLUMN origin_cwd TEXT;
