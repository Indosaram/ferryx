import { Github } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import { DiscordIcon } from "@/components/ui/PlatformIcons";
import { DownloadMenu } from "@/components/DownloadMenu";
import { ThemeToggle } from "@/components/ThemeToggle";
import { DISCORD_INVITE_URL, GITHUB_REPO_URL } from "@/lib/links";
// The navbar paints this at 28 CSS px; the 1024x1024 source is 806 KB and was the single
// largest resource on the landing page, loaded eagerly because the navbar is client:load.
// 96px covers up to a 3x display at this size.
import ferryxIcon from "@/assets/ferryx-icon-96.png";

const iconSrc = typeof ferryxIcon === 'object' && ferryxIcon !== null && 'src' in ferryxIcon ? (ferryxIcon as { src: string }).src : String(ferryxIcon);

export function Navbar({ basePath }: { basePath: string }) {
  return (
    <header className="fixed top-0 inset-x-0 z-50 px-4 pt-4">
      {/* The pill is a single flex row, so the brand must be the only shrinking track:
          without min-w-0 + truncate the wordmark keeps its min-content width, overflows
          its own track, and paints underneath the control cluster on narrow phones. */}
      <div className="mx-auto flex h-14 max-w-6xl items-center justify-between gap-2 sm:gap-3 rounded-full border border-line bg-nav-fill/85 px-3 sm:px-5 backdrop-blur-xl shadow-nav">
        <div className="flex min-w-0 items-center gap-2 sm:gap-3">
          <a href={basePath} className="flex min-w-0 items-center gap-2 sm:gap-2.5 group">
            <img
              src={iconSrc}
              alt="Ferryx Logo"
              width={28}
              height={28}
              className="h-7 w-7 shrink-0 rounded-lg transition-transform group-hover:scale-105"
            />
            <span className="truncate text-[15px] font-medium tracking-[-0.02em] text-ink">
              Ferryx
            </span>
          </a>
          {/* No version string here: the site cannot know which release is current, so it
              links to whatever GitHub currently serves as latest. */}
          <a
            href={`${GITHUB_REPO_URL}/releases/latest`}
            target="_blank"
            rel="noreferrer"
            className="hidden lg:inline-flex shrink-0"
          >
            <Badge variant="secondary" className="text-[10px] px-2 py-0.5 border-line bg-surface text-ink-soft hover:text-ink">
              Latest release
            </Badge>
          </a>
        </div>

        <nav className="hidden md:flex items-center gap-7 text-[14px] font-medium text-ink-soft">
          <a href={`${basePath}docs/introduction/`} className="hover:text-ink transition-colors">
            Docs
          </a>
          <a href="#features" className="hover:text-ink transition-colors">
            Features
          </a>
          <a href="#architecture" className="hover:text-ink transition-colors">
            Architecture
          </a>
        </nav>

        {/* Download and theme stay at every width; Discord and GitHub fold away below sm,
            where they would push the download control past the viewport edge. Both keep a
            persistent home in the footer. */}
        <div className="flex shrink-0 items-center gap-0.5 sm:gap-2">
          <ThemeToggle />
          <a
            href={DISCORD_INVITE_URL}
            target="_blank"
            rel="noreferrer"
            aria-label="Join the Ferryx Discord"
            className="hidden sm:inline-flex text-ink-soft hover:text-ink transition-colors"
          >
            <Button variant="ghost" size="icon" className="h-9 w-9 rounded-full text-ink-soft hover:text-ink hover:bg-ink/[0.04]">
              <DiscordIcon className="h-4 w-4" />
            </Button>
          </a>
          <a
            href={GITHUB_REPO_URL}
            target="_blank"
            rel="noreferrer"
            aria-label="Ferryx on GitHub"
            className="hidden sm:inline-flex text-ink-soft hover:text-ink transition-colors"
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
