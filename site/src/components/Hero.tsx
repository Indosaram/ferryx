import { Github } from "lucide-react";
import { DiscordIcon } from "@/components/ui/PlatformIcons";
import { DownloadMenu } from "@/components/DownloadMenu";
import { DISCORD_INVITE_URL, GITHUB_REPO_URL } from "@/lib/links";
import type { LandingLocale } from "@/lib/locale";

export function Hero({ basePath, locale }: { readonly basePath: string; readonly locale: LandingLocale }) {
  return (
    <section className="relative z-20 pt-36 pb-0 text-center">
      <div className="absolute inset-0 bg-[radial-gradient(60%_50%_at_50%_0%,rgb(var(--ink-rgb)/0.05),transparent)] pointer-events-none" />

      <div className="relative mx-auto max-w-6xl px-4 sm:px-6 text-center">
        <h1 className="text-[clamp(2.75rem,7vw,5.25rem)] font-medium tracking-[-0.045em] leading-[0.95] text-ink max-w-4xl mx-auto">
          {locale.hero.title}
          <br />
          {locale.hero.titleEnd}
        </h1>

        {/* The server-rendered sentence above the fold: which agents, what keeps
            them from colliding, and that the work outlives the window. */}
        <p className="mx-auto mt-7 max-w-2xl text-[17px] leading-relaxed text-ink-soft">{locale.hero.subtitle}</p>

        <div className="mt-10 flex flex-col sm:flex-row items-center justify-center gap-4">
          <DownloadMenu variant="hero" locale={locale} />
          <a href="https://github.com/Indosaram/ferryx" target="_blank" rel="noreferrer" className="inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-full border bg-surface text-ink border-line hover:border-line-strong hover:bg-page-raised hover:text-ink px-6 py-3 text-base font-medium transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30">
              <Github className="mr-2 h-5 w-5" />
              {locale.hero.github}
          </a>
          <a href={DISCORD_INVITE_URL} target="_blank" rel="noreferrer" className="inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-full border bg-surface text-ink border-line hover:border-line-strong hover:bg-page-raised hover:text-ink px-6 py-3 text-base font-medium transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30">
              <DiscordIcon className="mr-2 h-5 w-5" />
              {locale.hero.discord}
          </a>
        </div>

        <p className="mt-6 text-[13px] leading-relaxed text-ink-faint">
            {locale.hero.license}{" "}
          <a
            href={`${GITHUB_REPO_URL}/blob/main/LICENSE`}
            target="_blank"
            rel="noreferrer"
            className="underline underline-offset-4 hover:text-ink transition-colors"
          >
            SUL-1.0
          </a>{" "}
          &middot;{" "}
          <a
            href={`${basePath}docs/facts/`}
            className="underline underline-offset-4 hover:text-ink transition-colors"
          >
            {locale.hero.facts}
          </a>
        </p>
      </div>
    </section>
  );
}
