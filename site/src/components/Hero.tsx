import { useState } from "react";
import { Terminal, Globe, Smartphone, ArrowRight, Play, CheckCircle2 } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { CopySnippet } from "@/components/ui/CopySnippet";

export function Hero() {
  const [activeTab, setActiveTab] = useState<"terminal" | "browser" | "mobile">("terminal");

  return (
    <section className="relative pt-20 pb-16 md:pt-28 md:pb-24 overflow-hidden">
      <div className="absolute inset-0 bg-grid-pattern opacity-40 pointer-events-none" />

      <div className="relative mx-auto max-w-6xl px-4 sm:px-6 text-center">
        <h1 className="text-4xl sm:text-6xl md:text-7xl font-extrabold tracking-tight text-zinc-100 max-w-4xl mx-auto leading-[1.1]">
          Parallel Agentic Development. Zero Bloat.
        </h1>

        <p className="mt-6 text-lg sm:text-xl text-zinc-400 max-w-2xl mx-auto leading-relaxed">
          Ultra-lightweight Rust native terminal host daemon with seamless split-pane tiling, embedded web companion, and authenticated mobile web remote control.
        </p>

        <div className="mt-8 flex flex-col sm:flex-row items-center justify-center gap-4 max-w-md mx-auto">
          <CopySnippet
            code="git clone https://github.com/stablyai/orca && cd orca && bun dev"
            className="w-full text-left"
          />
        </div>

        <div className="mt-6 flex flex-wrap items-center justify-center gap-4 text-xs text-zinc-400">
          <div className="flex items-center space-x-1.5">
            <CheckCircle2 className="h-4 w-4 text-zinc-300" />
            <span>&lt;150ms Cold Launch</span>
          </div>
          <div className="flex items-center space-x-1.5">
            <CheckCircle2 className="h-4 w-4 text-zinc-300" />
            <span>~82MB Base RAM</span>
          </div>
          <div className="flex items-center space-x-1.5">
            <CheckCircle2 className="h-4 w-4 text-zinc-300" />
            <span>Native Rust PTY</span>
          </div>
          <div className="flex items-center space-x-1.5">
            <CheckCircle2 className="h-4 w-4 text-zinc-300" />
            <span>Mobile Web Remote</span>
          </div>
        </div>

        <div className="mt-14 relative mx-auto max-w-5xl rounded-xl border border-zinc-800 bg-zinc-950/80 shadow-2xl overflow-hidden text-left">
          <div className="flex items-center justify-between border-b border-zinc-800/80 bg-zinc-900/60 px-4 py-2.5">
            <div className="flex items-center space-x-2">
              <div className="h-3 w-3 rounded-full bg-zinc-700/60" />
              <div className="h-3 w-3 rounded-full bg-zinc-700/60" />
              <div className="h-3 w-3 rounded-full bg-zinc-700/60" />
              <span className="ml-2 text-xs font-medium text-zinc-400 select-none">
                ferryx — workspace: main
              </span>
            </div>

            <div className="flex items-center space-x-1 rounded-md bg-zinc-950/80 p-0.5 border border-zinc-800">
              <button
                onClick={() => setActiveTab("terminal")}
                className={`flex items-center space-x-1.5 rounded px-2.5 py-1 text-xs font-medium transition-all ${
                  activeTab === "terminal"
                    ? "bg-zinc-800 text-zinc-100 shadow"
                    : "text-zinc-400 hover:text-zinc-200"
                }`}
              >
                <Terminal className="h-3.5 w-3.5" />
                <span>Agent Terminals (2)</span>
              </button>
              <button
                onClick={() => setActiveTab("browser")}
                className={`flex items-center space-x-1.5 rounded px-2.5 py-1 text-xs font-medium transition-all ${
                  activeTab === "browser"
                    ? "bg-zinc-800 text-zinc-100 shadow"
                    : "text-zinc-400 hover:text-zinc-200"
                }`}
              >
                <Globe className="h-3.5 w-3.5" />
                <span>Web Companion</span>
              </button>
              <button
                onClick={() => setActiveTab("mobile")}
                className={`flex items-center space-x-1.5 rounded px-2.5 py-1 text-xs font-medium transition-all ${
                  activeTab === "mobile"
                    ? "bg-zinc-800 text-zinc-100 shadow"
                    : "text-zinc-400 hover:text-zinc-200"
                }`}
              >
                <Smartphone className="h-3.5 w-3.5" />
                <span>Mobile Remote</span>
              </button>
            </div>
          </div>

          <div className="p-4 sm:p-6 font-mono text-xs sm:text-sm">
            {activeTab === "terminal" ? (
              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div className="rounded-lg border border-zinc-800/80 bg-zinc-950 p-4 space-y-2">
                  <div className="flex items-center justify-between text-zinc-500 pb-2 border-b border-zinc-900">
                    <span className="text-zinc-400 flex items-center gap-1.5">
                      <Terminal className="h-3 w-3 text-zinc-300" /> agent-lead [Claude 3.7]
                    </span>
                    <span className="text-emerald-400 text-[10px] bg-emerald-950/60 border border-emerald-800/40 px-1.5 py-0.5 rounded">active</span>
                  </div>
                  <div className="text-zinc-400 space-y-1">
                    <p className="text-zinc-500">$ senpi --task "Refactor auth and run tests"</p>
                    <p className="text-zinc-300">⚡ Spawned 2 background workers</p>
                    <p className="text-zinc-400">→ [worker-1] updating token auth middleware</p>
                    <p className="text-zinc-400">→ [worker-2] adding integration test matrix</p>
                    <p className="text-emerald-400 pt-2">✓ All 42 tests passing in 0.28s</p>
                  </div>
                </div>

                <div className="rounded-lg border border-zinc-800/80 bg-zinc-950 p-4 space-y-2">
                  <div className="flex items-center justify-between text-zinc-500 pb-2 border-b border-zinc-900">
                    <span className="text-zinc-400 flex items-center gap-1.5">
                      <Terminal className="h-3 w-3 text-zinc-300" /> subagent-worker [Gemini Flash]
                    </span>
                    <span className="text-zinc-400 text-[10px] bg-zinc-900 border border-zinc-800 px-1.5 py-0.5 rounded">idle</span>
                  </div>
                  <div className="text-zinc-400 space-y-1">
                    <p className="text-zinc-500">$ cargo test --manifest-path src-tauri/Cargo.toml</p>
                    <p className="text-zinc-300">running 18 tests</p>
                    <p className="text-zinc-400">test pty::daemon::tests::test_pty_lifecycle ... ok</p>
                    <p className="text-zinc-400">test session::tests::test_session_recovery ... ok</p>
                    <p className="text-emerald-400 pt-2">test result: ok. 18 passed; 0 failed</p>
                  </div>
                </div>
              </div>
            ) : activeTab === "browser" ? (
              <div className="rounded-lg border border-zinc-800/80 bg-zinc-950 p-6 space-y-4">
                <div className="flex items-center space-x-2 text-zinc-400 text-xs pb-3 border-b border-zinc-900">
                  <Globe className="h-3.5 w-3.5 text-zinc-300" />
                  <span className="text-zinc-200 bg-zinc-900 px-2.5 py-1 rounded border border-zinc-800 w-full truncate">
                    http://127.0.0.1:5173 — Dev Server Live View
                  </span>
                </div>
                <div className="flex items-center justify-center py-12 text-center text-zinc-400 space-y-2 flex-col">
                  <Globe className="h-10 w-10 text-zinc-500 stroke-1" />
                  <p className="text-sm font-medium text-zinc-300">Embedded Webview2 / WebKit Engine</p>
                  <p className="text-xs text-zinc-500 max-w-sm">
                    Inspect UI live side-by-side with your agent terminals without leaving the workspace.
                  </p>
                </div>
              </div>
            ) : (
              <div className="rounded-lg border border-zinc-800/80 bg-zinc-950 p-6 space-y-4">
                <div className="flex items-center justify-between text-zinc-400 text-xs pb-3 border-b border-zinc-900">
                  <div className="flex items-center space-x-2">
                    <Smartphone className="h-3.5 w-3.5 text-zinc-300" />
                    <span className="text-zinc-200 bg-zinc-900 px-2.5 py-1 rounded border border-zinc-800">
                      Mobile Remote Pair: 192.168.1.50:5174 / Tailscale
                    </span>
                  </div>
                  <span className="text-emerald-400 text-[10px] bg-emerald-950/60 border border-emerald-800/40 px-2 py-0.5 rounded">
                    Pairing Authenticated
                  </span>
                </div>
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 pt-2">
                  <div className="border border-zinc-800 rounded p-4 bg-zinc-900/40 space-y-2">
                    <p className="text-xs font-semibold text-zinc-200">Live Agent Stream</p>
                    <p className="text-zinc-400 text-xs font-mono">[lead] executing tests... (42/42 pass)</p>
                    <p className="text-zinc-500 text-[11px]">Real-time output over encrypted WebSocket</p>
                  </div>
                  <div className="border border-zinc-800 rounded p-4 bg-zinc-900/40 space-y-2">
                    <p className="text-xs font-semibold text-zinc-200">Remote Steering Controls</p>
                    <p className="text-zinc-400 text-xs font-mono">Send SIGINT (Ctrl+C), input prompt, approve tasks</p>
                    <p className="text-zinc-500 text-[11px]">Monitor on mobile while away from desk</p>
                  </div>
                </div>
              </div>
            )}
          </div>
        </div>

        <div className="mt-8 flex items-center justify-center space-x-4">
          <a href="#quickstart">
            <Button size="lg" className="bg-zinc-100 text-zinc-900 hover:bg-zinc-200 font-semibold px-6">
              Start Using Ferryx
              <ArrowRight className="ml-2 h-4 w-4" />
            </Button>
          </a>
          <a href="https://github.com/stablyai/orca" target="_blank" rel="noreferrer">
            <Button variant="outline" size="lg" className="border-zinc-800 hover:bg-zinc-900 text-zinc-300">
              <Play className="mr-2 h-4 w-4" />
              View on GitHub
            </Button>
          </a>
        </div>
      </div>
    </section>
  );
}
