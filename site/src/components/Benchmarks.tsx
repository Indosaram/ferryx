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
    <section id="architecture" className="py-20 border-t border-zinc-800/60 bg-zinc-950/80 relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16">
          <h2 className="text-3xl sm:text-4xl font-bold tracking-tight text-zinc-100">
            Built Different
          </h2>
          <p className="mt-4 text-zinc-400 text-base">
            How Ferryx's native Ghostty and Rust architecture compares to traditional Electron-based AI IDEs and terminal emulators.
          </p>
        </div>

        <div className="rounded-xl border border-zinc-800 bg-zinc-900/30 overflow-hidden backdrop-blur-sm">
          <div className="p-6 border-b border-zinc-800 bg-zinc-900/60">
            <h3 className="text-lg font-semibold text-zinc-100">Direct Feature Matrix</h3>
            <p className="text-xs text-zinc-400 mt-1">Comparing Ferryx Architecture vs Heavy Webview Bundles</p>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm text-zinc-300">
              <thead className="border-b border-zinc-800 bg-zinc-950/60 text-xs uppercase font-medium text-zinc-400">
                <tr>
                  <th className="py-3.5 px-6">Capability</th>
                  <th className="py-3.5 px-6 text-center text-zinc-100 font-bold">Ferryx (Rust + Tauri v2)</th>
                  <th className="py-3.5 px-6 text-center text-zinc-500">Traditional Electron IDEs</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-zinc-800/60 font-mono text-xs">
                {comparisonRows.map((row, idx) => (
                  <tr key={idx} className="hover:bg-zinc-900/40 transition-colors">
                    <td className="py-4 px-6 font-sans text-sm font-medium text-zinc-200">{row.feature}</td>
                    <td className="py-4 px-6 text-center">
                      <div className="inline-flex items-center justify-center h-6 w-6 rounded-full bg-emerald-950/80 border border-emerald-800 text-emerald-400">
                        <Check className="h-3.5 w-3.5" />
                      </div>
                    </td>
                    <td className="py-4 px-6 text-center">
                      <div className="inline-flex items-center justify-center h-6 w-6 rounded-full bg-zinc-900 border border-zinc-800 text-zinc-500">
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
