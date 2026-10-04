import { Github } from "lucide-react";
import { DiscordIcon } from "@/components/ui/PlatformIcons";
import { DownloadMenu } from "@/components/DownloadMenu";
import { DISCORD_INVITE_URL, GITHUB_REPO_URL } from "@/lib/links";

export function Hero({ basePath }: { readonly basePath: string }) {
  return (
    <section className="relative z-20 pt-36 pb-0 text-center">
      <div className="absolute inset-0 bg-[radial-gradient(60%_50%_at_50%_0%,rgb(var(--ink-rgb)/0.05),transparent)] pointer-events-none" />

      <div className="relative mx-auto max-w-6xl px-4 sm:px-6 text-center">
        <h1 className="text-[clamp(2.75rem,7vw,5.25rem)] font-medium tracking-[-0.045em] leading-[0.95] text-ink max-w-4xl mx-auto">
          Run your coding agents in parallel.
          <br />
          Know which one needs you.
        </h1>

        {/* The server-rendered sentence above the fold: which agents, what keeps
            them from colliding, and that the work outlives the window. */}
        <p className="mx-auto mt-7 max-w-2xl text-[17px] leading-relaxed text-ink-soft">Ferryx runs Claude Code, Codex, and Gemini CLI at once, each in its own Git worktree so they never edit the same files. Sessions survive closing the window, and you can check in from your phone.</p>

        <div className="mt-10 flex flex-col sm:flex-row items-center justify-center gap-4">
          <DownloadMenu variant="hero" />
          <a href="https://github.com/Indosaram/ferryx" target="_blank" rel="noreferrer" className="inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-full border bg-surface text-ink border-line hover:border-line-strong hover:bg-page-raised hover:text-ink px-6 py-3 text-base font-medium transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30">
              <Github className="mr-2 h-5 w-5" />
              View on GitHub
          </a>
          <a href={DISCORD_INVITE_URL} target="_blank" rel="noreferrer" className="inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-full border bg-surface text-ink border-line hover:border-line-strong hover:bg-page-raised hover:text-ink px-6 py-3 text-base font-medium transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ink/30">
              <DiscordIcon className="mr-2 h-5 w-5" />
              Join Discord
          </a>
        </div>

        <div className="mt-6 flex flex-wrap items-center justify-center gap-2">
          <span className="rounded-full border border-line bg-surface px-3 py-1 text-[13px] text-ink-soft">Any CLI agent</span>
          <span className="rounded-full border border-line bg-surface px-3 py-1 text-[13px] text-ink-soft">macOS · Windows · Linux</span>
          <span className="rounded-full border border-line bg-surface px-3 py-1 text-[13px] text-ink-soft">Free for personal use</span>
        </div>

        <p className="mt-6 text-[13px] leading-relaxed text-ink-faint">
          Source-available under{" "}
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
            product facts
          </a>
        </p>
      </div>
    </section>
  );
}
