-- One owner notice per (session, owner turn, failure kind). The primary key is the dedup claim:
-- only the caller whose INSERT ... ON CONFLICT DO NOTHING inserts a row queues the notice.
-- state: 'claimed' (outbox row not yet written), 'queued' (outbox_id set), 'uncertain'
-- (the outbox row could not be written; the owner may not have been told).
CREATE TABLE IF NOT EXISTS turn_failure_notices (
    session_key TEXT NOT NULL,
    turn_seq TEXT NOT NULL,
    kind TEXT NOT NULL,
    state TEXT NOT NULL,
    outbox_id INTEGER REFERENCES chat_outbox(id),
    created_at TEXT NOT NULL,
    PRIMARY KEY (session_key, turn_seq, kind)
);
