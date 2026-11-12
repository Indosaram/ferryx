-- Durable record of a lane brief delivery. A brief counts as sent only when the send result
-- carried the success marker; then the path and the sha256 (lowercase hex) of the brief file
-- content are recorded. A missing or different ack makes the next drive re-send the brief.
-- An ambiguous send records its escalation code instead and never an ack.
-- No backfill: past lanes have no ack.
ALTER TABLE lanes ADD COLUMN brief_ack_path TEXT;
ALTER TABLE lanes ADD COLUMN brief_ack_sha256 TEXT;
ALTER TABLE lanes ADD COLUMN brief_ack_at TEXT;
ALTER TABLE lanes ADD COLUMN brief_escalation TEXT;
