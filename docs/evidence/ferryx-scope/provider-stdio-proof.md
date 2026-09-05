# Actual Codex stdio protocol proof

2026-09-05, Codex 0.153.2, isolated ephemeral thread. No Ferryx product UI exercised yet.

Command: `/Users/indo/.local/bin/codex app-server --listen stdio://` (monitor mon_WMPVG8XG525AZB70 / bash_44).

Requests:
```json
{"id":1,"method":"initialize","params":{"clientInfo":{"name":"ferryx_scope_ephemeral","title":"Ferryx Scope QA","version":"0.1.0"},"capabilities":{"experimentalApi":true}}}
{"method":"initialized"}
{"id":2,"method":"thread/start","params":{"cwd":"/tmp/ferryx-scope-ssh.Tody5Z/qa-one","ephemeral":true,"approvalPolicy":"untrusted","sandbox":"read-only"}}}
{"id":3,"method":"turn/start","params":{"threadId":"01a070f8-6983-7390-b7bb-e97810cdbe06","input":[{"type":"text","text":"Reply exactly FERRYX_SCOPE_PROVIDER_OK. Do not run tools or read files."}]}}
```

Observed thread/start returned provider-authored ID `01a070f8-6983-7390-b7bb-e97810cdbe06`, ephemeral true, path null, requested cwd, approval untrusted and read-only sandbox. Model gpt-5.6-luna. Turn/start returned `01a070f9-027a-7f50-b67f-86eb996a1ab6`. Received actual item/agentMessage/delta events and item/completed text `FERRYX_SCOPE_PROVIDER_OK`; thread status returned idle. Initial websocket 426 warning did not prevent completion through provider fallback.

This is evidence of native provider creation/input/output, NOT Ferryx chat integration, native approval or question cards. No thread was resumed, no user TUI ownership changed, no tool executed.

Cleanup receipt: `kill_bash({bash_id:"bash_44"})` completed and watcher delivered killed/exit 1. All data was ephemeral; schema files remain inside the separately tracked SSH fixture root until its teardown.
