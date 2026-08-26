import { Github } from "lucide-react";
import { Button } from "@/components/ui/Button";
import { DiscordIcon } from "@/components/ui/PlatformIcons";
import { DownloadMenu } from "@/components/DownloadMenu";
import { DISCORD_INVITE_URL } from "@/lib/links";

export function Hero() {
  return (
    <section className="relative z-20 pt-36 pb-0 text-center">
      <div className="absolute inset-0 bg-[radial-gradient(60%_50%_at_50%_0%,rgb(var(--ink-rgb)/0.05),transparent)] pointer-events-none" />

      <div className="relative mx-auto max-w-6xl px-4 sm:px-6 text-center">
        <h1 className="text-[clamp(2.75rem,7vw,5.25rem)] font-medium tracking-[-0.045em] leading-[0.95] text-ink max-w-4xl mx-auto">
          Parallel agentic development.
          <br />
          Zero bloat.
        </h1>

        <div className="mt-10 flex flex-col sm:flex-row items-center justify-center gap-4">
          <DownloadMenu variant="hero" />
          <a href="https://github.com/Indosaram/ferryx" target="_blank" rel="noreferrer">
            <Button
              variant="outline"
              size="lg"
              className="rounded-full bg-surface text-ink border-line hover:border-line-strong hover:bg-page-raised hover:text-ink px-6 py-3 h-auto text-base font-medium transition-colors duration-150"
            >
              <Github className="mr-2 h-5 w-5" />
              View on GitHub
            </Button>
          </a>
          <a href={DISCORD_INVITE_URL} target="_blank" rel="noreferrer">
            <Button
              variant="outline"
              size="lg"
              className="rounded-full bg-surface text-ink border-line hover:border-line-strong hover:bg-page-raised hover:text-ink px-6 py-3 h-auto text-base font-medium transition-colors duration-150"
            >
              <DiscordIcon className="mr-2 h-5 w-5" />
              Join Discord
            </Button>
          </a>
        </div>
      </div>
    </section>
  );
}
