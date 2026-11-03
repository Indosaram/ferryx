-- The failing cron run a report is about. Producers that observe the same failure (the job's
-- own collector and the hub's remediation) share it, so the owner hears about one failure once.
ALTER TABLE report_suppressions ADD COLUMN incident TEXT;
CREATE INDEX IF NOT EXISTS idx_report_suppressions_incident
    ON report_suppressions(incident, decision);
