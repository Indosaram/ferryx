import { Check, X } from "lucide-react";

export function Benchmarks() {
  const comparisonRows = [
    { feature: "Native Ghostty Terminal Engine (Desktop)", ferryx: true, electron: false },
    { feature: "GPU-Accelerated wgpu Rendering Pipeline", ferryx: true, electron: false },
    { feature: "Native Rust PTY Host Daemon", ferryx: true, electron: false },
    { feature: "Integrated Webview Split-Tabs", ferryx: true, electron: false },
    { feature: "Mobile Web Remote Pairing (xterm.js Web Client)", ferryx: true, electron: false },
    { feature: "Multi-Agent Status Indicators", ferryx: true, electron: false },
    { feature: "Persistent Background Daemon", ferryx: true, electron: false },
  ];

  return (
    <section id="architecture" className="py-24 sm:py-28 border-t border-line bg-page relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16">
          <h2 className="text-[clamp(2rem,4vw,3rem)] font-medium tracking-[-0.035em] leading-[1.05] text-ink">
            Built Different
          </h2>
          <p className="mt-4 text-[15px] leading-relaxed text-ink-soft">
            How Ferryx's native Ghostty and Rust architecture compares to traditional Electron-based AI IDEs and terminal emulators.
          </p>
        </div>

        <div className="rounded-3xl border border-line bg-surface overflow-hidden shadow-card">
          <div className="p-6 border-b border-line bg-page-raised">
            <h3 className="text-[17px] font-medium tracking-[-0.015em] text-ink">Direct Feature Matrix</h3>
            <p className="text-[13px] text-ink-faint mt-1">Comparing Ferryx Architecture vs Heavy Webview Bundles</p>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-left">
              <thead className="border-b border-line bg-page-raised text-[11px] uppercase tracking-[0.12em] text-ink-faint">
                <tr>
                  <th className="py-3.5 px-6 font-medium">Capability</th>
                  <th className="py-3.5 px-6 text-center font-medium text-ink">Ferryx (Rust + Tauri v2)</th>
                  <th className="py-3.5 px-6 text-center font-medium text-ink-faint">Traditional Electron IDEs</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-line">
                {comparisonRows.map((row, idx) => (
                  <tr key={idx} className="hover:bg-page-raised transition-colors">
                    <td className="py-4 px-6 text-[15px] text-ink font-medium">{row.feature}</td>
                    <td className="py-4 px-6 text-center">
                      <div className="inline-flex items-center justify-center h-6 w-6 rounded-full bg-ink text-page">
                        <Check className="h-3.5 w-3.5" />
                      </div>
                    </td>
                    <td className="py-4 px-6 text-center">
                      <div className="inline-flex items-center justify-center h-6 w-6 rounded-full border border-line bg-surface text-mark-off">
                        <X className="h-3.5 w-3.5" />
                      </div>
                    </td>
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

