-- A sender claims an outbox row before posting it so overlapping senders (the writer's own
-- post, flush_pending, flush_stale, drain_outbox, another hub process) cannot post it twice.
-- The claim expires so a sender that crashed or was cancelled mid-post does not strand the row.
ALTER TABLE chat_outbox ADD COLUMN send_claim TEXT;
ALTER TABLE chat_outbox ADD COLUMN send_claim_until TEXT;
