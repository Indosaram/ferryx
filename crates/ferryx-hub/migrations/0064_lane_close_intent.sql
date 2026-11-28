-- Todo 6: a done claim settles only after an objection window and live re-checks.
-- The window's deadline and the recorded close intent live on the lane row so a crash
-- between "checks passed" and "lane closed" resumes the same close instead of re-deciding it.
ALTER TABLE lanes ADD COLUMN claim_objection_until TEXT;
ALTER TABLE lanes ADD COLUMN close_intent_json TEXT;
CREATE INDEX IF NOT EXISTS ix_lanes_objection_due
    ON lanes(claim_objection_until) WHERE close_intent_json IS NULL;
