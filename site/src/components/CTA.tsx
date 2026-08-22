import { Download, Github, Terminal, ArrowRight } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { CopySnippet } from "@/components/ui/CopySnippet";
import ferryxIcon from "@/assets/ferryx-icon.png";

const iconSrc = typeof ferryxIcon === 'object' && ferryxIcon !== null && 'src' in ferryxIcon ? (ferryxIcon as { src: string }).src : String(ferryxIcon);

export function CTA() {
  return (
    <section id="quickstart" className="py-20 border-t border-zinc-800/60 bg-gradient-to-b from-zinc-950 via-zinc-900/30 to-zinc-950 relative">
      <div className="mx-auto max-w-5xl px-4 sm:px-6 text-center">
        <div className="rounded-2xl border border-zinc-800 bg-zinc-950/80 p-8 sm:p-12 shadow-2xl relative overflow-hidden">
          <img
            src={iconSrc}
            alt="Ferryx"
            className="h-16 w-16 mx-auto mb-6 rounded-2xl drop-shadow-xl"
          />

          <h2 className="text-3xl sm:text-4xl font-bold tracking-tight text-zinc-100 max-w-xl mx-auto">
            Ready to Supercharge Your Agentic Development?
          </h2>

          <p className="mt-4 text-zinc-400 max-w-md mx-auto text-sm sm:text-base">
            Get started with Ferryx in seconds. Build natively on macOS, Linux, and Windows with companion mobile remote access.
          </p>

          <div className="mt-8 max-w-md mx-auto">
            <CopySnippet
              code="git clone https://github.com/ferryx/ferryx && cd ferryx && bun dev"
              className="text-left"
            />
          </div>

          <div className="mt-8 flex flex-col sm:flex-row items-center justify-center gap-4">
            <a href="https://github.com/ferryx/ferryx/releases" target="_blank" rel="noreferrer">
              <Button size="lg" className="w-full sm:w-auto bg-zinc-100 text-zinc-900 hover:bg-zinc-200 font-semibold px-8">
                <Download className="mr-2 h-4 w-4" />
                Download Alpha Release
              </Button>
            </a>
            <a href="https://github.com/ferryx/ferryx" target="_blank" rel="noreferrer">
              <Button variant="outline" size="lg" className="w-full sm:w-auto border-zinc-800 hover:bg-zinc-900 text-zinc-300">
                <Github className="mr-2 h-4 w-4" />
                GitHub Repository
                <ArrowRight className="ml-2 h-4 w-4 text-zinc-500" />
              </Button>
            </a>
          </div>

          <div className="mt-8 flex items-center justify-center space-x-6 text-xs text-zinc-500 font-mono">
            <span className="flex items-center gap-1.5">
              <Terminal className="h-3.5 w-3.5 text-zinc-400" /> Rust + Tauri v2
            </span>
            <span>•</span>
            <span>Apache 2.0 / MIT License</span>
            <span>•</span>
            <span>100% Open Source</span>
          </div>
        </div>
      </div>
    </section>
  );
}
