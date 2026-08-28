import type { ReactNode } from "react";

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
    <header className="mb-8 border-b border-border pb-5">
      <div className="mb-2 flex items-center gap-2 text-[15px] font-semibold">
        <span className="text-muted-foreground [&>svg]:size-4">{icon}</span>
        {title}
      </div>
      <p className="max-w-2xl text-[12px] leading-5 text-muted-foreground">{description}</p>
    </header>
  );
}

export function SettingsGroup({
  title,
  action,
  description,
  children,
}: {
  title: string;
  action?: ReactNode;
  description?: string;
  children: ReactNode;
}) {
  return (
    <div className="mb-6">
      <div className="mb-4 flex items-center justify-between border-b border-border pb-3">
        <div>
          <h3 className="text-[13px] font-semibold text-foreground">{title}</h3>
          {description ? (
            <p className="mt-0.5 text-[11px] text-muted-foreground">{description}</p>
          ) : null}
        </div>
        {action ? <div className="flex items-center gap-2">{action}</div> : null}
      </div>
      {children}
    </div>
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
    <div className="flex items-start justify-between gap-4 py-3 border-b border-border/40 last:border-0">
      <div className="space-y-0.5 max-w-[480px]">
        <div className="text-[13px] font-medium text-foreground">{heading}</div>
        {description ? <p className="text-[11px] text-muted-foreground">{description}</p> : null}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}
