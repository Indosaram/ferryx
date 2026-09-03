# Ferryx All Agents Resume & Discovery Verification Report

**Date:** 2026-09-02  
**Subject:** Full Audit & Implementation of All Agents for Session Discovery and Automatic Reconnect / Resume  
**Status:** Completed & Verified  

---

## 1. Executive Summary

A comprehensive, zero-assumption empirical audit was conducted on every AI coding agent supported or referenced across Ferryx (`src-tauri` and `ui`). The audit verified:
1. Real CLI binaries installed on the host machine.
2. Official CLI help, session options (`--session`, `-s`, `--resume`), and resume arguments.
3. Actual filesystem and database storage formats used by each agent (`SQLite`, `.jsonl`, `.db`, `.json`).
4. Ferryx daemon session discovery (`src-tauri/src/ipc/agents.rs`) and UI reconnection affordance (`ui/src/lib/agentResume.ts`, `ui/src/lib/agentSessionDiscovery.ts`).

Following this audit, authoritative session discovery and automatic reconnection were implemented for:
- **`opencode`**: SQLite query via `sqlite3` CLI against `~/.local/share/opencode/opencode.db` matching `directory = <cwd>` with fallback to `opencode session list --format json -n 10`. Resume command: `opencode --session <id>`.
- **`pi`**: Multi-tier discovery (`PI_SESSION_FILE` environment variable, open-file `lsof` matching `/.pi/agent/sessions/` or `/.pi/sessions/`, and directory inspection in `~/.pi/agent/sessions/--<safe-cwd>--/`). Resume command: `pi --session <transcriptPath>`.

---

## 2. Complete Agent Compatibility Matrix

| Agent | Binary Installed | CLI Resume Spec | Session Storage Location | Discovery Mechanism | Ferryx Reconnect Status |
|---|---|---|---|---|---|
| **claude** | `/Users/indo/.local/bin/claude` | `claude --resume <uuid>` | `~/.claude/projects/...` | `lsof` open file inspection | ✅ **Authoritative Auto-Resume** |
| **codex** | `/opt/homebrew/bin/codex` | `codex resume <uuid>` | `~/.codex/sessions/...` | `lsof` open file inspection | ✅ **Authoritative Auto-Resume** |
| **omo** | `/Users/indo/.bun/bin/omo` | `omo --session <uuid>` | `~/.omo/sessions/...` | Process env (`PI_SESSION_FILE`) + `lsof` | ✅ **Authoritative Auto-Resume** |
| **antigravity** (`agy`) | `/Users/indo/.local/bin/agy` | `agy --conversation <uuid>` | `~/.gemini/antigravity-cli/cache/last_conversations.json` | JSON cache parser + `.db` lsof | ✅ **Authoritative Auto-Resume** |
| **gjc** | `/Users/indo/.bun/bin/gjc` | `gjc --resume <id>` | `~/.gjc/sessions/...` | `lsof` open file inspection | ✅ **Authoritative Auto-Resume** |
| **cursor-agent** (`cursor`) | `/Users/indo/.local/bin/cursor-agent` | `cursor-agent --resume <id>` | `~/.cursor/chats/...` | `lsof` open file inspection | ✅ **Authoritative Auto-Resume** |
| **copilot** | `/opt/homebrew/bin/copilot` | `copilot --resume <id>` | `~/.copilot/session-state/...` | `lsof` open file inspection | ✅ **Authoritative Auto-Resume** |
| **kimi** | `/Users/indo/.local/bin/kimi` | `kimi --resume <id>` | `~/.kimi/sessions/...` | `lsof` open file inspection | ✅ **Authoritative Auto-Resume** |
| **opencode** | `/Users/indo/.bun/bin/opencode` | `opencode --session <id>` (or `-s`) | `~/.local/share/opencode/opencode.db` | SQLite query + `session list --format json` fallback | ✅ **Authoritative Auto-Resume (New)** |
| **pi** | `/Users/indo/.bun/install/global/node_modules/.bin/pi` | `pi --session <transcriptPath>` | `~/.pi/agent/sessions/--<cwd>--/` | `PI_SESSION_FILE` env + `lsof` + project session scan | ✅ **Authoritative Auto-Resume (New)** |
| **aider** | Not installed | N/A | Current working dir chat history | No on-disk session ID (git history) | ⏸️ Unsupported by design (no ID) |
| **droid** | Not installed | `droid --resume <id>` | Vendor store | Vendor store unindexed | ⏸️ Needs vendor DB spec |

---

## 3. Implementation Details

### A. Backend (`src-tauri/src/ipc/agents.rs`)
1. **White-list update**: Added `"opencode"` and `"pi"` to `discover_agent_session_id`.
2. **Process matching**: Added `args_match_agent` rules for `opencode` and `pi` (matching binary basename and `@mariozechner/pi-coding-agent`).
3. **OpenCode discovery**:
   - Implemented `opencode_session_id(agent_pid, root_pid)`.
   - Queries `~/.local/share/opencode/opencode.db` using `sqlite3` command:
     `SELECT id FROM session WHERE directory = '<cwd>' ORDER BY time_updated DESC LIMIT 1;`.
   - Validates session ID pattern: starts with `ses_`, length >= 10, alphanumeric.
   - Robust fallback parser `parse_opencode_session_list_json` parsing `opencode session list --format json -n 10`.
4. **Pi discovery**:
   - Implemented `pi_session_id(agent_pid, root_pid)`.
   - First checks `omo_session_id_from_environment(pid, "pi")` for `PI_SESSION_FILE`.
   - Second checks `lsof_session_id(pid, "pi")` with marker `/.pi/`.
   - Third checks project-safe directory `~/.pi/agent/sessions/--<clean_cwd>--/` for the latest `.jsonl` session file.

### B. Frontend (`ui/src/lib/`)
1. **`agentResume.ts`**:
   - Added `"opencode"` and `"pi"` to `AUTHORITATIVE_RECONNECT_AGENTS`.
2. **`agentSessionDiscovery.ts`**:
   - Removed `"opencode"` from `UNSUPPORTED_DISCOVERY_AGENTS`.
   - Added `PI_STORE_RE` pattern matching `/.pi/agent/sessions/<dir>/<timestamp>_<uuid>.jsonl`.
3. **`agentResumeAffordance.ts`**:
   - `getAgentReconnectAffordance` now advertises Reconnect for `opencode` and `pi` sessions with authoritative capture.

---

## 4. Verification Evidence

### Backend Tests
- `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::agents`:
  - `opencode_session_id_validation`: **PASSED**
  - `opencode_session_list_json_parser_matches_cwd`: **PASSED**
  - `session_discovery_extracts_pi_session_path`: **PASSED**
  - `args_match_agent_recognizes_supported_commands`: **PASSED**
  - Total: 16 passed, 0 failed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib terminal::shell`:
  - 14 passed, 0 failed.

### Frontend Tests
- `bun run --cwd ui test`:
  - **136 test files passed (136/136)**
  - **1,362 tests passed (1,362/1,362)**
  - 0 failed.
