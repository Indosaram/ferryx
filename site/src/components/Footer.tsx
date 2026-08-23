import { Github } from "lucide-react";
import ferryxIcon from "@/assets/ferryx-icon.png";

const iconSrc = typeof ferryxIcon === 'object' && ferryxIcon !== null && 'src' in ferryxIcon ? (ferryxIcon as { src: string }).src : String(ferryxIcon);

export function Footer() {
  return (
    <footer className="border-t border-zinc-800/80 bg-zinc-950 py-12 text-xs text-zinc-500">
      <div className="mx-auto max-w-6xl px-4 sm:px-6 flex flex-col sm:flex-row items-center justify-between gap-6">
        <div className="flex items-center space-x-3">
          <img src={iconSrc} alt="Ferryx" className="h-6 w-6 rounded-md opacity-90" />
          <span className="font-semibold text-zinc-300 tracking-tight">Ferryx</span>
          <span className="text-zinc-600">|</span>
          <span>Ultra-lightweight Rust Native AI Workspace</span>
        </div>

        <div className="flex items-center space-x-6">
          <a
            href="https://github.com/Indosaram/ferryx"
            target="_blank"
            rel="noreferrer"
            className="hover:text-zinc-300 transition-colors flex items-center gap-1"
          >
            <Github className="h-3.5 w-3.5" />
            GitHub
          </a>
          <a
            href="#features"
            className="hover:text-zinc-300 transition-colors"
          >
            Features
          </a>
          <a
            href="#architecture"
            className="hover:text-zinc-300 transition-colors"
          >
            Architecture
          </a>
        </div>

        <div>
          <p>© {new Date().getFullYear()} Ferryx Contributors. All rights reserved.</p>
        </div>
      </div>
    </footer>
  );
}
