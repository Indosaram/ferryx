import {
  AgentsVisual,
  GhosttyVisual,
  MobileVisual,
  PersistenceVisual,
  SplitVisual,
  ZeroElectronVisual,
} from "@/components/FeatureVisuals";

export function Features() {
  const features = [
    {
      visual: GhosttyVisual,
      eyebrow: "Rendering",
      title: "Native Ghostty & wgpu Engine",
      description: "Desktop terminal panes render directly via native libghostty and a GPU-accelerated wgpu pipeline for crisp font rasterization and low-latency throughput.",
      points: ["libghostty terminal core", "wgpu GPU pipeline", "Sub-frame input latency"],
    },
    {
      visual: AgentsVisual,
      eyebrow: "Agents",
      title: "Multi-Agent Workspaces",
      description: "Orchestrate parallel AI coding agents (Claude, Codex, Gemini Flash) in isolated split-panes with real-time status indicators.",
      points: ["Isolated worktree per agent", "Live status indicators", "Launch from the tab bar"],
    },
    {
      visual: SplitVisual,
      eyebrow: "Layout",
      title: "Flexible Split-Pane Tiling",
      description: "Arbitrary vertical and horizontal terminal splits with responsive pointer drag resizing and smooth layout transitions.",
      points: ["Vertical & horizontal splits", "Pointer drag resizing", "Drag tabs into any pane"],
    },
    {
      visual: MobileVisual,
      eyebrow: "Remote",
      title: "Mobile Web Pairing",
      description: "Secure, authenticated remote web access via QR/PIN code. Stream terminal output via lightweight browser xterm.js and steer agent workflows on the go.",
      points: ["6-digit PIN pairing", "Streamed terminal grid", "Steer agents from a phone"],
    },
    {
      visual: ZeroElectronVisual,
      eyebrow: "Architecture",
      title: "Zero Electron Overhead",
      description: "Built on Tauri v2 and native Webview2/WebKit engines paired with a headless Rust PTY daemon. Instant startup with minimal footprint.",
      points: ["Tauri v2 shell", "Headless Rust PTY daemon", "macOS, Windows, Linux"],
    },
    {
      visual: PersistenceVisual,
      eyebrow: "Reliability",
      title: "Resilient Persistence",
      description: "Automatic workspace state snapshotting and background daemon reattachment guarantee you never lose work on crash or exit.",
      points: ["Layout snapshots", "Daemon survives the GUI", "Reattach with replay"],
    },
  ];

  return (
    <section id="features" className="py-24 sm:py-28 border-t border-line bg-page-raised relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16 sm:mb-20">
          <p className="text-[11px] font-medium uppercase tracking-[0.14em] text-ink-faint mb-3">
            Why Ferryx
          </p>
          <h2 className="text-[clamp(2rem,4vw,3rem)] font-medium tracking-[-0.035em] leading-[1.05] text-ink">
            Speed. Isolation. Total control.
          </h2>
          <p className="mt-4 text-[15px] leading-relaxed text-ink-soft">
            Every layer in Ferryx is engineered from scratch for minimal latency, zero memory leaks, and seamless autonomous agent collaboration.
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
