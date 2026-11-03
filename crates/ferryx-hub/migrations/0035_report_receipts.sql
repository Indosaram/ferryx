-- A posted card is identified by the platform receipt, never by admission alone.
ALTER TABLE chat_outbox ADD COLUMN posted_channel TEXT;
ALTER TABLE chat_outbox ADD COLUMN posted_ts TEXT;
