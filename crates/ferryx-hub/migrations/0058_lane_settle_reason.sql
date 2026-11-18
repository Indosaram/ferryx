-- T6: why a row settled on its own.
--
-- 'inactivity' (seven days without a lifecycle write) or 'pr_merged' (the lane's pull request
-- merged, as T7's projection last observed it). NULL for a manual settle, for every row that has
-- never settled, and for a row whose state moved on to something else (the reducer clears it with
-- the other state timestamps, so a row never carries a reason for a state it is not in).
--
-- Additive only: 0056 is landed work, and an ALTER is how a landed table gains a column. No index:
-- nothing reads by reason, the sidebar renders it beside the row it belongs to.
ALTER TABLE lane_lifecycles ADD COLUMN settle_reason TEXT;
