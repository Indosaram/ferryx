-- Voice replies read assistant rows written after the utterance, in insertion order.
ALTER TABLE messages ADD COLUMN sequence INTEGER;

CREATE INDEX IF NOT EXISTS idx_messages_session_sequence ON messages(session_key, sequence);
