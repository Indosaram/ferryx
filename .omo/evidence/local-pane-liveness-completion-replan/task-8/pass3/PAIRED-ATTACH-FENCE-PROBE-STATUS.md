# Paired/remote attach fence — probe status (finding items 1 and 2)

Claim under test: *the candidate's attach fence makes REAL paired/remote attach impossible*, because
`DescribeSession` answers `incarnation: None` for a session owned by the paired runtime.

## 1. What I could NOT measure (stated plainly — no inference from source)

**I could not establish a real paired session in Task8 scope, so I did not observe the runtime
behavior.** Specifically, on both candidate-staged hosts (mac `I552267@100.65.239.35`, linux
`indo@100.91.254.71`):

| Needed to measure it | What I found |
| --- | --- |
| A live ferryx **app daemon** socket to probe | **none.** `find /tmp -maxdepth 3 -name daemon.sock` → empty on both hosts; `/tmp/rorca-*` contains only `legacy-*.transaction` files. The only running daemons are `ferryx-remote-helper daemon` processes (helper-side), not the app daemon that hosts the paired runtime. |
| A **paired host** to connect over the relay | not available in scope; no relay host/credentials in the Task8 staging. |
| An **existing test** that drives `handle_describe_session` with a paired id | **none.** `handle_describe_session` is `pub(super)` (`src-tauri/src/daemon/session_service.rs:2567`), and every existing caller passes a local or ssh-qualified id. `paired_host/proxy_tests.rs` installs proxies into a bare `TerminalService`, never a `DaemonServer`; `daemon/ssh_survival_tests.rs` drives a real `DaemonServer` but only with a `LegacyPeer` and plain ids (`"s"`). |

**Consequence:** the runtime claim is **UNVERIFIED**. I am not asserting it as measured.

## 2. Source-level A/B — this part IS decided (every line read directly)

| Fact | base `d82b35e4` | candidate `e57685ec` | Introduced by |
| --- | --- | --- | --- |
| `"Attach requires the persisted seven-field pane binding"` in `ipc/terminal.rs` | **0 occurrences** | 1 | `5464da0d` |
| `"Attach binding incarnation cannot be proven"` in `ipc/terminal.rs` | **0 occurrences** | 1 | `5464da0d` |
| paired branch answers `incarnation: None` (`session_service.rs`) | **present** (base line 2338+) | present (line 2604+) | **pre-existing** |

So at base the attach route carried **no binding requirement and no incarnation check at all**; both
are **candidate-introduced** by the composed candidate commit `5464da0d`
(`git log -S` on each string returns that commit). The *null incarnation* for paired-owned sessions
is **pre-existing** — the candidate did not create it, it added the check that consumes it.

Mechanism as read (not measured): `ipc/terminal.rs:2736` sets `target_session_id` to the
daemon-minted proxy id on the legacy paired route, and the paired-owned branch returns
`incarnation: None`, so `ipc/terminal.rs:2820`
(`description.incarnation.is_none() || description.incarnation != binding.incarnation`) rejects.

## 3. What would settle the runtime claim

Any one of these, in order of cheapness:

1. **The candidate's own repaired p13 test, once the tree compiles.** Its fixture installs a binding
   *and* teaches the mock daemon a `DescribeSession` arm returning a matching incarnation. If it
   **passes**, the fence demonstrably *accepts* a proxy session when the incarnation is provable,
   which isolates the real-world failure to the daemon answering `None` rather than to the fence's
   logic. If it **fails**, the fence's logic itself is implicated. **Blocked** by the E0308 below.
2. **A one-test probe in `src-tauri/src/daemon/`** that registers a paired descriptor into a
   `DaemonServer` and calls `handle_describe_session("daemon-session:…")`, asserting on
   `session.incarnation`. The verifier must not author this (test scope belongs to the owning lane).
3. **A real paired host over the relay** with a live app daemon — the only fully faithful measurement,
   and not available in this scope.

## 4. Blocking condition for everything above

`e57685ec` (and its parent `11c3a46a`) **do not compile the lib test target**:
`error[E0308]` at `src-tauri/src/ipc/tests.rs:309` — `incarnation` given `String`, expected
`Option<String>`. See `BLOCKER-11c3a46a-lib-test-does-not-compile.md`. Until that one line is fixed,
no test in this dispatch can run on any host, and item 3 of the finding (re-running the two
`ipc::tests` attach tests plus the paired-path test the repair worker adds) cannot start.
