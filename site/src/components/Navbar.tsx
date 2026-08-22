import { Github, Download } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import ferryxIcon from "@/assets/ferryx-icon.png";

const iconSrc = typeof ferryxIcon === 'object' && ferryxIcon !== null && 'src' in ferryxIcon ? (ferryxIcon as { src: string }).src : String(ferryxIcon);

export function Navbar({ basePath }: { basePath: string }) {
  return (
    <header className="sticky top-0 z-50 w-full border-b border-zinc-800/80 bg-zinc-950/70 backdrop-blur-md">
      <div className="mx-auto flex h-16 max-w-6xl items-center justify-between px-4 sm:px-6">
        <div className="flex items-center space-x-3">
          <a href={basePath} className="flex items-center space-x-2.5 group">
            <img
              src={iconSrc}
              alt="Ferryx Logo"
              className="h-8 w-8 rounded-lg transition-transform group-hover:scale-105"
            />
            <span className="font-bold tracking-tight text-lg text-zinc-100">
              Ferryx
            </span>
          </a>
          <Badge variant="secondary" className="hidden sm:inline-flex text-[10px] px-2 py-0.5 border-zinc-800">
            v0.1.0-alpha
          </Badge>
        </div>

        <nav className="hidden md:flex items-center space-x-6 text-sm font-medium text-zinc-400">
          <a href={`${basePath}docs/introduction/`} className="hover:text-zinc-100 transition-colors">
            Docs
          </a>
          <a href="#features" className="hover:text-zinc-100 transition-colors">
            Features
          </a>
          <a href="#architecture" className="hover:text-zinc-100 transition-colors">
            Architecture
          </a>
        </nav>

        <div className="flex items-center space-x-3">
          <a
            href="https://github.com/ferryx/ferryx"
            target="_blank"
            rel="noreferrer"
            className="text-zinc-400 hover:text-zinc-100 transition-colors"
          >
            <Button variant="ghost" size="icon" className="h-9 w-9">
              <Github className="h-4 w-4" />
            </Button>
          </a>
          <a href="#quickstart">
            <Button size="sm" className="bg-zinc-100 text-zinc-900 hover:bg-zinc-200 font-semibold">
              <Download className="mr-1.5 h-3.5 w-3.5" />
              Get Ferryx
            </Button>
          </a>
        </div>
      </div>
    </header>
  );
}
