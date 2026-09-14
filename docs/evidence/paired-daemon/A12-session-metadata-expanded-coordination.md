# A12 expanded-scope implementation ownership

st_01a098de owns only metadata additions: server agent report dispatch and MachineSessionMetadata owner request arm; protocol additive metadata request; client retry-kind exhaustiveness for that request; Session title/agentType; journal metadata update/exit merge; session_service output consumer startup/join and metadata projection. No paired-host sections and no whole-file formatting.

## Revision proposal to parent / st_01a098e5 (machine_events.rs stays untouched here)

Keep existing top-level sequence as sole ordering cursor. Session metadata events will publish_revision(committed session_revision, sessionMetadataChanged, ..., complete Session payload). Add explicit snapshot payload/session revision (already Sessions.revision) and define top-level inventory revision as an opaque composite project/session revision, e.g. `p:<projects.revision>;s:<sessions.revision>`; use it only for equality/staleness, never numeric ordering. Alternatively retain legacy top-level project revision for compatibility and add `sessionRevision` to snapshot/event envelopes. Recommendation: additive `sessionRevision` and use sequence for event ordering, preserving the existing project revision contract. Parent must own composition and downstream consumer agreement; this lane will not silently change machine_events.rs.

## Implementation

A session-lifetime subscription uses NativeTerminal/TerminalEngine, not a new OSC parser; bounded existing output history/broadcast and engine scrollback limit. It carries the exact committed target and reads CWD only from that owner's PTY PID, independently of active selection. Metadata transactions compare full target and running status, preserve creator/workspace, update only title/CWD/validated provider metadata, retain fields on exit. Provider hints from canonical agent reports are compared to owner discovery and authoritative transcript resolution before claim/publication. Additive IPC forwards the report to the original owner based on session_router; gateway epoch is not used as owner authority.
