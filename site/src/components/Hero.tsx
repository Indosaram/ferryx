import type { ReactNode } from "react";
import { Github, ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { CopySnippet } from "@/components/ui/CopySnippet";
import { DownloadMenu } from "@/components/DownloadMenu";
import { PlatformIcon } from "@/components/ui/PlatformIcons";
import { PLATFORMS } from "@/lib/downloads";

export function Hero({ children }: { children?: ReactNode }) {
  return (
    <section className="relative pt-20 pb-16 md:pt-28 md:pb-24 overflow-hidden">
      <div className="absolute inset-0 bg-grid-pattern opacity-40 pointer-events-none" />

      <div className="relative mx-auto max-w-6xl px-4 sm:px-6 text-center">
        <h1 className="text-4xl sm:text-6xl md:text-7xl font-extrabold tracking-tight text-zinc-100 max-w-4xl mx-auto leading-[1.1]">
          Parallel Agentic Development. Zero Bloat.
        </h1>

        <p className="mt-6 text-lg sm:text-xl text-zinc-400 max-w-2xl mx-auto leading-relaxed">
          Ultra-lightweight Rust workspace powered by a native Ghostty terminal engine with wgpu GPU rendering, seamless split-pane tiling, embedded web companion, and mobile web remote control.
        </p>

        {/* Primary Action Section */}
        <div className="mt-10 flex flex-col sm:flex-row items-center justify-center gap-4">
          <DownloadMenu variant="hero" />
          <a href="https://github.com/Indosaram/ferryx" target="_blank" rel="noreferrer">
            <Button variant="outline" size="lg" className="border-zinc-800 hover:bg-zinc-900 text-zinc-300 px-6 py-3 h-auto text-base">
              <Github className="mr-2 h-5 w-5" />
              View on GitHub
            </Button>
          </a>
        </div>

        {/* Platform Direct Quick Links */}
        <div className="mt-4 flex flex-wrap items-center justify-center gap-x-6 gap-y-2 text-xs text-zinc-400">
          <span className="text-zinc-500 font-medium">Direct Downloads:</span>
          <a
            href={PLATFORMS.macos.defaultAsset.url}
            className="inline-flex items-center gap-1.5 hover:text-zinc-200 transition-colors py-1 px-2 rounded-md hover:bg-zinc-900/60"
            title="Download for macOS (Apple Silicon .dmg)"
          >
            <PlatformIcon platform="macos" className="h-3.5 w-3.5 text-zinc-400" />
            <span>macOS (.dmg)</span>
          </a>
          <a
            href={PLATFORMS.windows.defaultAsset.url}
            className="inline-flex items-center gap-1.5 hover:text-zinc-200 transition-colors py-1 px-2 rounded-md hover:bg-zinc-900/60"
            title="Download for Windows (NSIS Setup .exe)"
          >
            <PlatformIcon platform="windows" className="h-3.5 w-3.5 text-zinc-400" />
            <span>Windows (.exe / .msix)</span>
          </a>
          <a
            href={PLATFORMS.linux.defaultAsset.url}
            className="inline-flex items-center gap-1.5 hover:text-zinc-200 transition-colors py-1 px-2 rounded-md hover:bg-zinc-900/60"
            title="Download for Linux (.AppImage / .deb)"
          >
            <PlatformIcon platform="linux" className="h-3.5 w-3.5 text-zinc-400" />
            <span>Linux (.AppImage / .deb)</span>
          </a>
        </div>

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
          <a href="#downloads">
            <Button size="lg" className="bg-zinc-100 text-zinc-900 hover:bg-zinc-200 font-semibold px-6">
              Download All Platforms
              <ArrowRight className="ml-2 h-4 w-4" />
            </Button>
          </a>
          <a href="#quickstart">
            <Button variant="outline" size="lg" className="border-zinc-800 hover:bg-zinc-900 text-zinc-300">
              Quickstart Guide
            </Button>
          </a>
        </div>
      </div>
    </section>
  );
}
