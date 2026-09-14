# A12 metadata integration coordination

Owner: st_01a098de. Server/native owner: st_01a098d0. Parent owns machine_events.rs.

Read findings:
- Machine Session currently has cwd/providerSession but no title/agentType metadata fields (remote/machine_protocol.rs:180).
- canonical AgentStateReport socket carries sessionId/state/agent/providerSession only; server.rs:1218 currently publishes directly to AgentStateHub without validating a machine owner.
- title notifications exist only in native_terminal surface/engine, not the daemon output hub. No event-driven authoritative headless CWD source exists in terminal service; process_cwd is a query used by spawn inheritance and provider discovery.
- machine event revision currently reflects project revision only; session journal has independent session_revision.

Exact additive server need (do not call with gateway epoch): route parsed canonical agent reports through an async session authority hook, resolving the raw session's exact owner first; validate provider identity using remote owner discovery/claim logic before machine publication. A client-supplied provider ID is NOT sufficient. Old-owner reports must be forwarded to/validated by that owner, not accepted as gateway-owned.

Proposed scoped implementation hook: DaemonSessionService::publish_machine_metadata(target: &RemoteTerminalTarget) -> Result<Session, String>, blocking owner-side operation called via run_blocking after a real local metadata source changes. It will derive live CWD from the owned PTY PID, retain only already-authoritative provider references, update owner metadata/journal, and publish machine-only metadata event with exact target. It must NOT accept arbitrary client metadata or probe a routed remote path locally. Title/agent provider discovery requires additional authoritative event source integration beyond this query hook; no invented IDs or polling permitted.

Parent action needed: approve/add event revision composition in machine_events.rs and supply an authoritative headless metadata-change trigger (not desktop client claims). No server/protocol/native files will be edited by this child.

Behavioral RED obtained: A12-session-metadata-RED.log, exit 101, test ran successfully then timed out specifically waiting for sessionMetadataChanged after observing real inactive PTY title output. Cleanup succeeded. This is not a compilation RED.

Concrete output hook: register a session-lifetime output subscription at the existing successful machine spawn boundary before acknowledging, carrying the committed RemoteTerminalTarget (never current gateway epoch), and cancel/join it at lifecycle end. Existing NativeTerminal + TerminalEngine can parse title controls (remote/mirror.rs demonstrates a headless consumer); do not add a guessed regex/OSC parser. The module needs retained title and agentType fields in machine Session and journal records so HTTP/reconnect snapshots agree with events. The exit task currently captures the original spawn MachineSession and save_session overwrites the entire record: metadata updates would be lost on exit unless lifecycle writes merge the latest authoritative record under the journal transaction. That must be coordinated with the journal spawn owner; child was expressly told not to refactor journal spawn code.

For agent reports: parsed report agent/provider values are hints, not proof. Resolve provider using the owned local PTY PID and existing ipc::agents::discover_agent_session_id; require exact supported key and discovered ID, validate transcript/CWD using the existing owner resolver, atomically claim before accepting, reject unsupported providers. For legacy sessions, invoke this on the original owner through an additive typed owner request, not on the gateway's PID/path table. server.rs DiscoverAgentSession handling already routes discovery through session_router; use that ownership seam rather than accepting reports at the successor.
