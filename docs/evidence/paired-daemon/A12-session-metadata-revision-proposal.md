# Concrete minimal revision proposal (parent approval requested)

Read current machine_events.rs after cancellation merge, binding plan 6.1, machine_events regression and paired-host consumers. Top-level `revision` is currently ambiguously project catalog revision for snapshots/publish_revision and a cached max for unrevisioned events. Metadata currently supplies session revision, so comparisons across event kinds are invalid.

Recommended contract:
- Event envelope `sequence` and `revision` are the SAME canonical decimal monotonic machine-broadcaster cursor. Both advance once under existing sequence mutex per published event. They are independent of catalog and session journal revisions. Duplicate alias retained because plan requires both fields; sequence orders stream, revision versions that event boundary.
- Snapshot envelope `sequence == revision == cursor captured BEFORE snapshot`, preserving subscribe-before-snapshot replay rules.
- Snapshot `payload.projects.revision` and `payload.sessions.revision` remain untouched per-domain authority revisions.
- `publish_revision(domain_revision, ...)` additionally emits `sessionRevision` when sessionId is present, otherwise `projectRevision`, retaining passed revision without confusing it with event ordering. Unrevisioned publish need not claim a domain revision; either refactor wrapper to private optional-domain publisher or omit domain field for zero sentinel. Recommendation optional-domain publisher, keeping public existing call signatures.
- Metadata test compares event revision to snapshot EVENT revision and separately checks `event.sessionRevision == HTTP sessions.revision` only when no later session mutation occurs. Never compare envelope revision with HTTP list revision.

Exact edit surface: MachineEvents revision atomic removal, publish/publish_revision encoding, snapshot boundary revision assignment only. NO serve refresh/select/permit modifications. Existing machine_events test only demands string revision and changed event revision; contract remains compatible with those assertions. No native consumer found interpreting event revision as project revision.

Parent can apply these precise edits directly or approve this lane to apply them. This proposal supersedes the earlier additive-cached-revision suggestion.
