-- Keep the report-only wake judge's per-goal retry budget when the hub restarts.
-- Existing goals start without a deadline, and the columns don't alter owner or goal state.
ALTER TABLE supervisor_goals
    ADD COLUMN wake_judge_failures INTEGER NOT NULL DEFAULT 0
        CHECK(wake_judge_failures BETWEEN 0 AND 6);
ALTER TABLE supervisor_goals ADD COLUMN wake_judge_retry_at_ms INTEGER;
