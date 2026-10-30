-- Owner holds remember what they interrupted (todo 13). Additive: two nullable columns on
-- supervisor_goals; every table, index and trigger of 0020 and 0021 is unchanged.
--
-- An owner pause through the supervisor request service records the run state and detail
-- the goal had before the pause, so a resume restores it (or the state its facts moved to
-- during the pause: a settled run goes to verifying, pending waits to waiting) instead of
-- stranding the goal in ready. NULL for goals that were never paused that way.
ALTER TABLE supervisor_goals ADD COLUMN held_from TEXT
    CHECK(held_from IS NULL OR held_from IN ('ready','dispatching','running','waiting_children',
        'waiting_event','awaiting_owner','verifying','degraded','delivery_uncertain'));
ALTER TABLE supervisor_goals ADD COLUMN held_detail TEXT;
