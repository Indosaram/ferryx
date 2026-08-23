import type { ReactNode } from "react";
import { ArrowRight, Play } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { CopySnippet } from "@/components/ui/CopySnippet";

export function Hero({ children }: { children?: ReactNode }) {
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
            code="git clone https://github.com/Indosaram/ferryx.git && cd ferryx && bun install --cwd ui && bun install --cwd site && cd src-tauri && cargo tauri dev"
            className="w-full text-left"
          />
        </div>

        <div className="relative mx-auto mt-14 max-w-5xl">
          {children ?? (
            <div className="h-[560px] animate-pulse rounded-xl border border-zinc-800 bg-zinc-950/80" />
          )}
        </div>

        <div className="mt-8 flex items-center justify-center space-x-4">
          <a href="#quickstart">
            <Button size="lg" className="bg-zinc-100 text-zinc-900 hover:bg-zinc-200 font-semibold px-6">
              Start Using Ferryx
              <ArrowRight className="ml-2 h-4 w-4" />
            </Button>
          </a>
          <a href="https://github.com/Indosaram/ferryx" target="_blank" rel="noreferrer">
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
