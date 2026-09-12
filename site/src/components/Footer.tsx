import { Github } from "lucide-react";
import { DiscordIcon } from "@/components/ui/PlatformIcons";
import { DISCORD_INVITE_URL } from "@/lib/links";
import ferryxIcon from "@/assets/ferryx-icon.png";

const iconSrc = typeof ferryxIcon === 'object' && ferryxIcon !== null && 'src' in ferryxIcon ? (ferryxIcon as { src: string }).src : String(ferryxIcon);

export function Footer({ basePath }: { readonly basePath: string }) {
  return (
    <footer className="border-t border-line bg-page py-12 text-[13px] text-ink-faint">
      <div className="mx-auto max-w-6xl px-4 sm:px-6 flex flex-col sm:flex-row items-center justify-between gap-6">
        <div className="flex items-center space-x-3">
          <img src={iconSrc} alt="Ferryx" width={24} height={24} className="h-6 w-6 rounded-md" />
          <span className="font-medium text-ink tracking-tight">Ferryx</span>
          <span className="text-line-strong">|</span>
          <span>Ultra-lightweight Rust Native AI Workspace</span>
        </div>

        <div className="flex flex-wrap items-center justify-center gap-x-6 gap-y-3">
          <a
            href="https://github.com/Indosaram/ferryx"
            target="_blank"
            rel="noreferrer"
            className="hover:text-ink transition-colors flex items-center gap-1"
          >
            <Github className="h-3.5 w-3.5" />
            GitHub
          </a>
          <a
            href={DISCORD_INVITE_URL}
            target="_blank"
            rel="noreferrer"
            className="hover:text-ink transition-colors flex items-center gap-1"
          >
            <DiscordIcon className="h-3.5 w-3.5" />
            Discord
          </a>
          <a
            href="#features"
            className="hover:text-ink transition-colors"
          >
            Features
          </a>
          <a
            href={`${basePath}docs/architecture/`}
            className="hover:text-ink transition-colors"
          >
            Architecture Docs
          </a>
          <a
            href={`${basePath}use-cases/parallel-ai-agents/`}
            className="hover:text-ink transition-colors"
          >
            Use Cases
          </a>
          <a
            href={`${basePath}compare/`}
            className="hover:text-ink transition-colors"
          >
            Compare
          </a>
          <a
            href={`${basePath}privacy/`}
            className="text-ink-soft hover:text-ink transition-colors whitespace-nowrap"
          >
            Privacy Policy
          </a>
        </div>

        <div>
          <p>© {new Date().getFullYear()} Ferryx Contributors. All rights reserved.</p>
        </div>
      </div>
    </footer>
  );
}

