# Supervisor chat-first redesign: research report

Date: 2026-11-02. Scope: Ferryx Supervisor on desktop (Tauri) and phone (remote web client and iOS shell).
Question from the owner: rebuild the Supervisor screens from scratch so one chat-style surface manages every session.
Method: six research lanes (two codebase readers, three web lanes, one skeptic) plus a Lazyweb screen sweep. Journal, sources ledger (184 entries) and claim graph: `.omo/ulw-research/20261102T135705-supervisor-chat/`. Every URL below was fetched by a lane in this run. Lines marked [inferred] are our own reading.

## 1. Answer first

Build a **chat-first Supervisor with a permanent session rail**, not a chat-only one.

- The products that have already shipped this idea converged on the same shape: one coordinator conversation that does not write code, plus per-session threads, plus a side panel that groups sessions by state with "waiting on you" first. Claude Projects, Cursor Projects, Factory Workstreams, GitHub Copilot app Projects, Amp Puck, Devin managed Devins, Codex dots and Nimbalyst's Meta Agent all do this (section 4).
- The one documented hole in all of them: an approval raised by a session can only be answered inside that session's thread. Telling the coordinator "go ahead" does nothing (Claude Projects docs, Factory docs, Claude agent view docs). Ferryx can beat them by rendering every session's prompt card **inside the main conversation** and resolving it from there.
- Chat alone fails at "what is the state of my N agents right now": state scrolls away, silent stalls hide, and an LLM summary can report work that never happened (section 6). So session status must come from telemetry and live in a list that never scrolls away.
- Ferryx already has most of the per-session plumbing (prompt detection, answer by id with screen revision, send, stop, cross-host agent control, a fleet aggregator). It lacks the parts that make one conversation possible: a converse route outside Slack, a persisted supervisor transcript, LLM tools for sessions, a fleet read and event stream that clients can reach, and a last-message field (section 3).

The plan is `docs/plans/SUPERVISOR_AUTONOMY_PANEL_PLAN_2026-11-02.md`. Prototype: `docs/mockups/supervisor-chat/panel.html`, screenshots in `docs/mockups/supervisor-chat/shots/`. The plan supersedes this report's chat-first recommendation: the owner set the supervisor as an autonomous operator, so the UI is one panel of decisions, live recoveries, sessions and an action log.

## 2. Current Ferryx Supervisor

### 2.1 Desktop

- The sidebar has Worktrees / Inbox / Supervisor tabs (`ui/src/components/SidebarViewTabs.tsx:17-46`). The Supervisor tab is hidden unless advanced features are on (`SidebarView.tsx:97`). Supervisor and Inbox cannot be open at the same time (`Sidebar.tsx:158`).
- `SupervisorPanel.tsx` stacks two supervisor models on one screen: legacy lanes (`262-307`, LaneGoal `86-164`) above the unified goals section (`260`), then a Recent audit list (`309-325`).
- `UnifiedSupervisorSection.tsx`:
  - The "conversation" shows only the owner's messages, per project (`568-631`).
  - A composer with a separate "Done when" field (`225-259`).
  - Goal cards (`415-566`), an Adopt flow (`291-317`), and a memory panel showing raw JSON (`691-694`).
- Each pane has its own supervisor bar with six states: acting, held, paused, awaiting, left to you, watching (`SupervisorPaneBar.tsx:83-155`).
- The Inbox shows real question text through `cmd_agent_prompt_get`, but only for local panes (`AttentionInbox` / `AttentionPromptPanel` `420-571`). Its store is localStorage on each desktop (`notificationCenterPersistence.ts:116-160`).
- `UnifiedGoal` has no session id, host, cwd or question (`ui/src/lib/unifiedSupervisor.ts:29-46`; `crates/ferryx-hub/src/supervisor/request.rs:1505-1550`).
- Words a user meets by default: lane, Adopt, rev N, "Allowed: grant", MHC_* codes, Off/Supervised/Full, CLI commands.

### 2.2 Phone

- Phone home appears only for a signed-in account on a narrow layout (`ui/src/remote/RemoteApp.tsx:3069`).
- Three tabs, each built on a different model (`ui/src/remote/home/usePhoneNavigation.ts:11-20`, `MobileHomeTabBar.tsx:10-14`):
  - Supervisor: lane cards whose main button is Pause.
  - Inbox: rows answered through per-machine routes.
  - Worktrees: a project → worktree → session browser.
- The session screen's chat is the reference chat:
  - Native transcript readers for 14 agents, with scrollback as a fallback (`src-tauri/src/remote/history.rs:368-420`).
  - Prompt cards built by scraping the screen, 14 detector families (`prompts.rs:182-216, 788-821`).
  - Answers carry `screenRevision` (`server.rs:7227-7287`).
  - The client polls every 3 s (`RemoteApp.tsx:4011-4018, 4102-4127`).
- `isRunning` is true while a prompt is waiting (`RemoteApp.tsx:3357`), so a blocked session looks busy.
- Every machine is its own tunnel, event socket and snapshot (`ui/src/remote/machineSupervisorSources.ts:76-146, 202-303`), so the phone has no single view across machines.

### 2.3 Backend capabilities

| Capability | State | Where |
|---|---|---|
| List sessions on one machine with state and pending prompt | yes | `src-tauri/src/remote/agent_control_service.rs:102-167, 320-342`; route `server.rs:7861-7863` |
| Answer / send / stop a session | yes, per machine; cross-host via hub `AgentControl.execute` | `agent_control_service.rs:361-427, 1024-1109`; `crates/ferryx-hub/src/agent_control.rs:66-91` |
| Fleet across machines | yes in the hub, MCP only, no HTTP route | `crates/ferryx-hub/src/fleet.rs:11-60`; `mcp.rs:103-110, 299-328` |
| Cross-machine live session with question text and last assistant line | yes, omo-probe path only | `crates/ferryx-hub/src/sessions/overview.rs:26-39, 84-101` |
| Last message per session in agent-control | no | `agent_control_service.rs:145-160` |
| Supervisor LLM conversation | Slack/Discord only; two tools `Submit`, `Status` | `meta/mhc_supervisor.rs:343-356, 1961-2023`; `slack_supervisor.rs:75` |
| `message` through the app's request route | returns `Conversational` and does no work | `supervisor/admission.rs:365-393` |
| Persisted conversation | owner requests only ("Engine replies are not persisted here") | `supervisor/request.rs:1590-1638` |
| Dispatch to a session | protocol variant `Submit{host_id, session_id}` with no Tauri command | `supervisor/protocol.rs:563-601` |
| Agent control | off on a fresh install | `src-tauri/src/agent_control/policy.rs:1-10` |
| Actionable notice cards | Slack/Discord `ApprovalCard{session_id, prompt_id, screen_revision, options}` | `crates/ferryx-hub/src/sessions/watch.rs:177-226` |

## 3. Gaps that block one conversation over every session

1. No converse route for the app. `converse_in_turn`'s only production caller is Slack (`slack_supervisor.rs:75`).
2. No persisted supervisor transcript to render (`request.rs:1590-1592`).
3. The LLM cannot list, read, answer, send to or stop sessions (`mhc_supervisor.rs:343-356`), even though the MCP surface has those operations (`mcp.rs:103-140`).
4. No fleet read reachable from desktop or phone (`fleet.rs` has no HTTP route).
5. No last-message text per session (`agent_control_service.rs:145-160`).
6. No aggregate event stream. It is one socket per machine, and prompt bodies are left out on purpose (`src-tauri/src/agent_control/events.rs:1-7, 93-121`).
7. No session handle for `@mention` addressing.
8. Agent control is off by default and the phone UI never calls the agent-control routes (`policy.rs:1-10`).
9. The desktop Inbox is per-desktop state, so two desktops disagree (`notificationCenterPersistence.ts:116-160`).

## 4. Comparable products

Each entry: what it is, what to take, what to avoid. Sources were fetched in this run.

1. **Claude Projects and agent view (Anthropic).** Sources: https://code.claude.com/docs/en/claude-projects.md, https://code.claude.com/docs/en/agent-view.md, https://code.claude.com/docs/en/cross-session-messaging.md.
   - What it is: one coordinator conversation that starts a thread per task and shows thread cards inline. The Overview groups threads as Ready for review / Waiting on you / Working / Landing / Idle / Resolved. The agent view adds Space to peek and reply and → to attach. Sessions can be addressed with `@session-name` typeahead, and Claude asks when the name is ambiguous.
   - Take: the state groups, peek-before-attach, `@session` with the ask-when-ambiguous rule, and reporting rules written in plain language ("only post when something finishes or is blocked").
   - Avoid: the documented flaw "When a thread needs your approval, the prompt is inside that thread… Telling Claude in the project conversation to go ahead doesn't reach it."
2. **Claude Dispatch / mobile Code tab.** Sources: https://support.claude.com/en/articles/13947068, https://code.claude.com/docs/en/mobile.md, https://code.claude.com/docs/en/remote-control.md.
   - What it is: a single persistent thread that "messages you the outcome… rather than showing you every step", with a push when a task needs your go-ahead.
   - Take: outcome messages instead of step streams.
   - Avoid: a waiting question that never renders on the phone, so a waiting turn looks finished (https://github.com/anthropics/claude-code/issues/106279), and a question card taking over the input box (https://github.com/anthropics/claude-code/issues/103619).
3. **Cursor 3 Agents Window, Projects and iOS.** Sources: https://cursor.com/blog/cursor-3, https://cursor.com/docs/agent/projects.md, https://cursor.com/docs/cloud-agent/mobile.md.
   - What it is: every local and cloud agent appears in one sidebar. A Project is directed "by chatting with its coordinator agent. The coordinator doesn't write code itself." iOS has an inbox, Live Activities for up to 8 agents, and full diffs.
   - Take: the coordinator that doesn't code, and the lock-screen status.
   - Avoid: approvals that can only be given on the laptop. App Store reviewers call this "useless".
4. **OpenAI Codex app, Activity and pets.** Sources: https://openai.com/index/introducing-the-codex-app/, https://learn.chatgpt.com/docs/notifications.md, https://learn.chatgpt.com/docs/pets.md, https://developers.openai.com/codex/mobile.md.
   - What it is: an Activity view of chats that are unread, running or waiting for your response. A floating status shows Running / Needs input / Ready / Blocked in priority order. Mobile lists tasks across hosts.
   - Take: a priority-ordered attention summary and a task list that spans hosts.
   - Avoid: timed questions that auto-continue (https://github.com/openai/codex/issues/29702) and inline questions that never raise "Needs input" (https://github.com/openai/codex/issues/42433).
5. **Factory Workstreams.** Sources: https://docs.factory.com/workstreams/overview.md, https://docs.factory.com/workstreams/steer.md.
   - What it is: an orchestrator chat in the centre, with a right-hand Agents tab grouped Waiting on you / Working / Idle, each row showing its latest activity and PRs.
   - Take: the three-column layout (our desktop mockup uses it).
   - Avoid: "Agents don't see your conversation with the orchestrator", and the advice to tell the orchestrator after steering an agent directly. Ferryx should keep one shared record instead.
6. **GitHub Copilot mission control and app Projects.** Sources: https://github.blog/news-insights/company-news/welcome-home-agents/, https://github.blog/changelog/2025-10-28-a-mission-control-to-assign-steer-and-track-copilot-coding-agent-tasks/, and the docs.github.com articles listed in the ledger.
   - What it is: "a single command center to assign, steer, and track". The Overview groups Needs input / Open / Working / Done, and each session has a "Steer this session" box.
   - Take: steering from a diff comment, and the Needs input group label.
   - Avoid: steering that silently doesn't work for third-party agents.
7. **Amp Puck.** Source: https://ampcode.com/docs/markdown/puck, https://ampcode.com/docs/markdown/threads.
   - What it is: a personal assistant overlay that can "start agents, monitor them, send messages to them". It is context-aware of the current page, works by voice, and has saved prompts (e.g. send `ship` to a thread). A snooze ends when the thread needs approval.
   - Take: a supervisor overlay that knows the current page, saved prompts, and snooze-until-needed.
   - Avoid: nothing specific found.
8. **Devin (Slack and Agent Command Center).** Sources: https://docs.devin.ai/integrations/slack, https://docs.devin.ai/desktop/agent-command-center.md, https://docs.devin.ai/work-with-devin/advanced-capabilities.md.
   - What it is: a thread per session, which can be promoted to a channel with a status chip. Inside a session's space no mention is needed. `mute` and `aside` are supported. A coordinator manages child Devins and is woken when a child needs input.
   - Take: no mention needed inside a session, plus aside and mute.
   - Avoid: sessions locked read-only while their agent runs.
9. **Dori (Sisyphus Labs).** Source: https://sisyphuslabs.ai.
   - What it is: "One living status message per job, edited in place". Pings only after the merge. Decision prompts come with an id and options A/B/C.
   - Take: status cards edited in place and decision prompts with ids.
   - Avoid: none published.
10. **LangChain Agent Inbox / ambient agents.** Sources: https://blog.langchain.com/introducing-ambient-agents/, https://github.com/langchain-ai/agent-inbox.
    - What it is: LangChain moved from Slack pings to an inbox because "it's easy to lose track of all the notifications". Interrupts come in three kinds: notify, question, review.
    - Take: typed interrupts.
    - Avoid: a flat notification channel.
11. **Happy (slopus/happy).** Sources: https://github.com/slopus/happy (README, source at 67b57b8d), https://github.com/slopus/happy/issues/435, https://github.com/slopus/happy/issues/1908.
    - What it is: a phone session list with status lines ("permission required", "waiting for your answer"), a permission footer with scoped allow (once / session / all edits), and a voice agent whose tools are `sendMessageToSession` and `processPermissionRequest`.
    - Take: scoped allow buttons and the voice-to-session routing.
    - Avoid: generic pushes with no deep link (#435), no Allow/Deny on the push itself (#1908), and an "Inbox" that is not agent attention.
12. **Paseo.** Sources: https://paseo.sh, https://github.com/getpaseo/paseo/blob/555818f5f752c66fc324fbc3a220475159163982/docs/mobile-panels.md, https://github.com/getpaseo/paseo/blob/555818f5f752c66fc324fbc3a220475159163982/packages/app/src/utils/agent-attention.ts#L18-L60.
    - What it is: a workspace list grouped Ready to review / Working / Done. An attention picker (permission > error > finished, oldest first) lands you on the agent that needs you most. Pushes resolve the target at tap time.
    - Take: the attention picker and tap-time resolution.
    - Avoid: a swipe that jumps to a session on another host (https://github.com/getpaseo/paseo/issues/1321), and pushes for the chat you are reading (https://github.com/getpaseo/paseo/issues/8069).
13. **Ovis and Collie (herdr web clients).** Sources: https://github.com/devswha/ovis/blob/5456638ff5dfc1851bc7566cb641164daf7937eb/docs/guide.md, https://github.com/AltanS/collie.
    - What it is: "coding agents on your computer as chats you answer with a click or a tap". Prompt cards are re-checked as still current before answering. Sending while the agent works queues a next-turn message with "Send now". No push while any window is in the foreground. Collie leads its status board with what needs your input.
    - Take: the freshness check (Ferryx already has `screenRevision`), the queued-send receipt, and presence suppression.
    - Avoid: stale plan cards (https://github.com/devswha/ovis/issues/2589).
14. **T3 Code / OmO Desktop.** Source: https://t3.codes, and the t3code repository at 701e13ce (docs/user/thread-sidebar.md, docs/orchestration-v2/orchestrator-mcp-server.md).
    - What it is: sidebar sections pinned / active / working / snoozed / settled. A thread returns to the top when it finishes, fails or needs approval. An app-owned MCP server lets any agent list, read, send to and interrupt threads, with read-only / supervised / full access.
    - Take: settle and snooze shelves, and scoped orchestrator access.
    - Avoid: too many status pill kinds (nine).
15. **Conductor.** Sources: https://www.conductor.build/, https://www.conductor.build/changelog, https://www.conductor.build/docs/api/mcp.
    - What it is: a status-grouped sidebar. Queued messages appear lighter until read. The orchestrator is an external MCP.
    - Take: the faded queued message.
    - Avoid: background processes fooling the waiting-state detection ("Waiting for task").
16. **Paradev (formerly Superset).** Source: https://docs.paradev.com/llms-full.txt.
    - What it is: a board with Idle / Working / Needs attention / Needs review columns. Status comes from agent hooks, and a "Clear Status" escape hatch exists because hook state sticks. The orchestrate skill reads and writes worker terminals.
    - Take: the colour semantics (yellow = permission, red = failed, green = review), an explicit unknown state, and a manual clear.
    - Avoid: assuming every agent emits a waiting signal.
17. **Warp agent notifications.** Source: https://docs.warp.dev/_llms-txt/agents.txt.
    - What it is: Complete / Request / Error notifications, a mailbox with filters and keyboard navigation, and an approval card before an orchestrator starts children.
    - Take: three notification kinds and confirming before fan-out.
    - Avoid: third-party agents missing from the management panel.
18. **Zed terminal threads.** Source: https://zed.dev/docs/ai/terminal-threads.md.
    - What it is: a BEL from an unfocused terminal thread notifies like an agent finishing.
    - Take: BEL as a cheap needs-input signal for raw PTY agents that have no detector [inferred].
19. **Nimbalyst.** Sources: https://nimbalyst.com, https://nimbalyst.com/docs/session-management/agent-window-and-session-management.md.
    - What it is: an attention list of Awaiting input / Running / Unread ("Mark all read… does not approve requests"). A menu-bar session fleet. A Meta Agent with tools such as create, send_prompt, respond_to_prompt and get_session_status.
    - Take: the tool set, which almost matches our G3.
    - Avoid: mark-read ever being confused with answering.

Also checked, with nothing new to take or no longer shipping:
- Jules: shutting down 2026-12-31; it auto-approves plans on a timer.
- Omnara: pivoted.
- Vibe Kanban: the company shut down.
- Terragon: shut down.
- Sculptor, JetBrains Air, Termius, Blink, VibeTunnel, Rex (iOS not public), Kibitz (`/N` targeting).

The Lazyweb screen sweep (`wave-1-lazyweb.md`) covered mobile chat and inbox screens. It was used for layout only; its images are not redistributed.

## 5. Patterns

| Pattern | Seen in | Ferryx decision |
|---|---|---|
| State-grouped list, needs-you first | Claude, Factory, Copilot, Paseo, Collie, t3, Paradev, Codex | Rail groups Needs you / Working / Finished / Failed (plan D5) |
| Coordinator chat that doesn't code, with per-session threads | Claude Projects, Cursor, Factory, Copilot, Amp, Devin, Nimbalyst | Supervisor conversation + session view (D1) |
| Approvals answerable only inside the session | Claude, Factory, agent view | Rejected: prompt cards answerable in the main conversation, first answer wins (D3) |
| One living card per job, edited in place | Dori, Devin status chip, Slack agent guidance | Event cards update in place; only needs-you / failed / finished add messages (D2) |
| `@session` addressing; no mention needed inside a session | Claude, Devin, Cursor, Kibitz | D4 |
| Queued send with visible delivery state | Ovis, Conductor, Kibitz, Copilot | Delivery receipt card |
| Peek before attach | Claude agent view, Nimbalyst, Warp | Desktop right peek; phone session sheet |
| Push only for needs-you/failed, deep-linked, suppressed when watching | Ovis, Paseo, Codex, Cursor | D7 |
| Status from hooks or telemetry, with an unknown state | Paradev, Warp, Zed | Rail never trusts LLM text; unknown shows as "status unknown" |
| Snooze / settle shelves | Amp, t3, Claude auto-archive | Later phase: Finished auto-collapses after 24 h [inferred] |
| Timed auto-continue on questions | Jules, Codex (complaints) | Must not have |

## 6. Skeptic: where chat-only fails

From the A6 lane, with its sources:
1. **State scrolls away.**
   - Chat "can't simply 'stick around'" (https://thesephist.com/posts/latent/).
   - "Chat reduces you to memory and hope" (https://danieldelaney.net/chat/).
   - Situation awareness starts with perceiving every element's state (https://doi.org/10.1518/001872095779049543).
   - Answer: the always-visible rail.
2. **Silent stalls.**
   - Agents block on permission prompts and nobody sees it (https://news.ycombinator.com/item?id=47310039).
   - Subagents have no timeout or progress (https://github.com/anthropics/claude-code/issues/61405).
   - Answer: Needs you comes from the detector, not narration, plus time-in-state.
3. **A narrating orchestrator invents work.**
   - Fabricated dispatches (https://github.com/anthropics/claude-code/issues/61167).
   - Bainbridge, "Ironies of Automation" (https://ckrybus.com/static/papers/Bainbridge_1983_Automatica.pdf).
   - Answer: supervisor claims link to events; the rail ignores LLM text.
4. **Diffs don't fit in chat.**
   - https://wattenberger.com/thoughts/boo-chatbots
   - https://www.nngroup.com/articles/accordion-editing-apple-picking/
   - Answer: a Changes tab per session.
5. **Notification and approval fatigue.**
   - https://blog.langchain.com/introducing-ambient-agents/
   - https://news.ycombinator.com/item?id=48319511
   - Answer: typed interrupts and push limited to needs-you/failed.
6. **Chat pulls attention off the primary task.** Cummings 2004, Tomahawk supervision (https://doi.org/10.1089/cpb.2004.7.653). Answer: progress never creates a message.
7. **Switching cost.** Cummings & Mitchell 2008 capacity model (https://doi.org/10.1109/tsmca.2007.914757). Answer: every card carries a re-entry line (goal, last action, blocker).
8. **Typing on a phone is slow.** https://julian.digital/2025/03/27/the-case-against-conversational-interfaces/. Answer: one-tap options on every card.
9. **A blank box hides what is possible.** https://www.nngroup.com/articles/prompt-controls-genai/. Answer: composer chips `To:` / `@` / `+ New task`.
10. **Layouts generated per request break the mental model.** https://www.nngroup.com/articles/generative-ui/. Answer: fixed card types; no generated UI.

Where chat clearly wins, and why we keep it:
- Stating intent (https://www.nngroup.com/articles/ai-paradigm/).
- Steering one agent mid-run.
- Asking about the fleet from the phone (https://simonwillison.net/2025/Oct/5/parallel-coding-agents/).

Limit: no controlled study compares chat-only and dashboard supervision for coding agents. The human-factors evidence comes from UAV and process control and is applied here by analogy.

## 7. Recommended design (summary of the plan)

- **Desktop:**
  - Left rail: computer chips; Needs you / Working / Finished groups; offline computers collapsed.
  - Centre: Supervisor conversation with a digest, inline prompt cards, `@mentions`, finished recaps, and a composer with `To:` / `@` / `+ New task`.
  - Right: peek of the selected session (Terminal / Chat / Changes).
  - Opened from a top-level Supervisor entry; no longer gated behind advanced features.
- **Phone:**
  - No tab bar. The conversation is the home screen, with a needs-you-first strip of session pills and a list button for the full rail.
  - A session opens as a sheet with Chat / Terminal / Changes.
  - The native gestures from f78ccb80 (edge-swipe back, sheet dismiss, pull-to-refresh) stay.
- **Words:** Needs you, Working, Finished, Failed, Idle. No lane, goal contract, rev, grant, Adopt, engine codes or CLI text by default.
- **Backend:**
  - converse route with a persisted transcript;
  - session tools for the LLM;
  - fleet sessions read with `lastAssistantLine`;
  - owner-scoped fleet event channel;
  - `sessionRef` handles;
  - a dispatch command;
  - one-step agent-control enable for the owner's own devices.

## 8. Mockups

- `docs/mockups/supervisor-chat/panel.html` → `shots/desktop-p1.png` (1440 x 900) and `shots/phone-p1.png` (390 x 844).
- Tokens come from `ui/src/index.css`, so the mockups use the app's real palette.

## 9. Open items and uncertainty

- Supervisor host when no hub runs: the plan defaults to a single setup card (D9).
- Flick dismiss on iPhone is still waiting for a real-finger check from the 2026-11-02 gesture QA. It is unrelated to this redesign.
- Sources that were not reachable (X, Reddit, openai.com launch posts, Cursor's signed-in agents page) are recorded as dead ends in `expansion-log.md`.
