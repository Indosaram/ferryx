# Ferryx site search priorities — 2026-09-19

Evidence-backed content priorities for `https://ferryx.dev`.

**Correction notice, added 2026-09-19 after the site edits shipped.** Two claims in the
original draft of this report were wrong and have been corrected in place below. First, the
report described Conductor as a cloud-microVM product and treated "local vs cloud" as the axis
Conductor's page should argue; Conductor is in fact a **macOS desktop app** whose free tier runs
agents locally in git worktrees under `~/conductor/workspaces/`, with Conductor Cloud as a paid
add-on on Pro and above. No unrelated products were conflated (conductor.build, Conductor Cloud,
and the Vercel sandboxes behind it are one product line from Melty Labs), but the emphasis was
backwards. Second, the report did not catch that **Warp open-sourced its client**
(`warpdotdev/warp`, AGPL-3.0 with MIT for the `warpui`/`warpui_core` crates), which made the
existing `/compare/warp/` page's repeated "closed source" claim false. Both are fixed on the
live pages. Sources and dates are in the evidence index.

## Method and its limits

- **What was collected.** 34 live search-engine result pages and 12 primary vendor/community
  pages, fetched on 2026-09-19 (KST) by driving an already-running Chrome over the local CDP
  endpoint (`127.0.0.1:9455`, the Aside browser profile). Each SERP's rendered text and every
  extracted result URL were saved before analysis.
- **Provider failures encountered.** `web_search` native provider returned 401 and the DDG
  fetch path failed; `duckduckgo.com/html`, `search.brave.com` (bot check), and
  `startpage.com` (proof-of-work interstitial) all refused automation. Plain `curl` to Google
  returned a consent/JS shell with zero results. Bing returned locale-corrupted results for
  English queries from this Korean IP (a query for `monitor claude code from phone` returned
  Korean monitor-hardware retailers), so Bing is excluded from the evidence base except for the
  `site:ferryx.dev` check. Direct-to-Google-via-CDP on port 9354 hit "unusual traffic"; the
  9455 profile did not. Results therefore reflect **Google, en/US locale, personalized to a
  Seoul-based signed-in profile** — treat ranking order as indicative, not canonical.
- **No search volumes, no difficulty scores.** No keyword tool was used and none is claimed.
  Every "demand" statement below is a **demand proxy**: an observable artifact on the SERP
  (a forum thread with a stated comment count, the existence of an AI Overview, a dedicated
  vendor comparison page competing for the term). A demand proxy is not a measured search
  volume and must not be reported as one. Ranking difficulty is likewise **not estimated**;
  what is recorded instead is the observed *shape* of the result set (docs-dominated,
  forum-dominated, or vendor-comparison-dominated), which is checkable.

## Where Ferryx stands in search today (observed)

| Check | Observation |
| :--- | :--- |
| `site:ferryx.dev` (Google) | 1 result: the homepage only. |
| `site:ferryx.dev/compare/` | 5 results: `/compare/`, `/compare/crystal/`, `/compare/warp/`, `/compare/conductor/`, `/compare/tmux-git-worktree/`. `/compare/wave-terminal/` and `/compare/ghostty/` did not appear. |
| `site:ferryx.dev/use-cases/` | 3 results: all three use-case pages indexed. `git-worktree-workflow` was listed under the **`http://`** scheme. |
| `site:ferryx.dev/docs/` | 3 results: introduction, architecture, shortcuts. Google reported "omitted some entries very similar to the 3 already displayed". |
| `site:ferryx.dev/docs/facts/` | "did not match any documents" in this search; this does not prove the URL is absent from Google's index. Use Search Console URL Inspection to establish that. |
| `site:ferryx.dev/privacy/` | 1 result, titled "Ferryx Privacy Policy" (current page title is "Ferryx Privacy Declaration"). |
| Brand query `ferryx` | Page 1 is entirely **Ferryx Ltd, the UK probiotics company** (ferryx.com, LinkedIn, EIT Food), plus an AI Overview about probiotics. ferryx.dev does not appear on page 1. |
| `ferryx vs conductor` | The GitHub README ranks; ferryx.dev does not. Remaining results are about Netflix Conductor and unrelated "conductor" senses. |
| Any of the 17 non-brand queries tested | **Zero ferryx.dev appearances.** Confirmed by case-insensitive scan of every saved SERP. |

Two indexing hygiene problems are visible in the snippets, both independent of content quality:

1. **Stale cached copy.** Google's snippet for `/compare/` reads *"Ferryx is an open source
   desktop terminal… It's MIT licensed, written in Rust on Tauri v2, and currently at v0.1.0"*,
   and the `/compare/tmux-git-worktree/` snippet says *"Ferryx is an MIT licensed desktop app"*.
   The live pages say **SUL-1.0, source-available** (verified by fetching all three pages:
   `/compare/` and `/compare/tmux-git-worktree/` each contain "SUL-1.0" and "source-available",
   with zero occurrences of "MIT licensed"). Google is serving a licensing claim the project
   has since corrected. The same applies to the homepage title variant
   *"Ferryx — Ultra-Lightweight Rust Native Ghostty AI Workspace"* surfaced under the
   `www.` host, which is not the current title.
2. **Scheme and host duplication.** `http://ferryx.dev/…` returns **200 directly** (no redirect
   to HTTPS) and Google has indexed at least one page under `http://`. `www.ferryx.dev` does
   301 correctly to the apex. Canonical tags are correct (`https://ferryx.dev/...`), so this is
   recoverable, but the `http://` origin is currently a live duplicate.

## Cluster 1 — Parallel Claude Code in git worktrees

**Representative queries observed:** `claude code parallel git worktrees`,
`run multiple claude code sessions in parallel`, `parallel ai coding agents git worktree`,
`best tool to run multiple claude code agents`.

**Intent:** overwhelmingly *informational / how-to*, shading into *tool discovery* on the
"best tool" variant. Users want the command and the isolation model first, a tool second.

**Observed results (Google, 2026-09-19):**

- https://code.claude.com/docs/en/worktrees — position 1 on the head query.
- https://code.claude.com/docs/en/common-workflows
- https://code.claude.com/docs/en/agents
- https://github.com/spillwavesolutions/parallel-worktrees
- https://incident.io/blog/shipping-faster-with-claude-code-and-git-worktrees
- https://www.mindstudio.ai/blog/claude-code-git-worktree-parallel-branches (MindStudio holds
  several distinct URLs across this cluster)
- https://www.trigger.dev/blog/parallel-agents-gitbutler ("We ditched worktrees for Claude Code")
- https://www.codeagentswarm.com/en/guides/run-multiple-claude-code-sessions
- https://matrix-os.com/blog/run-multiple-claude-code-sessions
- https://nimbalyst.com/blog/best-tools-for-running-parallel-… (title: "Best Tools for Parallel
  AI Coding Agents (2026)")
- https://www.reddit.com/r/ClaudeAI/comments/1q6u7xz/how_do_people_run_multiple_claude_code_sessions/
- https://www.reddit.com/r/ClaudeCode/comments/1ru3i4q/how_i_run_56_claude_code_agents_in_parallel/

**Demand proxies (not volume):** the Reddit thread "How do people run multiple Claude Code
sessions?" shows *70+ comments / 72 answers*; "Run 2 (or even more) instances of Claude Code"
shows *77 answers*; Google renders an AI Overview plus a Videos carousel on every query in this
cluster. Multiple commercial vendors maintain dedicated pages for the exact phrase, which is
itself evidence someone judged the term worth owning.

**Result-set shape:** vendor-docs-dominated at the top (`code.claude.com` outranks everything),
with a long tail of SEO blogs and Reddit. Ferryx will not displace Anthropic's own docs.

**Critical product fact the page was missing:** Claude Code ships **native worktree
management**. Per https://code.claude.com/docs/en/worktrees (read 2026-09-19):
`claude --worktree <name>` / `-w` creates the worktree under `.claude/worktrees/<name>/` on a
branch `worktree-<name>`; Claude cleans it up on exit; `EnterWorktree`/`ExitWorktree` tools
exist; subagents can carry `isolation: worktree`; and Claude Code **enforces isolation** by
blocking edits, command working directories, git redirects, and unverifiable command shapes that
target the main checkout. The Ferryx page `/use-cases/parallel-ai-agents/` framed the alternative
to Ferryx as "git worktree + tmux + a couple of scripts" and never mentioned `claude -w`.
**Fixed 2026-09-19:** the page now leads with what Claude Code and Codex do natively, tells
single-vendor users to use the vendor flag, and argues the cross-vendor case instead.

**Realistic Ferryx differentiation here:** not "we do worktrees" — Claude Code does that
itself now. The defensible claims are (a) **one convention across every agent**, since
`claude -w` only helps Claude Code while Ferryx's `.orca-worktrees/wt-<slug>` +
`orca/<workspace-id>/<slug>` applies to any CLI agent, including Codex, which uses its own
`$CODEX_HOME/worktrees` detached-HEAD scheme; (b) **status detection across eleven agents**;
(c) **the daemon owning the PTYs**, which no worktree flag addresses.

**Existing page mapping:** `/use-cases/parallel-ai-agents/` (primary),
`/compare/tmux-git-worktree/` (secondary).

## Cluster 2 — Codex worktree workflow

**Representative query:** `codex git worktree workflow`.

**Intent:** *how-to with a strong troubleshooting edge* — several top results are complaints
about Codex's worktree behaviour rather than tutorials.

**Observed results:**

- https://learn.chatgpt.com/docs/environments/git-worktrees — the official Codex worktrees doc.
- https://www.reddit.com/r/codex/comments/1sc7g2x/how_are_you_actually_running_codex_at_scale/
- "How do you actually make Codex use existing git worktree…" (r/codex, *8 comments / 8 answers*),
  "Confusion about Git worktrees" (r/codex), "How are you using multiple agents and worktrees"
  (*18 answers*)
- https://community.openai.com/t/reviewing-changes-from-multiple-git-repos-worktrees-per-project/1384405
- https://github.com/openai/codex/issues/13120 ("Git worktrees with codex", which explicitly
  cites `claude --worktree` as prior art)
- https://www.chatprd.ai/ (How I AI → Workflows)
- https://www.qlerebours.dev/blog/use-git-worktree-with-ai-assistants
- https://jessepeplinski.com/blog/how-i-use-worktrees-with-codex/
- https://www.verdent.ai/guides/codex-app-worktrees-… , https://blog.4sapi.com/blog/codex-git-worktree-parallel-dev

**Demand proxies:** a discussion-and-forums block is promoted onto page 1; the top organic
non-doc result is a frustration thread; four separate YouTube tutorials are carouselled
(lustoykov, Thiago Temple, PeterCoding, Augment Code).

**Result-set shape:** official doc + forum complaints. Thin on well-structured third-party
explainers — this is the **least saturated** of the five clusters.

**Primary facts worth citing accurately** (from https://learn.chatgpt.com/docs/environments/git-worktrees,
fetched 2026-09-19): Codex creates worktrees in `$CODEX_HOME/worktrees` in **detached HEAD**;
"Handoff" moves a chat between Local and Worktree; Codex keeps the most recent **15**
Codex-managed worktrees and auto-deletes on chat archive; a branch created in a worktree cannot
be checked out locally at the same time (`fatal: '<branch>' is already used by worktree at …`).

**Realistic Ferryx differentiation:** Ferryx does not need to beat the Codex app at Codex; it
needs to be the answer to *"I want Codex and Claude Code in the same window, on the same repo,
without the branch-checkout collision"*. That is exactly the pain in the Codex FAQ and in the
`run codex and claude code side by side same repo` SERP (r/ClaudeCode thread, *40+ comments /
40 answers*, top answer naming Herdr/Orca/Buzz).

**Existing page mapping:** **none.** `/use-cases/parallel-ai-agents/` mentions Codex exactly
twice, in passing lists. There is no Codex-specific page and no page about running two vendors'
agents on one repository. This is the clearest content gap in the set.

## Cluster 3 — Watching / steering agents from a phone

**Representative queries:** `monitor claude code from phone`,
`check ai agent progress from phone terminal`, `keep claude code running after closing terminal`,
`keep coding agent running when you close the app`.

**Observed results:**

- https://code.claude.com/docs/en/remote-control and https://code.claude.com/docs/en/mobile
  — positions 1 and 2.
- https://docs.warp.dev/agents/cli-agents/remote-control/
- https://agentsroom.dev/code-from-phone , https://agentsroom.dev/features/mobile-desktop-sync
- https://www.builder.io/blog/claude-code-mobile-phone
- https://apps.apple.com/us/app/quicktui-ai-agent-terminal/id6761338192
- https://www.reddit.com/r/ClaudeCode/comments/1skn2tm/whats_the_best_setup_for_checking_claude_code/
- https://www.reddit.com/r/termux/comments/1sacnvj/the_ultimate_mobile_ai_agent_terminal/
- https://www.reddit.com/r/ProductivityApps/comments/1vcqc8q/i_built_a_mobile_control_center_for_coding_agents/
- https://codeongrass.com/blog/how-to-keep-claude-code-running-after-terminal-close/ ,
  https://cdmckay.org/how-to-keep-claude-code-running-across-ssh-disconnects/ ,
  https://hivra.cloud/blog/keep-claude-code-running-24-7 ,
  https://matrix-os.com/blog/keep-claude-code-running-after-laptop-closes
- https://github.com/herdrdev/herdr ("detach without stopping work — herdr keeps terminals
  running in a background server when you close the client")

**Demand proxies:** "Running Claude Code from your phone — it actually works" (*40+ comments*);
"Using Claude code on your phone?" (*108 posts*); "Is there a decent way to access claude code
terminal" (*38 posts*); "How do you keep Claude Code running 24/7" (*140 answers*). This is the
highest-engagement cluster by forum-thread size in the sample.

**This cluster changed under Ferryx's feet.** Anthropic's Remote Control
(https://code.claude.com/docs/en/remote-control) now does the phone job natively: a session URL
with a QR code from `claude remote-control`, sync across terminal/browser/phone, automatic
reconnect after laptop sleep, push notifications, and a `--spawn worktree` flag that gives each
on-demand remote session its own git worktree. Warp ships an equivalent cloud-published Remote
Control. Ferryx's `/use-cases/remote-terminal-access/` framed the competition as "plain SSH, tmux
over SSH, or a VPN/Tailscale", a landscape one release cycle out of date that omitted both
first-party options. **Fixed 2026-09-19.**

**Realistic Ferryx differentiation, stated honestly:** Remote Control requires a
claude.ai Pro/Max/Team/Enterprise plan, refuses API-key auth, and is unavailable on
Bedrock / Google Agent Platform / Microsoft Foundry or behind a custom `ANTHROPIC_BASE_URL`
(all stated in Anthropic's own requirements section). While connected, the transcript is stored
on Anthropic servers, so Zero Data Retention organisations can't enable it, and the local
`claude` process has to keep running. It is also Claude-only. Ferryx's gateway is
**agent-agnostic, self-hosted, subscription-free, and works for any CLI process in a pane**,
with `ferryx-relay` runnable on your own host. That is a real and checkable distinction, and it
is now stated on the page.

**Existing page mapping:** `/use-cases/remote-terminal-access/` (primary; needs a factual
refresh more than it needs new keywords).

## Cluster 4 — Conductor alternative for Windows / Linux

**Representative queries:** `conductor.build alternative windows`,
`conductor alternative linux ai agents`, `does conductor work on windows`.

**Intent:** *transactional/comparison*. The user has already chosen the category and is blocked
by platform. Highest commercial intent of the five clusters.

**Observed results:**

- https://runpane.com/alternatives/conductor , /alternatives/conductor-windows , /alternatives/conductor-linux
- https://nimbalyst.com/blog/best-conductor-alternatives-2026/ , /compare/nimbalyst-vs-conductor/ ,
  /conductor-for-windows-and-linux/
- https://paseo.sh/alternatives/conductor
- https://superset.sh/compare/conductor-alternative
- https://agentsroom.dev/alternatives/conductor
- https://vicoa.ai/vs/conductor
- https://aq.dev/alternatives/conductor/
- https://zencoder.ai/blog/conductor-alternatives
- https://www.reddit.com/r/conductorbuild/comments/1s80oc8/open_source_alternatives_to_conductor/
  (*30+ comments / 31 answers*)
- https://www.reddit.com/r/webdev/comments/1sts8th/looking_for_the_best_alternative_to_conductor_ai/ (*13 answers*)
- https://www.reddit.com/r/SideProject/comments/1vm5uz7/build_a_conductor_build_alternative_for_windows/
- https://www.reddit.com/r/LocalLLaMA/comments/1q9gwpx/developers_what_code_orchestration_tools_do_you/ (*23 answers*)

**Verified premise:** https://www.conductor.build/docs/installation (fetched 2026-09-19, re-checked
the same day) states verbatim: *"Conductor is not available for Windows or Linux yet."* The whole
cluster rests on a fact that is still true today.

**Correction to this cluster's framing.** Conductor is a macOS desktop application, not a cloud
service. Its docs describe local workspaces as git worktrees created under
`~/conductor/workspaces/<repo name>/<workspace name>`
(https://www.conductor.build/docs/concepts/git-worktrees), and its pricing page gates Conductor
Cloud, multiplayer, the API, and the mobile app behind Pro ($50/mo) and above
(https://www.conductor.build/pricing). Cloud workspaces are Vercel sandboxes, 8-core / 16 GB,
Amazon Linux 2023, `us-east-1`, with chat messages stored on Conductor's servers. So the
comparison is not local-vs-cloud: both tools run agents locally by default, and the real axes are
platform availability, who owns the session when the app closes, agent breadth, and price.

**Demand proxies:** at least **eight** competitors run a dedicated `/alternatives/conductor`
page, several with platform-split children (`-windows`, `-linux`). Google's AI Overview for the
Windows query names Pane, Nimbalyst, and Paseo by name and cites those vendor pages directly.
The r/conductorbuild thread is founder-answered and vendor-crowded. This is the most contested
term in the set.

**Where Ferryx actually stands:** `/compare/conductor/` exists and *is indexed*, but it argued
the wrong axis. It framed the choice as local-vs-cloud microVMs and never once said
"Windows" or "Linux" — verified: the string "Windows" appeared in that page only inside the
packaging bullet, and the page contained no platform-availability comparison. Meanwhile the
query that people actually type is a platform question with a verified answer in Ferryx's
favour. **Fixed on 2026-09-19:** the page now opens on the platform fact with a dated citation,
carries an availability table, and reframes the cloud section as a paid tier rather than the
product. Competitors' comparison pages already list an "Orca" (MIT, cross-platform, per-worktree
browser tab, mobile companions) in exactly the slot Ferryx would occupy; note Orca's own docs
describe `.orca/worktrees/<hash>` worktrees, which is close enough to Ferryx's
`.orca-worktrees/` naming that a `"orca-worktrees"` search returns Orca, not Ferryx. Ferryx is
invisible in a conversation its feature set belongs in.

**Existing page mapping:** `/compare/conductor/` (primary), `/compare/index.md` (secondary).

## Cluster 5 — tmux + git worktree, and worktree GUIs

**Representative queries:** `tmux git worktree gui`, `git worktree manager gui`,
`git worktree ai agents windows app`.

**Observed results:**

- https://github.com/raine/workmux + https://workmux.raine.dev/guide/
- https://github.com/denesbeck/tmux-worktree
- https://github.com/tt6746690/worktree-mux , https://github.com/andersonkrs/twig ,
  https://github.com/PeterHdd/Git-Worktree-Visualizer
- https://dev.to/b-d055/introducing-muxtree-dead-simple-worktree-tmux-sessions-for-ai-coding-2kf2
- https://marketplace.visualstudio.com/ (VS Code "TMUX Worktree" extension)
- https://apps.microsoft.com/detail/… + https://github.com/petroemil/git-worktree-manager
  (a Microsoft Store worktree GUI)
- https://github.com/jackiotyu/git-worktree-manager , https://github.com/chmouel/lazyworktree
- https://worktrunk.dev/ + https://github.com/max-sixty/worktrunk
- https://parallelcode.dev/use-cases/git-worktree-manager , https://www.worktreewise.com/blog/git-worktree-gui-…
- https://www.reddit.com/r/tmux/comments/1p7bszd/workmux_git_worktrees_tmux_windows_for/ ,
  /r/tmux/comments/1qqwdum/tmuxworktree_native_tmux_menus_for_git_worktree/ ,
  /r/ClaudeAI/comments/1mcny4a/gui_for_claude_git_worktree_management/ (*29 posts*)

**Intent:** *tool-shopping by shape* ("I want a UI for this"), split between tmux loyalists who
want a popup menu and people who want a real desktop app.

**Demand proxies:** Google explicitly flags `Missing: gui` on several results — meaning the
literal phrase has thin matching supply, and the engine is stretching. A Microsoft Store listing
ranks on page 1 for `git worktree manager gui`, which indicates a genuine Windows-desktop-GUI
appetite that the tmux ecosystem does not serve.

**Result-set shape:** GitHub-repo-dominated, almost no editorial content. Lowest-authority
competition of the five clusters.

**Realistic Ferryx differentiation:** Ferryx is a genuine cross-platform desktop GUI with a
terminal per worktree, which is precisely the thing the tmux plugins approximate and the
Windows Store GUI does without terminals. `/compare/tmux-git-worktree/` was written as an
essay about when to keep tmux and named no competing tool, so it matched none of the
comparison-shaped queries people issue. **Fixed 2026-09-19:** workmux (MIT), worktrunk
(MIT or Apache-2.0), lazyworktree (Apache-2.0), and tmux-worktree (MIT) are now named with
licenses and one-line descriptions, and the page concedes that a terminal-resident user is
probably better served by one of them.

**Existing page mapping:** `/compare/tmux-git-worktree/` (primary),
`/use-cases/git-worktree-workflow/` (secondary).

## Cluster-to-page map and gaps

| Cluster | Best existing page | Indexed? | Verdict |
| :--- | :--- | :--- | :--- |
| 1. Parallel Claude Code worktrees | `/use-cases/parallel-ai-agents/` | yes | **Fixed 2026-09-19.** Now covers `claude --worktree`, the four isolation checks, and Codex's detached-HEAD worktrees, then argues the cross-vendor convention |
| 2. Codex worktree workflow | *(none)* | — | **Gap.** No Codex page; no "two vendors, one repo" page. Partly mitigated by the Codex section now on the parallel-agents page |
| 3. Phone monitoring / session survival | `/use-cases/remote-terminal-access/` | yes | **Fixed 2026-09-19.** Leads with Claude and Warp Remote Control, states their documented boundaries, then positions Ferryx as self-hosted and agent-agnostic |
| 4. Conductor alternative Windows/Linux | `/compare/conductor/` | yes | **Fixed 2026-09-19.** Leads with the platform fact and an availability table; cloud reframed as a paid tier |
| 5. tmux / worktree GUI | `/compare/tmux-git-worktree/` | yes | **Fixed 2026-09-19.** Names workmux, worktrunk, lazyworktree, and tmux-worktree with licenses, and concedes where they fit better |
| — | `/compare/warp/` | yes | **Fixed 2026-09-19.** The page claimed Warp was closed source; Warp's client is AGPL-3.0 (MIT for `warpui`/`warpui_core`) with a closed server |
| — | `/docs/facts/` | **not observed** | Absent from the sampled search results; index status requires URL Inspection |

Additional gaps worth naming, all observed rather than inferred:

- **Brand ambiguity is unresolved.** `ferryx` returns a UK probiotics company for the entire
  first page, including the AI Overview. Any brand-led acquisition plan is fighting an
  established entity with a `.com`, a LinkedIn presence, and government-grant coverage. Plan
  for *category* queries, not brand queries, until that changes.
- **No "keep the agent running when I close the window" page.** The `keep claude code running
  after closing terminal` and `keep coding agent running when you close the app` SERPs are full
  of tmux tutorials and one competitor (herdr) whose pitch is literally Ferryx's daemon feature.
  Ferryx's strongest architectural claim has no page dedicated to the question users ask.
- **The `/compare/` hub is indexed with a stale, incorrect license claim** — the single
  highest-severity item on this list, because it publishes a wrong legal fact in Google's own
  snippet.

## Recommended first three page improvements

Ordered by (verified problem) × (query intent value) × (effort), not by traffic guesses.

### 1. Force a re-crawl of the pages carrying stale license text, and close the `http://` duplicate

**Problem, verified:** Google's snippets for `/compare/` and `/compare/tmux-git-worktree/`
assert "MIT licensed" and "v0.1.0"; the live pages say SUL-1.0 and source-available.
`http://ferryx.dev/…` serves 200 without redirecting, and Google has indexed at least one page
under that scheme.

**Do:** request indexing for `/compare/`, `/compare/tmux-git-worktree/`, and `/docs/facts/` in
Search Console; add a 301 from `http://` to `https://` at the Cloudflare edge so the apex has a
single live origin; confirm `/docs/facts/` is not excluded (it is in `sitemap-0.xml` and returns
200, so the likely cause is discovery/quality, not blocking — check the Search Console coverage
state before changing the page).

**Why first:** it is the only item on this list where the site is currently publishing a
factually wrong claim to searchers, and it costs no new content.

### 2. Rewrite `/compare/conductor/` around the platform question — **done 2026-09-19**

**Problem, verified:** the page never addressed Windows or Linux availability, while every
competing page that ranks for `conductor alternative windows` leads with exactly that, and
Conductor's own install doc confirms *"not available for Windows or Linux yet."*

**What shipped:** the page now opens with the platform fact, cites
`https://www.conductor.build/docs/installation` with the read date, and carries a compact
availability table (platforms, licensing, agents, cloud) because that is the format every ranking
competitor uses and the format Google's AI Overview quotes from. The old local-vs-cloud framing
was corrected rather than kept: Conductor's free tier is a local macOS app using git worktrees
under `~/conductor/workspaces/`, and Conductor Cloud is a paid tier from $50/mo running Vercel
sandboxes. The "when Conductor is better" section survives and now names SOC 2 Type II, enterprise
SSO/SCIM, bundled agents, and the review-to-PR flow. A sibling Linux page is still deliberately
not created; revisit only if the main page proves it can rank.

**Why second:** highest-intent query cluster in the set, a verified factual hook in Ferryx's
favour, and the page already exists and is already indexed.

### 3. Refresh `/use-cases/remote-terminal-access/` against Claude Remote Control and Warp — **done 2026-09-19**

**Problem, verified:** the page's "usual answers" were SSH, tmux-over-SSH, and Tailscale. The
actual top two results for the phone query are Anthropic's own Remote Control and mobile docs,
with Warp third. A reader who has seen those pages would judge Ferryx's page out of date.

**What shipped:** the page now opens by describing Claude Code Remote Control and Warp Remote
Control accurately and favourably, then states each one's documented boundary: a claude.ai
Pro/Max/Team/Enterprise plan with API keys unsupported, no Bedrock / Google Agent Platform /
Microsoft Foundry, no custom `ANTHROPIC_BASE_URL`, the transcript stored on Anthropic servers,
Zero Data Retention orgs excluded, the local `claude` process required to stay alive, and
Claude-only scope. Ferryx's position follows: self-hosted gateway, PIN or QR pairing, any process
in a pane, `ferryx-relay` runnable on your own host. The security caveat is unchanged, and the
page now explicitly concedes what Ferryx lacks (push notifications, vendor-managed sync, support).

**Why third:** it fixes a staleness risk on an already-indexed page, and it converts a
"Ferryx does this too" page into a "Ferryx does this when the first-party option cannot" page,
which is the only version that survives contact with a reader who already found Anthropic's docs.

**Deliberately not in the top three** (worth doing next, in this order): a Codex-specific page
covering `$CODEX_HOME/worktrees`, detached HEAD, Handoff, and the 15-worktree cap, aimed at
cluster 2 where competition is thinnest; and a "run Codex and Claude Code on one repo" page.
Naming the actual tmux/worktree tools inside `/compare/tmux-git-worktree/` was pulled forward and
is **done**: workmux, worktrunk, lazyworktree, and tmux-worktree are now named with their
licenses, alongside a plain statement that they fit better for terminal-resident users.

### 4. Correct `/compare/warp/` — **done 2026-09-19, not in the original report**

**Problem, verified during the edit pass:** the page asserted "Warp is closed source" four times
and built its central contrast on that. Warp's client codebase is now public at
`github.com/warpdotdev/warp` under AGPL v3, with the `warpui` and `warpui_core` crates under MIT;
the server stays closed. Publishing a false licensing claim about a competitor is the same class
of error as the stale MIT snippet in item 1, and worse in that it was on the live page rather than
in a cache.

**What shipped:** the licensing section now states Warp's actual license split, concedes that
AGPL-3.0 is OSI open source while SUL-1.0 is not, and tells a reader who needs OSI open source to
pick Warp. Warp's fifteen documented CLI agents, its current pricing tiers, and Remote Control are
stated from primary sources, and the Ferryx contrast moved to "no cloud in the path" rather than
"we publish source and they don't."

## Success metrics

Measure against the GA4 + Search Console setup already documented in `docs/SITE_MEASUREMENT.md`
(property 555083020, measurement ID `G-D6CY5B9DF5`). All thresholds below are **decision
triggers, not forecasts** — no traffic projection is made because no baseline volume has been
measured.

**Leading indicators (4 weeks, Search Console):**

- Track indexed URLs through Search Console Page indexing and URL Inspection, not
  `site:` result counts. The broad query returned one result while narrower queries
  returned many more; those counts are not a reliable index census. The separate
  indexing audit confirmed the homepage and parallel-agent page are indexed.
- Zero `http://ferryx.dev` URLs remain in the index; zero snippets contain "MIT licensed" or
  "v0.1.0". These are binary and directly checkable.
- `/docs/facts/` returns at least one result for `site:ferryx.dev/docs/facts/`.
- Impressions become non-zero for at least one non-brand query family (Search Console
  Performance → Queries, filtered to queries containing `worktree`, `conductor`, or `parallel
  agent`). First non-zero impression is the milestone, not a rank.

**Outcome indicators (8–12 weeks):**

- `/compare/conductor/` records impressions for queries containing "conductor" **and** a
  platform word ("windows", "linux"). Any first-page position on one such query counts as the
  cluster-4 bet paying off.
- Clicks to `/use-cases/remote-terminal-access/` from queries containing "phone" or "remote".
- `download_click` events (already a GA4 key event, dimensioned by `platform` and
  `link_location`) attributed to sessions landing on `/compare/conductor/` — read the
  `platform` dimension specifically, since the Windows/Linux argument only worked if Windows
  and Linux downloads move.

**Guardrails — what must not be claimed:**

- A download click is not an install and not a retained user; `docs/SITE_MEASUREMENT.md`
  already forbids that inference and this report does not relax it.
- Consent-gated analytics undercount; measured visitors are a subset.
- None of the numbers above may be back-converted into a "search volume" for any keyword.
  No volume was measured in this work and none should be reported.

## Evidence index

Primary sources fetched and read on 2026-09-19 (all returned HTTP 200 unless noted). Sources
marked **(re-verified)** were fetched a second time the same day during the content-correction
pass that produced the site edits.

- https://code.claude.com/docs/en/worktrees **(re-verified)** — `--worktree`/`-w`,
  `.claude/worktrees/<name>/`, branch `worktree-<name>`, EnterWorktree/ExitWorktree,
  `isolation: worktree` subagent frontmatter, four isolation enforcement checks.
- https://code.claude.com/docs/en/remote-control **(re-verified)** — Pro/Max/Team/Enterprise
  plans, API keys unsupported, Bedrock / Google Agent Platform / Microsoft Foundry and custom
  `ANTHROPIC_BASE_URL` exclusions, QR code via spacebar, `--spawn worktree`, automatic reconnect,
  transcript stored on Anthropic servers, Zero Data Retention orgs excluded, local process must
  keep running.
- https://code.claude.com/docs/en/mobile
- https://learn.chatgpt.com/docs/environments/git-worktrees **(re-verified)** —
  `$CODEX_HOME/worktrees`, detached HEAD, Handoff, 15-worktree retention, branch-collision rule.
- https://www.conductor.build/docs/installation **(re-verified)** — "Conductor is available for
  macOS"; "Conductor is not available for Windows or Linux yet."
- https://www.conductor.build/docs/concepts/git-worktrees **(new)** — local workspaces are git
  worktrees under `~/conductor/workspaces/<repo name>/<workspace name>`; isolation is development
  isolation, not a security boundary.
- https://www.conductor.build/docs **(new)** — supported agents: Claude Code, Codex, Cursor,
  OpenCode.
- https://www.conductor.build/pricing **(new)** — Free $0 local-only; Pro $50/mo adds Cloud,
  multiplayer, API, and a mobile app marked "coming very soon"; Teams $60/mo/user; cloud
  workspaces are Vercel sandboxes, 8-core / 16 GB, Amazon Linux 2023, `us-east-1`; cloud chat
  messages stored on Conductor's servers; SOC 2 Type II.
- https://www.conductor.build/docs/cloud and https://www.conductor.build/ — Cloud microVMs,
  multiplayer, "Trusted by 100k+ builders" (vendor claim), iOS marked SOON.
- https://www.conductor.build/docs/faq **(new)** — bundles its own Claude Code and Codex
  installations; agents run unsandboxed with the user's permissions.
- https://docs.warp.dev/agents/cli-agents/remote-control/ **(re-verified)** — one-click publish,
  clipboard link, browser viewing with no install, view/edit permissions, sync stops on unpublish.
- https://docs.warp.dev/agents/cli-agents/overview/ **(new)** — fifteen supported CLI agents and a
  per-agent feature-support table.
- https://github.com/warpdotdev/Warp **(new, the correction that mattered)** — the client codebase
  is now public with ~80 crates; README: `warpui_core` and `warpui` under MIT, the rest under
  AGPL v3, server closed. GitHub API reports `license.spdx_id = AGPL-3.0`.
- https://www.warp.dev/pricing **(new)** — Free $0, Build from $20/mo, Max from $200/mo, plus
  enterprise; SOC 2 certified.
- https://github.com/raine/workmux (MIT), https://github.com/max-sixty/worktrunk +
  https://worktrunk.dev/ (MIT or Apache-2.0), https://github.com/chmouel/lazyworktree (Apache-2.0),
  https://github.com/denesbeck/tmux-worktree (MIT) **(new)** — licenses and descriptions read from
  the GitHub API and each README for the tmux/worktree comparison page.
- https://api.github.com/repos/Indosaram/ferryx/releases/latest **(new)** — v2026.09.18.1 assets:
  `Ferryx_universal.dmg`, `Ferryx_x64-setup.exe`, `Ferryx_amd64.AppImage`, `Ferryx_amd64.deb`,
  `SHA256SUMS.txt`.
- https://runpane.com/alternatives/conductor — competitor comparison-table format.
- https://paseo.sh/alternatives/conductor — Apache-2.0, daemon + mobile clients.
- https://superset.sh/compare/conductor-alternative — alternative table incl. "Orca".
- https://nimbalyst.com/blog/best-conductor-alternatives — cross-platform positioning.
- https://www.onorca.dev/ and its docs (per-worktree browser, `.orca/worktrees/<hash>`).
- https://www.reddit.com/r/conductorbuild/comments/1s80oc8/open_source_alternatives_to_conductor/
  (403 to curl; read via browser) and
  https://www.reddit.com/r/SideProject/comments/1vm5uz7/build_a_conductor_build_alternative_for_windows/
- Ferryx live pages checked for current on-page text and status: `/`, `/compare/`,
  `/compare/conductor/`, `/compare/tmux-git-worktree/`, `/use-cases/parallel-ai-agents/`,
  `/use-cases/git-worktree-workflow/`, `/docs/facts/`, `/robots.txt`, `/sitemap-0.xml`,
  plus `http://` and `www.` host behaviour.
- Ferryx repository facts used on the edited pages: `src-tauri/src/terminal/output_hub.rs`
  (512 KiB ring buffer), `src-tauri/src/worktree/` (`.orca-worktrees/wt-<slug>`,
  `orca/<workspace-id>/<slug>`), `src-tauri/src/agent_detect/manifests/` (eleven manifests),
  `src-tauri/src/remote/auth.rs` (6-digit PIN), `src-tauri/src/bin/relay.rs` and
  `src-tauri/Cargo.toml` (`ferryx-relay` and `ferryx-cli` binaries).
