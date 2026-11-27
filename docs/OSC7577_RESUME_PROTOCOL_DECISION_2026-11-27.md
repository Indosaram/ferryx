# OSC 7577 program resume: decision record

Date: 2026-11-27
Status: not implemented. Ferryx keeps its provider-session resume path.
Scope: `src-tauri/src/terminal/resume_cwd.rs`, `src-tauri/src/terminal/shell.rs`,
`src-tauri/src/daemon/server.rs` (the herdr translation).

## What the protocol is

OSC 7577 is a terminal escape sequence a program emits to tell the terminal how to
start it again. Spec: `https://www.superlogical.com/rex/docs/build/program-resume.md`.

Grammar: `OSC 7577 ; <pair> (":" <pair>)* ":mac=" <hex> ST`, where each pair is
`key=value`. `pid`, `time` and `mac` are required and `mac` must be last. Optional
keys are `argv`, `cwd`, `clear=1` and `need`.

- `mac` is a plain SHA-256 over the body bytes before `:mac=` plus `":"` plus the
  terminal's `OSC7577_TOKEN`. It authenticates the report against a secret the
  terminal injects into the child environment, so a process that cannot read the
  token cannot forge a record.
- `time` is checked after `mac`; outside a 60 second window the report is dropped.
- There is no query and no reply. The terminal records the report and acts on it at
  its own restart.
- A record survives a crash or a shutdown and is discarded on a deliberate close.
  A mac-and-time-valid report that fails the rest of the grammar removes every
  record that owner had.
- Records are same-machine only, and at most one record is resumed in the
  foreground per pty.
- `argv` elements must not be turned into shell source text, and a `cwd` that no
  longer exists should not be resumed at all.

## Why Ferryx does not implement it now

1. Nothing we ship emits it. Measured 2026-11-27 with `grep -c OSC7577_TOKEN` over
   every agent binary installed on this Mac: claude, codex, mhc, omo, opencode,
   agy, kimi, copilot and cursor-agent all return 0. A listener today would parse
   zero reports.
2. The one agent family that reports to a terminal reports through herdr, not 7577.
   Ferryx already translates that envelope: `translate_herdr_report` in
   `src-tauri/src/daemon/server.rs` accepts `pane.report_agent` and carries
   `agent_session_id`, but `HERDR_REPORTING_AGENTS` is `["crush"]` and crush is not
   installed here, so that path also yields zero reports today. Herdr's own agent
   integration is a `pane report-agent` request with a `resume_argv` array.
3. 7577 restores a command line, not a process. The payload is argv plus cwd, so the
   best it can do is start the same program again. Ferryx already does more:
   `resolve_agent_resume_plan` and `resolve_agent_continue_plan` restart the exact
   provider session, and `resume_cwd.rs` recovers the original project directory
   from the agent's own transcript. Adopting 7577 would not add capability we lack;
   it would add a second, weaker source of the same two facts.
4. Accepting 7577 is a new execution surface. Any child process that can read
   `OSC7577_TOKEN` can nominate an argv the terminal runs later, at restore time,
   outside the pane that reported it. That is a real privilege and it needs the full
   set of gates: mac, time window, same uid, record ownership, and no shell
   interpretation of argv elements. It is not worth building while no producer
   exists.

## What we do instead

- Keep the provider-session path as the only resume mechanism.
- Keep the herdr `pane.report_agent` translation, which supplies provider session
  ids the resume builders already understand, and extend
  `HERDR_REPORTING_AGENTS` only after measuring a real reporter.
- Keep the transcript-derived cwd, which is what makes a resumed session land in the
  right project.

## When to revisit

Any one of these is enough to reopen the decision:

- An agent Ferryx supports starts emitting `OSC 7577` with `OSC7577_TOKEN` in its
  environment. Re-measure with the command block below before believing it.
- Restoring non-agent programs becomes a requirement. Provider-session resume cannot
  help vim, less, a REPL or a build; only an argv record can.
- A herdr reporter other than crush is measured sending `pane.report_agent`. The
  cheap step there is to accept its `resume_argv` in the existing translation, not
  to add 7577.

## Repeatable measurement

```sh
for b in claude codex mhc omo opencode agy kimi copilot cursor-agent crush; do
  p=$(command -v "$b" 2>/dev/null) || { echo "$b: not installed"; continue; }
  printf '%s: %s\n' "$b" "$(grep -c OSC7577_TOKEN "$p" 2>/dev/null || echo 0)"
done
```
