// The supervisor setup banner both surfaces render when setup needs the person (needs setup or
// blocked; never while unknown, ready or working): what is missing and the one action that
// finishes it. Transport-free: the desktop passes IPC actions, the phone its HTTP actions, and a
// view-only device passes none and sees the text only.
import { AlertCircle, Sparkles, X } from "lucide-react";
import { useEffect, useState } from "react";

import { cn } from "../../lib/cn";
import {
  actionFor,
  adoptSummary,
  dismissSetupKind,
  loadDismissedSetupKinds,
  readiness,
  SETUP_BANNER_DISMISSED_EVENT,
  setupNeedsAttention,
  type SetupActions,
  type SetupNextAction,
  type SetupProject,
  type SetupStatus,
} from "../../lib/supervisorSetupModel";

export const MEMORY_IMPORT_LABEL = "Learn from my recent Claude, Codex and Ferryx sessions (only what I typed)";

export type SupervisorSetupBannerProps = {
  readonly status: SetupStatus | null;
  readonly surface: "desktop" | "phone";
  /** Null: this device may not change setup (a view-only phone), so only the text shows. */
  readonly actions: SetupActions | null;
  /** Local projects mapped by "Turn on"; the phone does not know them and passes none. */
  readonly projects?: readonly SetupProject[];
  /** Which computer, when the surface shows more than one. */
  readonly hostLabel?: string;
  /** The run button's label; defaults to the next action's own wording. */
  readonly runLabel?: string;
  /** Offer "Not now", remembered per next action. */
  readonly dismissible?: boolean;
  readonly className?: string;
};

type Notice = { tone: "error" | "info"; text: string };

function runButtonLabel(kind: SetupNextAction): string {
  return kind === "enable_engine" ? "Turn on supervisor" : "Finish setup";
}

function useDismissedKinds(): readonly SetupNextAction[] {
  const [kinds, setKinds] = useState<readonly SetupNextAction[]>(() => loadDismissedSetupKinds());
  useEffect(() => {
    const reload = () => setKinds(loadDismissedSetupKinds());
    window.addEventListener(SETUP_BANNER_DISMISSED_EVENT, reload);
    return () => window.removeEventListener(SETUP_BANNER_DISMISSED_EVENT, reload);
  }, []);
  return kinds;
}

export function SupervisorSetupBanner({
  status,
  surface,
  actions,
  projects = [],
  hostLabel,
  runLabel,
  dismissible = false,
  className,
}: SupervisorSetupBannerProps) {
  const touch = surface === "phone";
  const dismissed = useDismissedKinds();
  const [importMemory, setImportMemory] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const kind = status?.nextAction ?? null;
  // A notice belongs to the step it answered; a new step starts clean.
  useEffect(() => setNotice(null), [kind]);

  if (!status || !setupNeedsAttention(status)) return null;
  if (dismissible && dismissed.includes(status.nextAction)) return null;

  const action = actionFor(status);
  const blocked = readiness(status) === "blocked";

  const run = async () => {
    if (!actions || busy) return;
    setBusy(true);
    setNotice(null);
    if (action.primary === "adopt") {
      const result = await actions.adopt();
      setNotice(result.kind === "ok" ? { tone: "info", text: adoptSummary(result) } : { tone: "error", text: `${result.code}: ${result.message}` });
    } else {
      const result = await actions.run({ importMemory, projects: [...projects] });
      if (result.kind === "error") setNotice({ tone: "error", text: `${result.code}: ${result.message}` });
    }
    setBusy(false);
  };

  const primaryLabel =
    action.primary === "adopt"
      ? `Adopt ${status.adoptable} earlier job${status.adoptable === 1 ? "" : "s"}`
      : (runLabel ?? runButtonLabel(status.nextAction));
  const Icon = blocked ? AlertCircle : Sparkles;

  return (
    <section
      data-testid="supervisor-setup-banner"
      data-kind={status.nextAction}
      data-readiness={readiness(status)}
      aria-label="Supervisor setup"
      role={blocked ? "alert" : "status"}
      className={cn(
        "flex min-w-0 flex-col gap-2 rounded-md border px-3 py-2.5 leading-snug",
        blocked ? "border-destructive/40 bg-destructive/10" : "border-border bg-card",
        touch ? "rounded-xl text-[14px]" : "text-[12px]",
        className,
      )}
    >
      <div className="flex min-w-0 items-start gap-2">
        <Icon aria-hidden="true" className={cn("mt-0.5 shrink-0", touch ? "size-4" : "size-3.5", blocked ? "text-destructive" : "text-muted-foreground")} />
        <div className="min-w-0 flex-1">
          <p data-testid="supervisor-setup-label" className="font-medium text-foreground">
            {action.label}
            {hostLabel ? <span className="font-normal text-muted-foreground"> on {hostLabel}</span> : null}
          </p>
          <p data-testid="supervisor-setup-description" className="mt-0.5 break-words text-muted-foreground">
            {action.description}
          </p>
        </div>
        {dismissible ? (
          <button
            type="button"
            data-testid="supervisor-setup-dismiss"
            aria-label="Not now"
            title="Not now"
            onClick={() => dismissSetupKind(status.nextAction)}
            className={cn(
              "flex shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
              touch ? "size-11 -my-2 -mr-2" : "size-5",
            )}
          >
            <X aria-hidden="true" className={touch ? "size-4" : "size-3.5"} />
          </button>
        ) : null}
      </div>

      {actions && action.primary ? (
        <div className={cn("flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2", touch ? "pl-6" : "pl-[22px]")}>
          {action.primary === "run_setup" ? (
            <label className="flex min-w-0 flex-1 basis-56 items-start gap-2 text-muted-foreground">
              <input
                type="checkbox"
                data-testid="supervisor-setup-import-memory"
                checked={importMemory}
                onChange={(event) => setImportMemory(event.target.checked)}
                className={cn("mt-0.5 shrink-0 accent-primary", touch ? "size-5" : "size-3.5")}
              />
              <span className="min-w-0">{MEMORY_IMPORT_LABEL}</span>
            </label>
          ) : null}
          <button
            type="button"
            data-testid="supervisor-setup-primary"
            aria-busy={busy || undefined}
            disabled={busy}
            onClick={() => void run()}
            className={cn(
              "ml-auto shrink-0 rounded-md bg-primary px-3 font-medium text-primary-foreground transition-colors hover:bg-primary/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50",
              touch ? "h-11 text-[14px]" : "h-7 text-[12px]",
            )}
          >
            {primaryLabel}
          </button>
        </div>
      ) : null}

      {!actions && action.primary ? (
        <p data-testid="supervisor-setup-view-only" className={cn("text-muted-foreground", touch ? "pl-6" : "pl-[22px]")}>
          Finish it on that computer, or from a device that can type.
        </p>
      ) : null}

      {notice ? (
        <p
          data-testid="supervisor-setup-notice"
          data-tone={notice.tone}
          role={notice.tone === "error" ? "alert" : undefined}
          className={cn("break-words", touch ? "pl-6" : "pl-[22px]", notice.tone === "error" ? "text-destructive" : "text-muted-foreground")}
        >
          {notice.text}
        </p>
      ) : null}
    </section>
  );
}
