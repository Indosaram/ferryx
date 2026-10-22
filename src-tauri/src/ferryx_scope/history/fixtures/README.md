Redacted store samples, copied structurally from the measured captures in `.omo-parity/facts`.
Message text is replaced with `*-sentinel` markers and `__CWD__` stands in for the absolute working
directory, which each test rewrites to its own temporary directory. Record keys, record order and
record types are verbatim.

| Fixture | Source capture |
|---|---|
| `omo/session.jsonl` | `.omo-parity/facts/local-1.md:358-372` (senpi record dump) |
| `omp/session.jsonl` | `.omo-parity/facts/raw-omp/sample-session.jsonl` |
| `gjc/session.jsonl` | `.omo-parity/facts/gjc-raw/sample-session.jsonl` |
| `copilot/session.jsonl` | `.omo-parity/facts/local-3.md:86-91` |
| `droid/session.jsonl` | `.omo-parity/facts/remote-droid.md:150-153` |
| `kimi/context.jsonl` | `.omo-parity/facts/local-2.md:257-262` |
| `kimi/wire.jsonl` | `.omo-parity/facts/kimi-wire-vocabulary.md` (event shapes from kimi-cli 1.12.0 `wire/types.py`, `kosong/message.py`) |
| `grok/summary.json`, `grok/updates.jsonl` | `.omo-parity/facts/grok-evidence/sample-summary.json`, `sample-updates-head.jsonl` |
| `cline/session.json`, `cline/messages.json` | `.omo-parity/facts/local-3.md:277-288` |

| `antigravity/transcript_full.jsonl` | `.omo-parity/facts/local-2.md:87-89` |
| `cursor/meta.json` | `.omo-parity/facts/local-3.md:180-182` (stored hex-encoded on disk) |
| `cursor/store.db` | Real `~/.cursor/chats/b8670c45df56ad6621fb57097feba1b4/41ae0fb1-…/store.db` (chat run in `/private/tmp/fx-cursor-jFjF`), WAL checkpointed, reduced to the root node and its children minus the 43 KB `<user_info>` context blob; no blob bytes edited |
| `antigravity/replied_transcript_full.jsonl` | Real agy transcript `06468bac-…`, minimized per `.omo-parity/facts/agy-assistant-records-2026-10-22.md` |
| `opencode/rows.json`, `crush/rows.json` | `.omo-parity/facts/local-1.md:255-262`, `.omo-parity/facts/remote-crush.md:115-117` |
