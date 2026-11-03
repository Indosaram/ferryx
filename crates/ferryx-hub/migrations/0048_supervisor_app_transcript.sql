-- The app conversation with the supervisor (the panel composer and its replies). Additive:
-- one new table; every earlier table is unchanged.
--
-- One row per message, in commit order. `request_id` is UNIQUE: the owner message carries the
-- client's request id and the supervisor reply carries `reply:<request id>`, so a retried
-- request never stores a second copy. `evidence_json` is the claim's evidence list; a reply
-- that says it did something without evidence keeps it `[]` and the UI shows it as Unverified.
CREATE TABLE IF NOT EXISTS supervisor_app_transcript (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    role TEXT NOT NULL CHECK(role IN ('owner','supervisor')),
    text TEXT NOT NULL,
    evidence_json TEXT NOT NULL DEFAULT '[]',
    request_id TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL
);
