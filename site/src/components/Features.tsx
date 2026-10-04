import {
  AgentsVisual,
  GhosttyVisual,
  MobileVisual,
  PersistenceVisual,
  SplitVisual,
  ZeroElectronVisual,
} from "@/components/FeatureVisuals";

const basePath = import.meta.env.BASE_URL.endsWith('/') ? import.meta.env.BASE_URL : `${import.meta.env.BASE_URL}/`;

export function Features() {
  const features = [
    {
      visual: AgentsVisual,
      eyebrow: "Agents",
      title: "Every agent in its own worktree",
      description:
        "Run Claude Code, Codex, Gemini CLI — anything that runs in a terminal — each in its own pane. When you ask for isolation, the app creates a managed Git worktree under .orca-worktrees/<workspace-id>/<slug> on branch orca/<workspace-id>/<slug>, jailed to the repo root, and removes it when you're done.",
      points: [
        "One pane per agent, side by side in one window",
        "App creates, names and removes the worktrees for you",
        "Status detection for 11 agents: working, waiting on you, idle",
      ],
    },
    {
      visual: PersistenceVisual,
      eyebrow: "Reliability",
      title: "Close the window. The agents keep working.",
      description:
        "A headless Rust daemon owns the terminals, not the window: closing, reloading or crashing the GUI does not kill running agents while the daemon lives. On reconnect, output you missed is replayed from a 512 KiB ring buffer per session — and if it wrapped, you are told there is a gap. Stopping the daemon or rebooting the host ends live sessions.",
      points: [
        "GUI can close, reload or crash; agents keep running",
        "Missed output replays on reconnect, with a gap warning if it wrapped",
        "Workspace layout snapshots and restores",
      ],
    },
    {
      visual: MobileVisual,
      eyebrow: "Remote",
      title: "Check in from your phone",
      description:
        "Pair a phone or browser with a 6-digit PIN or QR code and the mobile web client streams your terminal. It connects out through a relay tunnel, so it needs no inbound port and no public IP — and you can run your own self-hosted relay instead. Keep remote access off unless you need it.",
      points: [
        "Pair with a 6-digit PIN or QR code",
        "No inbound port or public IP — outbound relay tunnel",
        "Run your own self-hosted relay if you prefer",
      ],
    },
    {
      visual: SplitVisual,
      eyebrow: "Layout",
      title: "Terminals, browser and splits in one layout",
      description:
        "Split panes vertically or horizontally, drag tabs into any pane, and keep a native WebView browser tab next to your terminals so docs and dashboards sit beside the agents that need them. The layout is yours to rearrange and it snapshots with the workspace.",
      points: [
        "Vertical and horizontal splits, drag tabs into any pane",
        "Native browser tabs beside terminals (Cmd+Shift+B)",
        "Cmd+K palette, Cmd+T new tab, Cmd+D / Cmd+Shift+D to split",
      ],
    },
    {
      visual: GhosttyVisual,
      eyebrow: "Rendering",
      title: "Fast native terminal rendering",
      description:
        "Terminal output is parsed by libghostty-vt and each pane is drawn by WGPU on a native child surface, instead of a browser terminal emulator in the page.",
      points: [
        "libghostty-vt parses output; WGPU renders each pane natively",
        "Internal render benchmark: 50-frame offscreen p50 3.10 ms on M4 Max",
        "No performance comparison is claimed against other terminals",
      ],
    },
    {
      visual: ZeroElectronVisual,
      eyebrow: "Architecture",
      title: "No Electron under the hood",
      description:
        "Built in Rust on Tauri v2 with the OS webview — no Electron, no bundled Chromium — while a headless Rust PTY daemon owns the terminals. One codebase ships for macOS, Windows and Linux; some OS integrations (the Dock attention badge) are macOS-only.",
      points: [
        "Tauri v2 shell on the OS webview — no Electron, no bundled Chromium",
        "Headless Rust PTY daemon owns the terminals",
        "Cross-platform core; Dock badge is macOS-only",
      ],
    },
  ];

  return (
    <section id="features" className="py-24 sm:py-28 border-t border-line bg-page-raised relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16 sm:mb-20">
          <p className="text-[11px] font-medium uppercase tracking-[0.14em] text-ink-faint mb-3">
            What you get
          </p>
          <h2 className="text-[clamp(2rem,4vw,3rem)] font-medium tracking-[-0.035em] leading-[1.05] text-ink">
            Everything one window needs to run a fleet of agents
          </h2>
          <p className="mt-4 text-[15px] leading-relaxed text-ink-soft">
            Ferryx is a desktop terminal workspace for running several CLI coding agents at once.
            Each agent gets its own pane and a status you can read at a glance, isolation on
            request gives it its own managed worktree, and you can check in from your phone.
          </p>
        </div>

        <div className="flex flex-col gap-4">
          {features.map((feature, idx) => {
            const Visual = feature.visual;
            const flip = idx % 2 === 1;
            return (
              <div
                key={feature.title}
                className="grid grid-cols-1 items-center gap-8 rounded-2xl border border-line bg-surface p-8 shadow-card transition-colors hover:border-line-strong sm:p-10 lg:grid-cols-2 lg:gap-12"
              >
                <div className={flip ? "lg:order-2" : undefined}>
                  <p className="text-[11px] font-medium uppercase tracking-[0.14em] text-ink-faint mb-3">
                    {feature.eyebrow}
                  </p>
                  <h3 className="text-[clamp(1.375rem,2.2vw,1.875rem)] font-medium tracking-[-0.025em] leading-[1.15] text-ink">
                    {feature.title}
                  </h3>
                  <p className="mt-4 text-[16px] leading-relaxed text-ink-soft">
                    {feature.description}
                  </p>
                  {feature.eyebrow === 'Architecture' && (
                    <a href={`${basePath}docs/architecture/`} className="mt-4 inline-flex items-center text-[14px] font-medium text-ink hover:text-ink-soft transition-colors">Read the architecture deep dive →</a>
                  )}
                  <ul className="mt-6 flex flex-col gap-2.5">
                    {feature.points.map((point) => (
                      <li key={point} className="flex items-center gap-2.5 text-[14px] text-ink-soft">
                        <span className="h-1 w-1 shrink-0 rounded-full bg-ink-faint" />
                        {point}
                      </li>
                    ))}
                  </ul>
                </div>
                <div className={flip ? "lg:order-1" : undefined}>
                  <Visual />
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
