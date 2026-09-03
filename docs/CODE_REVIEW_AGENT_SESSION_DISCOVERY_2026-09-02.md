# Code Review Report: Agent Session Discovery & Resume Parity

**Date:** 2026-09-02  
**Target:** `opencode`, `pi`, and related agent session discovery & resume changes in Ferryx (`orca-lite`)  
**Status:** ALL LANES PASSED (with Remediation Applied)  

---

## 1. Executive Summary

A comprehensive 5-lane review was conducted across the implementation in:
- `src-tauri/src/ipc/agents.rs`
- `src-tauri/src/terminal/shell.rs`
- `src-tauri/src/daemon/server.rs`
- `ui/src/lib/agentResume.ts`
- `ui/src/lib/agentSessionDiscovery.ts`
- `ui/src/lib/agentResumeAffordance.ts`
- Associated unit & contract test suites

All 5 lanes have passed. One architectural violation identified during the review (synchronous blocking I/O and process spawns inside the async Tokio reactor loop) was immediately remediated by wrapping `discover_agent_session_id` in `tokio::task::spawn_blocking` in `server.rs`.

---

## 2. Lane-by-Lane Review Results

| # | Review Area | Verdict | Summary |
|---|---|---|---|
| **1** | **Goal & Constraint Verification** | **PASS** | Ferryx never mints or invents session IDs. Exactly discovers real IDs from agent stores. Resume command syntax matches CLI specs (`opencode --session <id>`, `pi --session <transcriptPath>`). |
| **2** | **Robustness & Edge Cases** | **PASS** | Missing CLI/DB gracefully returns `None`. Paths with trailing slashes and spaces handled properly (`encode_pi_safe_path` unit-tested). Corrupted JSONL files skipped safely. |
| **3** | **Code Quality & Consistency** | **PASS** | No `unsafe` in new code, no panics, no unwrap on untrusted input. Strict TypeScript types and regex anchors. |
| **4** | **Security & Safety** | **PASS** | SQLite queries use vector args (no shell execution). Single quotes in directory paths escaped (`''`). Returned IDs validated against strict alphanumeric patterns before use. |
| **5** | **Context & AGENTS.md Conformance** | **PASS** | Synchronous disk/process I/O offloaded to `spawn_blocking` per AGENTS.md rule. All unit and contract tests passing (17 Rust agent tests, 34 daemon server tests, 136 UI test files / 1362 tests). |

---

## 3. Detailed Findings & Remediation

### Finding 1: Async Reactor Offloading (Remediated)
- **Issue**: In `src-tauri/src/daemon/server.rs`, `DaemonRequest::DiscoverAgentSession` called `crate::ipc::agents::discover_agent_session_id(pid, &agent_type)` synchronously inside the connection's `tokio::select!` loop. Because `discover_agent_session_id` spawns `ps`, `lsof`, `sqlite3`, and inspects filesystem directories, this violated the project convention: *"NEVER execute synchronous disk I/O or external subprocess lookups on async Tokio runtime threads"*.
- **Fix Applied**: Wrapped discovery execution in `tokio::task::spawn_blocking(move || { crate::ipc::agents::discover_agent_session_id(pid, &agent_type) }).await.ok().flatten()`.

### Finding 2: `encode_pi_safe_path` Slug Alignment (Remediated)
- **Issue**: `encode_pi_safe_path` trimmed only leading slashes, causing paths with trailing slashes (e.g., `/path/to/project/`) to produce unintended triple dashes (`--path-to-project---`).
- **Fix Applied**: Updated to `cwd.trim_matches(|c| c == '/' || c == '\\')` and added regression tests `test_encode_pi_safe_path` covering normal Unix paths, paths with trailing slashes, and Windows paths.

### Finding 3: OpenCode Same-Directory Session Scope Note
- **Note**: OpenCode records sessions in a single shared SQLite database keyed by working directory. If multiple terminal panes run OpenCode in the exact same directory, both panes will discover the newest session for that directory upon daemon restart. This is an inherent design characteristic of OpenCode's storage model (unlike Claude/Pi which maintain per-process file handles or environment variables).

---

## 4. Verification Evidence

- `cargo test --manifest-path src-tauri/Cargo.toml --lib ipc::agents`: **17 passed, 0 failed**
- `cargo test --manifest-path src-tauri/Cargo.toml daemon::server::`: **34 passed, 0 failed**
- `bun run --cwd ui test`: **136 test files, 1,362 passed, 0 failed**
