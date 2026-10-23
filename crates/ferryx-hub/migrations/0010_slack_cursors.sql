-- omo-gateway's missed-message backfill cursor: the newest Slack message handled per bot,
-- channel and thread ('' for the channel itself).
CREATE TABLE IF NOT EXISTS slack_cursors (
    bot_id TEXT NOT NULL,
    channel_id TEXT NOT NULL,
    thread_ts TEXT NOT NULL DEFAULT '',
    last_ts TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (bot_id, channel_id, thread_ts)
);
