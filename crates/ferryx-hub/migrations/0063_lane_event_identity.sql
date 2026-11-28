-- Producer-owned identity and revision, independent of human-facing wording.
ALTER TABLE lane_events ADD COLUMN subject_id TEXT;
ALTER TABLE lane_events ADD COLUMN state_id TEXT;
ALTER TABLE lane_events ADD COLUMN wake_policy TEXT NOT NULL DEFAULT 'wake'
    CHECK (wake_policy IN ('wake', 'quiet'));
CREATE INDEX ix_lane_event_subject ON lane_events(lead_session_key, subject_id, id);
