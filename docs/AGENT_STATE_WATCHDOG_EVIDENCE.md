# Agent State Watchdog — verification evidence

Captured output for the work on this branch. Commands were run in this worktree with its own
`CARGO_TARGET_DIR`; the live daemon was never signalled, restarted, or killed.

## Tests required by the scope

`cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::agent_state`

```
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 1031 filtered out
```

The three tests the scope names, by full path:

| Requirement | Test (full module path) | Result |
|---|---|---|
| Existing invariant must not regress | `agent_detect::independent_probe::probe_working_then_unclassifiable_screen_holds_working` | ok |
| Agent → shell transition releases state | `daemon::agent_state::tests::agent_to_shell_transition_releases_state` | ok |
| Quiet agent (no output, process alive) is NOT released | `daemon::agent_state::tests::quiet_agent_with_no_output_and_live_process_is_not_released` | ok |

Reproduce exactly:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --lib agent_detect::independent_probe
#   -> ok. 12 passed; 0 failed   (includes the invariant test above)
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  daemon::agent_state::tests::agent_to_shell_transition_releases_state -- --exact
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  daemon::agent_state::tests::quiet_agent_with_no_output_and_live_process_is_not_released -- --exact
```

**Two traps here, both of which produce a misleading result rather than an obvious error.**

First, `"absence of evidence must hold, never reset"` is an assertion *message*, not a test
name. It is the third argument of the `assert_eq!` at
`src-tauri/src/agent_detect/independent_probe.rs:95`, inside the test
`probe_working_then_unclassifiable_screen_holds_working` (declared `:83`). Filtering on the
quoted phrase finds no test.

Second, that file lives under **`src/agent_detect/`**, not `tests/`. It is a unit-test module
reached through `--lib`. Running `cargo test --test independent_probe` fails with
`error: no test target named 'independent_probe' in default-run packages` — which reads like
the test is missing when it is merely addressed the wrong way.

The invariant itself, verbatim from the test body: `detect` on a working screen yields
`Working`; feeding it subsequent *unclassifiable* output (`"building module 42/93"`,
`"compiled …store.ts"`) must still yield `Working`. Holding through noise is the behaviour
being protected.

## Mutation proof (failing-first substitute)

The implementation predates this evidence document, so a true pre-change RED could not be
recovered. Instead the guard was proven by forcing the exact forbidden behaviour and
capturing the assertion failing.

The HOLD invariant is one line — `src-tauri/src/terminal/foreground.rs:18`:

```rust
let Some(current) = current else { return false };   // no evidence -> do NOT release
```

Mutated so that absence of evidence releases state, which is precisely the downgrade the
scope forbids:

```diff
-  let Some(current) = current else { return false };
+  let Some(current) = current else { return true };
```

RED with the mutation present (exit 101):

```
test daemon::agent_state::tests::quiet_agent_with_no_output_and_live_process_is_not_released ... FAILED
thread '...quiet_agent_with_no_output_and_live_process_is_not_released' panicked at
    src/daemon/agent_state.rs:202:9:
assertion `left == right` failed
test result: FAILED. 10 passed; 1 failed; 1031 filtered out
```

GREEN after reverting (exit 0):

```
test result: ok. 11 passed; 0 failed; 1031 filtered out
```

Two things this establishes that a passing suite alone does not:

1. The quiet-agent test genuinely detects the forbidden behaviour. It is not vacuous.
2. The two new tests are complementary rather than overlapping. Under the mutation,
   `agent_to_shell_transition_releases_state` still **passed** — correctly, because a real
   Agent → Shell transition is unaffected by how absence is treated. One test proves release
   *happens* on process evidence; the other proves it *does not* on absence. Only the second
   can catch this regression, and it did.

The source file was restored from an in-memory copy and verified byte-identical; no
`git restore` / `git checkout --` was used, and the tracked tree was clean afterwards.

## Time-based downgrade: absent by construction

No periodic or elapsed-time downgrade path exists. Time primitives in the touched files
appear only after `#[cfg(test)]` (`agent_state.rs:133`, `foreground.rs:215`); the production
regions contain none. `git show --stat <commit> -- src-tauri/src/agent_detect/` is empty, so
the HOLD invariant at `agent_detect/engine.rs:90` is non-regressed by construction rather
than by assertion.

The 250 ms figure in `server.rs:1392` is a *sampling* interval, not a state timeout — it
controls how often process evidence is collected, never how long a state may live.

## Release-reason logging

`agent_state.rs:66` and `:82` emit a structured `tracing::info!` with three fields:

```
session_id=<id>  reason=manual_reset               previous_state=Some(Working)
session_id=<id>  reason=foreground_agent_to_shell  previous_state=Some(Working)
```

`previous_state` is the diagnostic field: if state is ever released when it should have held,
this line names what was dropped and under which reason.

## Not verified here

Desktop GUI end-to-end behaviour needs a real Tauri window, WGPU child surfaces and live
PTYs. It is not claimed. Manual steps are written up separately; the two decisive ones are
that an agent finishing quietly must clear the indicator, and an agent thinking quietly past
two minutes must not.

## Mutation proof: the C2 tests catch the scope document's forbidden behaviour

Earlier mutations in this document target `terminal::foreground`. Those runs used a
`terminal::foreground` filter, so they never demonstrated that the two C2-named tests in
`daemon::agent_state` are themselves sensitive to a real regression. This closes that.

The scope document's central prohibition is that absence of evidence must HOLD, never
release (section 1; time-based downgrade is forbidden because a quietly thinking agent
would be misread as finished). The matching mutation is to treat an evidence gap as exit
evidence.

Mutation in `src-tauri/src/terminal/foreground.rs::ProcessTransition::observe`:

    - let Some(current) = current else { return false };
    + let Some(current) = current else { return matches!(self.previous, Some(Foreground::Agent(_))) };

Command: `cargo test --manifest-path src-tauri/Cargo.toml --lib daemon::agent_state::tests -- --test-threads=1`

GREEN before: `test result: ok. 11 passed; 0 failed`
RED with mutation (exit 101): `test result: FAILED. 10 passed; 1 failed`

    daemon::agent_state::tests::quiet_agent_with_no_output_and_live_process_is_not_released FAILED
    assertion `left == right` failed
      left: Some(AgentState { session_id: "s1", state: "idle",    ... })
     right: Some(AgentState { session_id: "s1", state: "working", ... })

Exactly one test failed, and it is the right one. `agent_to_shell_transition_releases_state`
stayed green because its path (Agent -> Shell) never observes `None`; each test guards its
own property rather than overlapping. Reverted; source restored byte-identical; 11 passed.
