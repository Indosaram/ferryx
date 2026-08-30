import { Github } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import { DiscordIcon } from "@/components/ui/PlatformIcons";
import { DownloadMenu } from "@/components/DownloadMenu";
import { ThemeToggle } from "@/components/ThemeToggle";
import { DISCORD_INVITE_URL } from "@/lib/links";
import ferryxIcon from "@/assets/ferryx-icon.png";

const iconSrc = typeof ferryxIcon === 'object' && ferryxIcon !== null && 'src' in ferryxIcon ? (ferryxIcon as { src: string }).src : String(ferryxIcon);

export function Navbar({ basePath }: { basePath: string }) {
  return (
    <header className="fixed top-0 inset-x-0 z-50 px-4 pt-4">
      <div className="mx-auto flex h-14 max-w-6xl items-center justify-between rounded-full border border-line bg-nav-fill/85 px-5 backdrop-blur-xl shadow-nav">
        <div className="flex items-center space-x-3">
          <a href={basePath} className="flex items-center space-x-2.5 group">
            <img
              src={iconSrc}
              alt="Ferryx Logo"
              className="h-7 w-7 rounded-lg transition-transform group-hover:scale-105"
            />
            <span className="text-[15px] font-medium tracking-[-0.02em] text-ink">
              Ferryx
            </span>
          </a>
          <Badge variant="secondary" className="hidden lg:inline-flex text-[10px] px-2 py-0.5 border-line bg-surface text-ink-soft">
            v0.1.0-alpha
          </Badge>
        </div>

        <nav className="hidden md:flex items-center gap-7 text-[14px] font-medium text-ink-soft">
          <a href={`${basePath}docs/introduction/`} className="hover:text-ink transition-colors">
            Docs
          </a>
          <a href={`${basePath}diagnostic/`} className="hover:text-ink transition-colors">
            Diagnostic
          </a>
          <a href={`${basePath}#features`} className="hover:text-ink transition-colors">
            Features
          </a>
          <a href={`${basePath}#architecture`} className="hover:text-ink transition-colors">
            Architecture
          </a>
        </nav>

        <div className="flex items-center space-x-2">
          <div className="hidden sm:block">
            <ThemeToggle />
          </div>
          <a
            href={DISCORD_INVITE_URL}
            target="_blank"
            rel="noreferrer"
            aria-label="Join the Ferryx Discord"
            className="hidden sm:block text-ink-soft hover:text-ink transition-colors"
          >
            <Button variant="ghost" size="icon" className="h-9 w-9 rounded-full text-ink-soft hover:text-ink hover:bg-ink/[0.04]">
              <DiscordIcon className="h-4 w-4" />
            </Button>
          </a>
          <a
            href="https://github.com/Indosaram/ferryx"
            target="_blank"
            rel="noreferrer"
            className="hidden sm:block text-ink-soft hover:text-ink transition-colors"
          >
            <Button variant="ghost" size="icon" className="h-9 w-9 rounded-full text-ink-soft hover:text-ink hover:bg-ink/[0.04]">
              <Github className="h-4 w-4" />
            </Button>
          </a>
          <DownloadMenu variant="navbar" />
        </div>
      </div>
    </header>
  );
}
