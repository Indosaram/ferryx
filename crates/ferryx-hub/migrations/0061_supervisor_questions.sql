-- Clarifications are tied to the authenticated conversation and the message that asked.
CREATE TABLE supervisor_questions (
    question_id TEXT PRIMARY KEY CHECK(length(question_id) = 36),
    supervisor_id TEXT NOT NULL REFERENCES supervisor_identity(supervisor_id),
    owner_principal TEXT NOT NULL CHECK(length(owner_principal) BETWEEN 1 AND 256),
    adapter TEXT NOT NULL CHECK(adapter IN ('slack','discord','desktop','phone','cli','mcp')),
    conversation_id TEXT NOT NULL CHECK(length(conversation_id) BETWEEN 1 AND 512),
    channel_id TEXT NOT NULL DEFAULT '' CHECK(length(channel_id) <= 256),
    thread_id TEXT NOT NULL DEFAULT '' CHECK(length(thread_id) <= 256),
    question_message_ts TEXT,
    missing TEXT NOT NULL CHECK(missing IN ('project','host')),
    options_json TEXT NOT NULL CHECK(json_valid(options_json) AND json_type(options_json) = 'array'),
    draft_json TEXT NOT NULL CHECK(json_valid(draft_json) AND json_type(draft_json) = 'object'),
    recommended_index INTEGER CHECK(recommended_index IS NULL OR recommended_index >= 1),
    status TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open','answered','superseded')),
    created_at TEXT NOT NULL,
    answered_at TEXT,
    FOREIGN KEY(supervisor_id, owner_principal)
        REFERENCES supervisor_identity(supervisor_id, owner_principal)
);
CREATE INDEX idx_supervisor_questions_open
    ON supervisor_questions(owner_principal, adapter, conversation_id, status, created_at);
