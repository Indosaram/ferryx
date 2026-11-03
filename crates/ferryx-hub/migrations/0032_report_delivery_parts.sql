ALTER TABLE chat_outbox ADD COLUMN cancelled_at TEXT;
CREATE TABLE report_delivery_parts (
    outbox_id INTEGER NOT NULL REFERENCES chat_outbox(id) ON DELETE CASCADE,
    report_id TEXT NOT NULL,
    report_json TEXT NOT NULL,
    PRIMARY KEY (outbox_id, report_id)
);
