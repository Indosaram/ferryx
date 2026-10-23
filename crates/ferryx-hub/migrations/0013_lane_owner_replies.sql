-- omo-gateway lane_owner_replies: the owner's reply in a finished lane's Slack thread
-- reopens the lane with that text. message_id dedupes Slack redelivery.
CREATE TABLE IF NOT EXISTS lane_owner_replies (
    message_id TEXT PRIMARY KEY,
    lane_key TEXT NOT NULL,
    author_id TEXT NOT NULL,
    text TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    error TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_lane_owner_replies_lane ON lane_owner_replies(lane_key, state);
