-- A worker's own report id on a lane event. Additive: one nullable column and a partial
-- unique index; existing rows and writers are unchanged. A worker that sends the same report
-- twice (a retry, or a replay out of order) names the same id, and the lane keeps one event,
-- so its supervisor goal gets one wake (supervisor_pending_wakes is keyed by the event id).
ALTER TABLE lane_events ADD COLUMN source_event_id TEXT;
CREATE UNIQUE INDEX IF NOT EXISTS uq_lane_events_source
    ON lane_events(lane_key, source_event_id) WHERE source_event_id IS NOT NULL;
