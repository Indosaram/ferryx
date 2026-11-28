-- The report a settled goal sent to the chat conversation it came from (todo 13).
-- One row per (goal, revision, settle state): the primary key is the idempotency key, so a
-- replay or a restart never queues a second report. `outbox_id` names the chat_outbox row that
-- carries it; whether Slack accepted it is read from that row (delivered_at, send_uncertain_at).
-- `reconciled_at` is set once an uncertain post was settled by reading the thread
-- (conversations.replies); a post whose outcome is unknown is never re-posted blindly.
CREATE TABLE IF NOT EXISTS supervisor_goal_reports (
    goal_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision >= 1),
    state TEXT NOT NULL,
    provider TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    thread_id TEXT NOT NULL DEFAULT '',
    origin_message_id TEXT NOT NULL,
    outbox_id INTEGER REFERENCES chat_outbox(id),
    reconciled_at TEXT,
    posted_ts TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (goal_id, revision, state)
);
CREATE INDEX IF NOT EXISTS idx_supervisor_goal_reports_outbox ON supervisor_goal_reports(outbox_id);
