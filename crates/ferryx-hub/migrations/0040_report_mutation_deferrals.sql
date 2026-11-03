CREATE TABLE report_mutation_deferrals (
    outbox_id INTEGER NOT NULL REFERENCES chat_outbox(id) ON DELETE CASCADE,
    mutation_id TEXT NOT NULL,
    request_json TEXT NOT NULL,
    not_before TEXT NOT NULL,
    PRIMARY KEY(outbox_id, mutation_id)
);
