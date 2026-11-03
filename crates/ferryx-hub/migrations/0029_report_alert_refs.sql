-- Owner report feedback capture (docs/design/hub-owner-reporting.md section 3, Capture).
-- Additive: one new table; every earlier table, row, index and trigger is unchanged.

-- One row per alert in a delivered owner message, keyed by the Slack message it was posted
-- as, so an owner reply or reaction on that message names the alert it objects to. A message
-- that carried several alerts has one row per alert.
CREATE TABLE IF NOT EXISTS delivered_alerts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    slack_channel TEXT NOT NULL,
    slack_ts TEXT NOT NULL,
    source TEXT NOT NULL,
    sender TEXT,
    subject TEXT,
    topic_key TEXT,
    alert_ref TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(slack_channel, slack_ts, alert_ref)
);
CREATE INDEX IF NOT EXISTS idx_delivered_alerts_created ON delivered_alerts(created_at);
