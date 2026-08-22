import { Zap, Activity, HardDrive, Timer, Check, X } from "lucide-react";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/Card";

export function Benchmarks() {
  const metrics = [
    {
      label: "Cold Startup Time",
      ferryx: "115 ms",
      electron: "3,800 ms",
      diff: "33x Faster",
      icon: <Timer className="h-4 w-4 text-emerald-400" />,
    },
    {
      label: "Idle Memory Usage (2 Panes)",
      ferryx: "82 MB",
      electron: "840 MB",
      diff: "10x Less RAM",
      icon: <Activity className="h-4 w-4 text-emerald-400" />,
    },
    {
      label: "Binary / Install Size",
      ferryx: "18.4 MB",
      electron: "220 MB",
      diff: "12x Smaller",
      icon: <HardDrive className="h-4 w-4 text-emerald-400" />,
    },
    {
      label: "PTY Throughput Latency",
      ferryx: "<0.4 ms",
      electron: "8-14 ms",
      diff: "Instant I/O",
      icon: <Zap className="h-4 w-4 text-emerald-400" />,
    },
  ];

  const comparisonRows = [
    { feature: "Native Rust PTY Host Daemon", ferryx: true, electron: false },
    { feature: "Sub-150ms Cold Launch", ferryx: true, electron: false },
    { feature: "Integrated Webview Split-Tabs", ferryx: true, electron: false },
    { feature: "Mobile Web Remote Pairing", ferryx: true, electron: false },
    { feature: "Multi-Agent Status Indicators", ferryx: true, electron: false },
    { feature: "Persistent Background Daemon", ferryx: true, electron: false },
    { feature: "Memory Consumption Under 100MB", ferryx: true, electron: false },
  ];

  return (
    <section id="benchmarks" className="py-20 border-t border-zinc-800/60 bg-zinc-950/80 relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16">
          <h2 className="text-3xl sm:text-4xl font-bold tracking-tight text-zinc-100">
            Uncompromising Speed & Efficiency
          </h2>
          <p className="mt-4 text-zinc-400 text-base">
            Benchmarked against traditional Electron-based AI IDEs and terminal emulators on Apple M-Series and Linux.
          </p>
        </div>

        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 mb-14">
          {metrics.map((metric, idx) => (
            <Card key={idx} className="bg-zinc-900/40 border-zinc-800">
              <CardHeader className="p-5 pb-2">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-medium text-zinc-400">{metric.label}</span>
                  {metric.icon}
                </div>
                <CardTitle className="text-2xl font-bold text-zinc-100 mt-2 font-mono">
                  {metric.ferryx}
                </CardTitle>
              </CardHeader>
              <CardContent className="p-5 pt-0">
                <div className="flex items-center justify-between text-xs text-zinc-500 pt-3 border-t border-zinc-800/60 mt-3 font-mono">
                  <span>Electron: {metric.electron}</span>
                  <span className="text-emerald-400 font-semibold">{metric.diff}</span>
                </div>
              </CardContent>
            </Card>
          ))}
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
