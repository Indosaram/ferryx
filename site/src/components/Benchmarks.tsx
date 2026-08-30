export function Benchmarks() {
  const comparisonRows = [
    { component: "Terminal parser", implementation: "libghostty-vt", evidence: "src-tauri/src/native_terminal/sys/ffi.rs" },
    { component: "Desktop renderer", implementation: "WGPU native child surfaces", evidence: "src-tauri/src/native_terminal/renderer/" },
    { component: "PTY lifecycle", implementation: "Headless Rust daemon with sequenced replay", evidence: "src-tauri/src/daemon/ · terminal/output_hub.rs" },
    { component: "Embedded browser", implementation: "Native WebView split-tabs", evidence: "ui/src/components/browser/" },
    { component: "Mobile terminal", implementation: "Dependency-free DOM grid", evidence: "ui/src/remote/RemoteTerminal.tsx" },
    { component: "Agent supervision", implementation: "Manifest-driven status detection", evidence: "src-tauri/src/agent_detect/" },
  ];

  return (
    <section id="architecture" className="py-24 sm:py-28 border-t border-line bg-page relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16">
          <h2 className="text-[clamp(2rem,4vw,3rem)] font-medium tracking-[-0.035em] leading-[1.05] text-ink">
            Architecture, Not a Benchmark
          </h2>
          <p className="mt-4 text-[15px] leading-relaxed text-ink-soft">
            A source-linked inventory of what Ferryx implements. It does not claim comparative performance without comparative measurements.
          </p>
        </div>

        <div className="rounded-3xl border border-line bg-surface overflow-hidden shadow-card">
          <div className="p-6 border-b border-line bg-page-raised">
            <h3 className="text-[17px] font-medium tracking-[-0.015em] text-ink">Implementation Inventory</h3>
            <p className="text-[13px] text-ink-faint mt-1">Each row names the repository location that supports the claim.</p>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-left">
              <thead className="border-b border-line bg-page-raised text-[11px] uppercase tracking-[0.12em] text-ink-faint">
                <tr>
                  <th className="py-3.5 px-6 font-medium">Component</th>
                  <th className="py-3.5 px-6 font-medium text-ink">Ferryx implementation</th>
                  <th className="py-3.5 px-6 font-medium text-ink-faint">Evidence</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-line">
                {comparisonRows.map((row, idx) => (
                  <tr key={idx} className="hover:bg-page-raised transition-colors">
                    <td className="py-4 px-6 text-[15px] text-ink font-medium">{row.component}</td>
                    <td className="py-4 px-6 text-[14px] text-ink-soft">{row.implementation}</td>
                    <td className="py-4 px-6 font-mono text-[12px] text-ink-faint">{row.evidence}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </section>
  );
}

