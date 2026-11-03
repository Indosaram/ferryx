-- A Slack outbox row posts as one or more rendered chunks. Progress is durable so a retry
-- resumes at the first chunk Slack did not confirm and never reposts an earlier one.
-- send_chunks_json: the chunks this row posts, fixed when its first chunk is about to be sent.
-- send_chunks_done: how many leading chunks Slack confirmed.
-- send_inflight: the chunk a sender is posting right now. It stays set when the outcome was
--   never recorded (crash, cancellation), and such a row is never claimed again.
-- send_uncertain_at / send_uncertain_error: the post may have reached Slack (timeout, dropped
--   connection, unreadable answer). Automatic retry stops so it can never duplicate the post.
ALTER TABLE chat_outbox ADD COLUMN send_chunks_json TEXT;
ALTER TABLE chat_outbox ADD COLUMN send_chunks_done INTEGER NOT NULL DEFAULT 0;
ALTER TABLE chat_outbox ADD COLUMN send_inflight INTEGER;
ALTER TABLE chat_outbox ADD COLUMN send_uncertain_at TEXT;
ALTER TABLE chat_outbox ADD COLUMN send_uncertain_error TEXT;
