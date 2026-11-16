-- A read-only retrieval index over the notes and reference files of the owner's agent memory
-- (~/.omo/memory/agents/*/repo/{notes,reference}/**). Additive: one new table. The files are
-- never sent to owner ingest; a row holds the frontmatter description and keywords drawn from
-- the description and the first 2 KiB of the body, and is refreshed when the body hash changes.
CREATE TABLE IF NOT EXISTS supervisor_reference_docs (
    -- Path relative to the memory root (agents/<agent>/repo/...).
    path TEXT PRIMARY KEY,
    agent TEXT NOT NULL,
    description TEXT NOT NULL,
    body_sha256 TEXT NOT NULL CHECK(length(body_sha256) = 64),
    -- Space-separated lowercase tokens.
    keywords TEXT NOT NULL,
    indexed_at TEXT NOT NULL
);
