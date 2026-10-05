import {
  AgentsVisual,
  GhosttyVisual,
  MobileVisual,
  PersistenceVisual,
  SplitVisual,
  ZeroElectronVisual,
} from "@/components/FeatureVisuals";
import type { LandingLocale } from "@/lib/locale";

export function Features({ locale, basePath }: { locale: LandingLocale; basePath: string }) {
  const visuals = [GhosttyVisual, AgentsVisual, SplitVisual, MobileVisual, ZeroElectronVisual, PersistenceVisual];

  return (
    <section id="features" className="py-24 sm:py-28 border-t border-line bg-page-raised relative">
      <div className="mx-auto max-w-6xl px-4 sm:px-6">
        <div className="text-center max-w-2xl mx-auto mb-16 sm:mb-20">
          <p className="text-[11px] font-medium uppercase tracking-[0.14em] text-ink-faint mb-3">
            {locale.features.eyebrow}
          </p>
          <h2 className="text-[clamp(2rem,4vw,3rem)] font-medium tracking-[-0.035em] leading-[1.05] text-ink">
            {locale.features.title}
          </h2>
          <p className="mt-4 text-[15px] leading-relaxed text-ink-soft">
            {locale.features.intro}
          </p>
        </div>

        <div className="flex flex-col gap-4">
          {locale.features.cards.map((feature, idx) => {
            const Visual = visuals[idx];
            const flip = idx % 2 === 1;
            return (
              <div
                key={feature.title}
                className="grid grid-cols-1 items-center gap-8 rounded-2xl border border-line bg-surface p-8 shadow-card transition-colors hover:border-line-strong sm:p-10 lg:grid-cols-2 lg:gap-12"
              >
                <div className={flip ? "lg:order-2" : undefined}>
                  <p className="text-[11px] font-medium uppercase tracking-[0.14em] text-ink-faint mb-3">
                    {feature.eyebrow}
                  </p>
                  <h3 className="text-[clamp(1.375rem,2.2vw,1.875rem)] font-medium tracking-[-0.025em] leading-[1.15] text-ink">
                    {feature.title}
                  </h3>
                  <p className="mt-4 text-[16px] leading-relaxed text-ink-soft">
                    {feature.description}
                  </p>
                  {feature.link && (
                    <a href={`${basePath}docs/architecture/`} className="mt-4 inline-flex items-center text-[14px] font-medium text-ink hover:text-ink-soft transition-colors">{feature.link}</a>
                  )}
                  <ul className="mt-6 flex flex-col gap-2.5">
                    {feature.points.map((point) => (
                      <li key={point} className="flex items-center gap-2.5 text-[14px] text-ink-soft">
                        <span className="h-1 w-1 shrink-0 rounded-full bg-ink-faint" />
                        {point}
                      </li>
                    ))}
                  </ul>
                </div>
                <div className={flip ? "lg:order-1" : undefined}>
                  <Visual />
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
