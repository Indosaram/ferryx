-- Reversible legacy cutover (todo 16). Additive: three new tables and a partial unique index;
-- every earlier table, row, index and trigger is unchanged.
--
-- supervisor_cutover says which intake owns the owner's supervision: the legacy one (omo
-- app-server owner turns, the legacy lane goal loop, the kibitzer lead wake) or the unified
-- supervisor (admission, dispatcher, wake scheduler). The CHECK forbids both open at once, so
-- no ordering mistake can ever produce a duplicate intake: a cutover stops the legacy intake in
-- one commit and opens the unified one in a later commit; a rollback closes the unified intake
-- first and reopens the legacy one after. Closing an intake deletes nothing: goals, receipts
-- and dispatch rows stay, and no pane is touched.
CREATE TABLE IF NOT EXISTS supervisor_cutover (
    supervisor_id TEXT PRIMARY KEY REFERENCES supervisor_identity(supervisor_id),
    legacy_intake TEXT NOT NULL CHECK(legacy_intake IN ('open','stopped')),
    unified_intake TEXT NOT NULL CHECK(unified_intake IN ('open','closed')),
    origin TEXT NOT NULL CHECK(origin IN ('fresh','upgrade','cutover','rollback')),
    updated_at TEXT NOT NULL,
    CHECK(NOT (legacy_intake = 'open' AND unified_intake = 'open'))
);

-- Append-only: every intake change, in commit order.
CREATE TABLE IF NOT EXISTS supervisor_cutover_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    supervisor_id TEXT NOT NULL REFERENCES supervisor_identity(supervisor_id),
    step TEXT NOT NULL CHECK(step IN ('initialized','legacy_intake_stopped',
        'unified_intake_enabled','unified_intake_closed','legacy_intake_reopened')),
    detail TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_supervisor_cutover_log_owner
    ON supervisor_cutover_log(supervisor_id, id);

-- Present when this hub carried legacy work before the unified supervisor could run it: an
-- upgrade of a hub with lanes or chat sessions, or an omo import. Such an installation starts
-- with the legacy intake open and the unified one closed until an explicit cutover, so an
-- upgrade alone schedules nothing new. A hub without it starts on the unified intake.
CREATE TABLE IF NOT EXISTS supervisor_legacy_history (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    source TEXT NOT NULL CHECK(source IN ('upgrade','import')),
    detected_at TEXT NOT NULL
);
INSERT OR IGNORE INTO supervisor_legacy_history(id, source, detected_at)
SELECT 1, 'upgrade', strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
WHERE EXISTS(SELECT 1 FROM lanes WHERE key NOT LIKE 'sv-%')
   OR EXISTS(SELECT 1 FROM sessions)
   OR EXISTS(SELECT 1 FROM hub_meta WHERE key = 'imported_from');

-- One old lane (`adopt:<lane>`) or channel session (`adopt-session:<session key>`) maps to
-- exactly one admitted request, whichever adapter asked: the schema refuses a second one.
CREATE UNIQUE INDEX IF NOT EXISTS uq_supervisor_requests_one_adoption
    ON supervisor_requests(source_message_id)
    WHERE source_message_id LIKE 'adopt:%' OR source_message_id LIKE 'adopt-session:%';
