-- Card updates converge to a durable desired state; attachments retain their claims.
CREATE TABLE report_mutations_next (
    outbox_id INTEGER NOT NULL REFERENCES chat_outbox(id) ON DELETE CASCADE,
    mutation_id TEXT NOT NULL,
    request_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('claimed','delivered','uncertain','accepted')),
    owner_token TEXT,
    owner_instance TEXT,
    owner_pid INTEGER,
    claim_until TEXT,
    card_gen INTEGER,
    PRIMARY KEY(outbox_id, mutation_id)
);
INSERT INTO report_mutations_next
SELECT m.outbox_id, m.mutation_id, m.request_json,
    CASE WHEN json_extract(m.request_json, '$.change.kind') <> 'attachment'
        AND m.status <> 'delivered' THEN 'accepted' ELSE m.status END,
    m.owner_token, m.owner_instance, m.owner_pid, m.claim_until,
    CASE WHEN json_extract(m.request_json, '$.change.kind') <> 'attachment' THEN
        (SELECT COUNT(*) FROM report_mutations p WHERE p.outbox_id=m.outbox_id
         AND json_extract(p.request_json, '$.change.kind') <> 'attachment' AND p.rowid<=m.rowid)
    END
FROM report_mutations m ORDER BY m.rowid;
DROP TABLE report_mutations;
ALTER TABLE report_mutations_next RENAME TO report_mutations;
CREATE TABLE report_cards (
    outbox_id INTEGER PRIMARY KEY REFERENCES chat_outbox(id) ON DELETE CASCADE,
    desired_gen INTEGER NOT NULL,
    desired_mutation_id TEXT NOT NULL,
    writer_token TEXT,
    writer_instance TEXT,
    writer_pid INTEGER,
    writer_until TEXT,
    check_after TEXT,
    check_backoff_secs INTEGER NOT NULL DEFAULT 0
);
INSERT INTO report_cards(outbox_id, desired_gen, desired_mutation_id)
SELECT m.outbox_id, m.card_gen, m.mutation_id FROM report_mutations m
WHERE m.card_gen=(SELECT MAX(p.card_gen) FROM report_mutations p WHERE p.outbox_id=m.outbox_id);
CREATE INDEX report_cards_due ON report_cards(check_after);
