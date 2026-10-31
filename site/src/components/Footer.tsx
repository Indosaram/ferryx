import { Github } from "lucide-react";
import { DiscordIcon } from "@/components/ui/PlatformIcons";
import { DISCORD_INVITE_URL } from "@/lib/links";
// Rendered at 24 CSS px. Shares the navbar's 96px asset so the landing page never fetches
// the 806 KB 1024x1024 original, which Starlight still uses for its own logo slot.
import ferryxIcon from "@/assets/ferryx-icon-96.png";
import type { LandingLocale } from "@/lib/locale";

const iconSrc = typeof ferryxIcon === 'object' && ferryxIcon !== null && 'src' in ferryxIcon ? (ferryxIcon as { src: string }).src : String(ferryxIcon);

export function Footer({ basePath, locale }: { readonly basePath: string; readonly locale: LandingLocale }) {
  return (
    <footer className="border-t border-line bg-page py-12 text-[13px] text-ink-faint">
      <div className="mx-auto max-w-6xl px-4 sm:px-6 flex flex-col sm:flex-row items-center justify-between gap-6">
        <div className="flex items-center space-x-3">
          <picture className="block h-6 w-6 shrink-0">
            <source srcSet={`${basePath}ferryx-icon-64.avif`} type="image/avif" />
            <source srcSet={`${basePath}ferryx-icon-64.webp`} type="image/webp" />
            <img src={iconSrc} alt="Ferryx" width={24} height={24} className="h-6 w-6 rounded-md" />
          </picture>
          <span className="font-medium text-ink tracking-tight">Ferryx</span>
          <span className="text-line-strong" aria-hidden="true">|</span>
          <span>{locale.footer.tagline}</span>
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
          <a href={`${locale.basePath}#features`} className="hover:text-ink transition-colors">{locale.footer.features}</a>
          <a
            href={`${basePath}docs/architecture/`}
            className="hover:text-ink transition-colors"
          >
            {locale.footer.architecture}
          </a>
          <a
            href={`${basePath}use-cases/parallel-ai-agents/`}
            className="hover:text-ink transition-colors"
          >
            {locale.footer.useCases}
          </a>
          <a
            href={`${basePath}compare/`}
            className="hover:text-ink transition-colors"
          >
            {locale.footer.compare}
          </a>
          <a
            href={`${basePath}privacy/`}
            className="text-ink-soft hover:text-ink transition-colors whitespace-nowrap"
          >
            {locale.footer.privacy}
          </a>
        </div>

        <div>
          <p>© {new Date().getFullYear()} {locale.footer.copyright}</p>
        </div>
      </div>
    </footer>
  );
}

