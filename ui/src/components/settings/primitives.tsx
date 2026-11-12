import type { ReactNode } from "react";

import { cn } from "../../lib/cn";

/**
 * The settings surface is a page of groups, and a group is a card whose rows are parted by
 * hairlines: a title and a sentence, then label-left / control-right rows with no box of
 * their own. These primitives own that rhythm so every section reads the same.
 */

export function SettingsHeading({
  icon,
  title,
  description,
}: {
  icon: ReactNode;
  title: string;
  description: string;
}) {
  return (
    <header className="mb-9">
      <div className="flex items-center gap-2.5">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-panel text-muted-foreground [&>svg]:size-4">
          {icon}
        </span>
        <div className="text-[17px] font-semibold leading-6 tracking-[-0.01em] text-foreground">{title}</div>
      </div>
      <p className="mt-2.5 max-w-[62ch] text-[12px] leading-5 text-muted-foreground">{description}</p>
    </header>
  );
}

export function SettingsCard({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn("overflow-hidden rounded-lg border border-border bg-panel", className)}
    >
      {children}
    </div>
  );
}

export function SettingsGroup({
  title,
  action,
  description,
  children,
}: {
  title?: string;
  action?: ReactNode;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section className="mb-8">
      {title || action || description ? (
        <div className="mb-2 flex items-end justify-between gap-3 px-0.5">
          <div className="min-w-0">
            {title ? (
              <h3 className="text-[11px] font-semibold uppercase tracking-[0.09em] text-muted-foreground/75">
                {title}
              </h3>
            ) : null}
            {description ? (
              <p className="mt-1 text-[11px] leading-4 text-muted-foreground">{description}</p>
            ) : null}
          </div>
          {action ? <div className="flex shrink-0 items-center gap-2 pb-0.5">{action}</div> : null}
        </div>
      ) : null}
      <SettingsCard>{children}</SettingsCard>
    </section>
  );
}

export function SettingRow({
  label,
  title,
  description,
  children,
}: {
  label?: string;
  title?: string;
  description?: ReactNode;
  children: ReactNode;
}) {
  const heading = label ?? title ?? "";
  return (
    <div className="flex min-h-[54px] items-center justify-between gap-4 border-b border-border/60 px-4 py-3 last:border-b-0">
      <div className="min-w-0 max-w-[54ch] space-y-1">
        <div className="text-[13px] font-medium leading-5 text-foreground">{heading}</div>
        {description ? (
          <p className="text-[11px] leading-[1.5] text-muted-foreground">{description}</p>
        ) : null}
      </div>
      <div className="flex shrink-0 items-center gap-2">{children}</div>
    </div>
  );
}

/**
 * A card row whose control needs the full width under its label (a theme strip, a URL field,
 * a list). Same padding and hairline as SettingRow so the two mix inside one card.
 */
export function SettingBlock({
  label,
  description,
  children,
}: {
  label: string;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="border-b border-border/60 px-4 py-3.5 last:border-b-0">
      <div className="text-[13px] font-medium leading-5 text-foreground">{label}</div>
      {description ? (
        <p className="mt-1 max-w-[62ch] text-[11px] leading-[1.5] text-muted-foreground">{description}</p>
      ) : null}
      <div className="mt-3">{children}</div>
    </div>
  );
}
