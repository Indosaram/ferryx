CREATE TABLE owner_answer_obligations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    adapter TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    source_message_id TEXT NOT NULL,
    channel_id TEXT,
    thread_id TEXT,
    turn_status TEXT NOT NULL DEFAULT 'queued',
    admitted_at TEXT NOT NULL,
    reminded_count INTEGER NOT NULL DEFAULT 0,
    last_reminded_at TEXT,
    answered_at TEXT,
    delivery_state TEXT NOT NULL DEFAULT 'pending',
    last_error TEXT,
    UNIQUE(adapter, source_message_id)
);
CREATE INDEX owner_answer_obligations_pending
    ON owner_answer_obligations(answered_at, admitted_at);
