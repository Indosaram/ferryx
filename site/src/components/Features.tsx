import { Cpu, SplitSquareHorizontal, Bot, ShieldCheck, Zap, Smartphone } from "lucide-react";
import { Card, CardHeader, CardTitle, CardDescription } from "@/components/ui/Card";
import { Badge } from "@/components/ui/Badge";

export function Features() {
  const features = [
    {
      icon: <Cpu className="h-6 w-6 text-zinc-200" />,
      badge: "Performance",
      title: "Native Ghostty & wgpu Engine",
      description: "Desktop terminal panes render directly via native libghostty and a GPU-accelerated wgpu pipeline for crisp font rasterization and low-latency throughput.",
    },
    {
      icon: <Bot className="h-6 w-6 text-zinc-200" />,
      badge: "Agentic AI",
      title: "Multi-Agent Workspaces",
      description: "Orchestrate parallel AI coding agents (Claude, Codex, Gemini Flash) in isolated split-panes with real-time status indicators.",
    },
    {
      icon: <SplitSquareHorizontal className="h-6 w-6 text-zinc-200" />,
      badge: "Productivity",
      title: "Flexible Split-Pane Tiling",
      description: "Arbitrary vertical and horizontal terminal splits with responsive pointer drag resizing and smooth layout transitions.",
    },
    {
      icon: <Smartphone className="h-6 w-6 text-zinc-200" />,
      badge: "Remote Control",
      title: "Mobile Web Pairing",
      description: "Secure, authenticated remote web access via QR/PIN code. Stream terminal output via lightweight browser xterm.js and steer agent workflows on the go.",
    },
    {
      icon: <Zap className="h-6 w-6 text-zinc-200" />,
      badge: "Architecture",
      title: "Zero Electron Overhead",
      description: "Built on Tauri v2 and native Webview2/WebKit engines paired with a headless Rust PTY daemon. Instant startup with minimal footprint.",
    },
    {
      icon: <ShieldCheck className="h-6 w-6 text-zinc-200" />,
      badge: "Reliability",
      title: "Resilient Persistence",
      description: "Automatic workspace state snapshotting and background daemon reattachment guarantee you never lose work on crash or exit.",
    },
  ];

  return (
    <section id="features" className="py-20 border-t border-zinc-800/60 bg-zinc-950/40 relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16">
          <h2 className="text-3xl sm:text-4xl font-bold tracking-tight text-zinc-100">
            Speed. Isolation. Total Control.
          </h2>
          <p className="mt-4 text-zinc-400 text-base">
            Every layer in Ferryx is engineered from scratch for minimal latency, zero memory leaks, and seamless autonomous agent collaboration.
          </p>
        </div>

        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
          {features.map((feature, idx) => (
            <Card key={idx} className="group relative overflow-hidden bg-zinc-900/30 border-zinc-800/80 hover:border-zinc-700 transition-all hover:bg-zinc-900/50">
              <div className="absolute top-0 right-0 p-4 opacity-10 group-hover:opacity-20 transition-opacity">
                {feature.icon}
              </div>
              <CardHeader className="p-6 space-y-3">
                <div className="flex items-center justify-between">
                  <div className="p-2.5 rounded-lg bg-zinc-900 border border-zinc-800 text-zinc-200 shadow-inner group-hover:scale-105 transition-transform">
                    {feature.icon}
                  </div>
                  <Badge variant="outline" className="text-[10px] text-zinc-400 border-zinc-800">
                    {feature.badge}
                  </Badge>
                </div>
                <CardTitle className="text-lg font-semibold text-zinc-100 group-hover:text-white transition-colors">
                  {feature.title}
                </CardTitle>
                <CardDescription className="text-zinc-400 text-sm leading-relaxed">
                  {feature.description}
                </CardDescription>
              </CardHeader>
            </Card>
          ))}
        </div>
      </div>
    </section>
  );
}
