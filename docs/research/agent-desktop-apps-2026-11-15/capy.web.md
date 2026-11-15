# Capy - public-web evidence (todo 17)

- Evidence route: PUBLIC_BINARY (no OSS_REPO; see acquisition.json). This file is the public-web supplement; binary findings live in apps/capy.binary.md (todo 16).
- Vendor: Scrapybara, Inc. Site https://capy.ai, docs https://docs.capy.ai. Captured 2026-11-15T15:20Z without login.
- Saved sources: web/capy/site-*.md (pandoc conversion of web/capy/raw-html/site-*.html: index, pricing, changelog, download, solo, security, open-source, features-captains, features-review), web/capy/docs-*.md (vendor-served Markdown twins from docs.capy.ai/<page>.md), web/capy/docs-llms.txt, landing screenshot web/capy/landing.png (headless Chrome via browser skill owned engine, 1440x900).
- Every line in the Claims section is a vendor claim (unverified): marketing/docs text, not measured behaviour. The quoted string appears verbatim in the cited file (checked by check-web-claims.mjs).

## Claims

- [positioning] vendor claim (unverified): "Orchestrate thousands of agents on any model, across the cloud and your machines." (source: web/capy/site-index.md)
- [a stack/platforms] vendor claim (unverified): "Web and desktop, on macOS, Windows, and Linux" (source: web/capy/docs-welcome.md)
- [a stack/platforms] vendor claim (unverified): "Apple Silicon · Version 0.8.0" (source: web/capy/site-download.md)
- [a stack/headless] vendor claim (unverified): "The Dock CLI, `capy dock`, is Capy without the desktop app." (source: web/capy/docs-dock.md)
- [b agent integration] vendor claim (unverified): "You can also run models on your own subscriptions and API keys, on any plan, and Capy never charges for those tokens." (source: web/capy/docs-models-and-pricing.md)
- [b agent integration] vendor claim (unverified): "A Claude Pro or Max plan runs Claude models in **Claude Code** mode." (source: web/capy/docs-models-and-pricing.md)
- [b agent integration] vendor claim (unverified): "it runs on the Claude Agent SDK" (source: web/capy/docs-models-and-pricing.md)
- [b agent integration] vendor claim (unverified): "A connected Codex, GitHub Copilot, or SuperGrok subscription runs the models its plan includes" (source: web/capy/docs-models-and-pricing.md)
- [b agent integration/subagents] vendor claim (unverified): "A subagent can run commands, open PRs, and spawn subagents of its own, up to three levels deep." (source: web/capy/docs-subagents.md)
- [b orchestration] vendor claim (unverified): "A Captain is a long-lived agent that runs other threads for you." (source: web/capy/docs-captains.md)
- [c worktree isolation] vendor claim (unverified): "A fresh git worktree, started from the latest base branch on your remote." (source: web/capy/docs-desktop.md)
- [c worktree isolation] vendor claim (unverified): "Worktrees are under `~/.capy/worktrees` and need 1 GB of free disk" (source: web/capy/docs-desktop.md)
- [c cloud isolation] vendor claim (unverified): "Every cloud thread runs on its own isolated machine with no GitHub token on it" (source: web/capy/site-security.md)
- [d terminal] vendor claim (unverified): "opens a terminal on the thread's machine and wakes it if it's asleep." (source: web/capy/docs-threads.md)
- [d terminal] vendor claim (unverified): "There is no SSH." (source: web/capy/docs-machines.md)
- [e review/PR] vendor claim (unverified): "The thread that owns a pull request fixes failing CI, answers review feedback, resolves conflicts, and wraps up after the merge." (source: web/capy/docs-pull-requests.md)
- [e review/PR] vendor claim (unverified): "posts findings as inline GitHub comments" (source: web/capy/docs-review.md)
- [e review/PR] vendor claim (unverified): "Squash and merge" (source: web/capy/docs-pull-requests.md)
- [f remote/mobile] vendor claim (unverified): "you can start, follow, and reply to its threads from any browser, including your phone's." (source: web/capy/docs-desktop.md)
- [f remote/mobile] vendor claim (unverified): "Moves between devices go through the cloud." (source: web/capy/docs-desktop.md)
- [g attention] vendor claim (unverified): "controls alerts when a thread finishes or needs you." (source: web/capy/docs-desktop.md)
- [g attention] vendor claim (unverified): "It tags you when a decision or a blocker is yours, and the tag notifies you like any mention." (source: web/capy/docs-captains.md)
- [h checkpoint/rollback] vendor claim (unverified): "Capy keeps a cloud checkpoint of your uncommitted work" (source: web/capy/docs-desktop.md)
- [h checkpoint/rollback] vendor claim (unverified): "Running processes, such as dev servers and watchers." (source: web/capy/docs-desktop.md)
- [i persistence/recovery] vendor claim (unverified): "A thread on your computer waits while the computer sleeps or is offline, for up to 24 hours." (source: web/capy/docs-desktop.md)
- [i persistence/recovery] vendor claim (unverified): "A cloud thread keeps working when you close your laptop." (source: web/capy/docs-desktop.md)
- [j license/telemetry] vendor claim (unverified): "No training on your non-public code or chats unless you opt in." (source: web/capy/site-security.md)
- [j license/telemetry] vendor claim (unverified): "SOC 2 Type II, audited every" (source: web/capy/site-security.md)
- [j license/OSS] vendor claim (unverified): "Public open source projects can apply for free daily Capy credits" (source: web/capy/site-open-source.md)
- [pricing] vendor claim (unverified): "One subscription per organization, with unlimited members and no per-seat charge." (source: web/capy/site-pricing.md)
- [pricing] vendor claim (unverified): "Billed only while awake, prorated to the exact time, plus a 2-minute wind-down." (source: web/capy/site-pricing.md)
- [changelog] vendor claim (unverified): "Split panes, SSO and passkeys, and comments on images and videos" (source: web/capy/site-changelog.md)

## Notes

- "Running processes, such as dev servers and watchers." is listed by the vendor under what stays behind when a thread moves between machines.
- No account was created and no sign-in page was submitted; capy.ai/sign-up and /sign-in were not opened.
- Telemetry of the desktop app itself is not stated on the captured pages; check apps/capy.binary.md for shipped analytics SDKs.
