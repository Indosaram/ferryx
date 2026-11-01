-- Keep producer approval buttons intact across deferred transport retries.
ALTER TABLE chat_outbox ADD COLUMN blocks_json TEXT;
