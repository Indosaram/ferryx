# Agent screen-rule coverage — ported to full target set (2026-08-28)

Supersedes the gap analysis in `AGENT_STATE_COVERAGE_GAPS.md` §2/§4.

## Result

Screen-rule manifests went from **5 to 11**, rules from **23 to 51**. Every agent that
ships a brand icon now has either screen rules or the explicit extension protocol.

| manifest | rules | states |
| --- | --- | --- |
| antigravity | 3 | blocked, working |
| claude | 7 | blocked, idle, unknown, working |
| cline | 1 | blocked |
| codex | 6 | blocked, idle, working |
| copilot | 3 | blocked, working |
| cursor | 4 | blocked, working |
| gemini | 3 | blocked, idle, working |
| grok | 11 | blocked, idle, working |
| kimi | 6 | blocked, working |
| omo | 4 | blocked, idle, working |
| opencode | 3 | blocked, idle, working |

Verification: `cargo test --lib --features native-terminal` **334 passed, rc=0**;
`agent_detect` sub-suite 15 -> 20 tests.

## Where the rules came from

Not invented. Extracted from **herdr**'s embedded manifest corpus (`~/.local/bin/herdr`,
19 manifests) by locating each `id = "<agent>"` header in the raw binary and slicing bytes,
then validating each candidate with a real TOML parser. Byte-level extraction was required:
`strings` mangles the multi-byte glyphs the rules depend on (moon spinners, braille ranges,
box-drawing), which would have silently produced broken regexes.

Ported: antigravity (herdr id `agy`), cline, copilot, cursor, grok, kimi.

## The adaptation that mattered: herdr scopes, Ferryx does not

herdr identifies a pane's foreground agent and evaluates **only that agent's** manifest.
Ferryx evaluates **every manifest against every pane** and takes the highest priority. So a
rule that is safe upstream can be catastrophic here. Three classes had to be fixed:

1. **Catch-all defaults — dropped.** `cline/default_cline_working` used `regex = ['(?s).+']`
   and `grok/osc_title_working` used `regex = ['\S']` on the title. Upstream these mean
   "we already know it's this agent, so assume working". Imported literally they meant
   *any non-empty pane is a working agent*, and grok's fired at priority 1000, outranking
   every genuine rule. Both broke the repo's existing false-positive probes immediately.
2. **Priority band — renormalized.** grok shipped priorities 100-1300 while this repo uses
   90-400. Left alone, grok's rules would win every cross-agent tie. Remapped monotonically
   (1300->400 ... 1100->350), preserving intra-grok order.
3. **Generic single-term rules — dropped.** `pi/working_literal` was
   `contains = ["Working..."]`; the whole pi manifest was dropped since pi is already covered
   by the extension protocol. `cursor/stop_hint_working` was `contains = ["ctrl+c to stop"]`,
   which marks any long-running dev server (hugo, flask, older docker-compose) as a working
   agent — measured, then dropped. Cursor keeps `spinner_working` and
   `background_task_status_working`.

Also dropped: grok's two `osc_progress` rules, since this engine has no OSC-progress region.

## Real captures found a stale upstream rule

CLIs were run in a real PTY and their screens captured
(`docs/evidence/agent-screen-rules/real-cli-screens.txt`):

- **copilot 0.0.375** renders `Confirm with number keys or up/down keys and Enter, Cancel with Esc`.
  herdr's ported rules look for `esc to cancel` + `enter to select` and **match none of it** —
  the port alone would have missed copilot's actual permission dialog. Added
  `numeric_confirm_footer_blocked` from the measured text; it is the flagship regression test.
- **kimi**'s welcome banner correctly yields no state, so launching cannot fabricate activity.
- **grok** on PATH is a stub (`Error: grok not found in PATH`) and **cursor-agent** exits to
  auth, so those two manifests remain unverified against a live screen.

## Honest residue

- grok and cursor rules are herdr-derived but unverified against a real screen here.
- antigravity and cline are not installed; same caveat.
- `omo/approval_footer_blocked` (priority 400) shadows `antigravity/permission_prompt`
  (300) when both match, since evaluation is global. Harmless today (same Blocked verdict).
- Rules are a living corpus: vendor TUIs change, as copilot already proved.
