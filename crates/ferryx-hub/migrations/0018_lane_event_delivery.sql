-- omo-gateway 0037: why a lane event left the queue (lead_turn, jev_skip, cron_remediation).
ALTER TABLE lane_events ADD COLUMN delivery_reason TEXT;
