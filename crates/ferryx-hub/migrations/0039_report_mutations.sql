-- A claimed upload with no receipt is uncertain, never blindly replayed.
CREATE TABLE report_mutations (
    outbox_id INTEGER NOT NULL REFERENCES chat_outbox(id) ON DELETE CASCADE,
    mutation_id TEXT NOT NULL,
    request_json TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('claimed','delivered','uncertain')),
    PRIMARY KEY(outbox_id, mutation_id)
);
