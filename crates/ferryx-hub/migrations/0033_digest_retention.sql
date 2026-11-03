-- Digest receipts must not prevent retention from deleting a finished outbox row.
CREATE TABLE report_digest_deliveries_retained (
    day TEXT PRIMARY KEY NOT NULL,
    outbox_id INTEGER NOT NULL REFERENCES chat_outbox(id) ON DELETE CASCADE
);
INSERT INTO report_digest_deliveries_retained(day, outbox_id)
SELECT day, outbox_id FROM report_digest_deliveries;
DROP TABLE report_digest_deliveries;
ALTER TABLE report_digest_deliveries_retained RENAME TO report_digest_deliveries;
