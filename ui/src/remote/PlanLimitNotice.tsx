import React from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "../components/ui/button";
import { REMOTE_SUSPENDED, type PlanLimitState } from "./accountSession";

export const DEFAULT_PRICING_URL = "https://ferryx.dev/docs/pricing/";

const PLAN_LABELS: Record<string, string> = {
  free: "Free",
  pro_monthly: "Pro (monthly)",
  pro_annual: "Pro (annual)",
  team_monthly: "Team (monthly)",
  team_annual: "Team (annual)",
};

function planLabel(plan: string | undefined): string | null {
  if (!plan) return null;
  return PLAN_LABELS[plan] ?? plan;
}

function unixSecondsToIso(value: number): string {
  return new Date(value * 1000).toISOString();
}

function formatUnixSeconds(value: number): string {
  return new Date(value * 1000).toLocaleString();
}

/** Drops values `toISOString` would throw on, so a bad prop can never crash the render. */
function renderableUnixSeconds(value: number | undefined): number | undefined {
  if (value === undefined || !Number.isFinite(value)) return undefined;
  return Number.isFinite(new Date(value * 1000).getTime()) ? value : undefined;
}

export interface PlanLimitNoticeProps {
  state: PlanLimitState;
  pricingUrl?: string;
  onRetry?: () => void;
}

/**
 * Full-screen notice for accounts whose remote access is limited or stopped.
 * Renders only server-provided values: when a suspension arrives as a bare
 * WebSocket close reason the timestamps stay absent instead of being invented.
 */
export const PlanLimitNotice: React.FC<PlanLimitNoticeProps> = ({
  state,
  pricingUrl = DEFAULT_PRICING_URL,
  onRetry,
}) => {
  const suspended = state.code === REMOTE_SUSPENDED;
  const label = planLabel(state.plan);
  const { limit, used } = state;
  const graceEndsAt = renderableUnixSeconds(state.graceEndsAt);
  const stoppedAt = renderableUnixSeconds(state.stoppedAt);

  let description: string;
  if (suspended) {
    description = `Remote access is stopped for this account${
      label ? ` on the ${label} plan` : ""
    }. Your computer and its sessions are untouched; only the account connection is stopped.`;
  } else if (limit !== undefined && used !== undefined) {
    description = `This account is using ${used} of ${limit} computers included with the ${
      label ?? "current"
    } plan.`;
  } else if (label) {
    description = `This account reached the computer limit included with the ${label} plan.`;
  } else {
    description = "This account reached the computer limit included with its plan.";
  }

  return (
    <div
      data-testid="remote-plan-limit-notice"
      data-code={state.code}
      data-plan={state.plan ?? ""}
      role="alert"
      aria-labelledby="remote-plan-limit-title"
      className="flex min-h-0 min-w-0 flex-1 flex-col items-center justify-center gap-3 overflow-y-auto bg-chat-screen px-4 py-6 text-center text-chat-foreground"
    >
      <AlertTriangle aria-hidden="true" className="size-6 shrink-0 text-status-warning" />
      <div className="flex w-full max-w-sm min-w-0 flex-col items-center gap-2">
        <h2
          id="remote-plan-limit-title"
          className="text-sm font-semibold tracking-tight text-chat-foreground"
        >
          {suspended ? "Remote access suspended" : "Plan limit reached"}
        </h2>
        <p className="min-w-0 break-words text-xs leading-relaxed text-chat-foreground-secondary">
          {description}
        </p>
        {limit !== undefined || used !== undefined ? (
          <p
            data-testid="remote-plan-limit-usage"
            data-limit={limit}
            data-used={used}
            className="min-w-0 break-words text-xs text-chat-foreground-secondary"
          >
            {limit !== undefined && used !== undefined
              ? `${used} of ${limit} computers in use`
              : limit !== undefined
                ? `${limit} computers included`
                : `${used} computers in use`}
          </p>
        ) : null}
        {graceEndsAt !== undefined ? (
          <time
            data-testid="remote-plan-limit-grace-ends"
            dateTime={unixSecondsToIso(graceEndsAt)}
            className="min-w-0 break-words text-xs text-chat-foreground-secondary"
          >
            {suspended ? "Grace period ended" : "Grace period ends"}{" "}
            {formatUnixSeconds(graceEndsAt)}
          </time>
        ) : null}
        {stoppedAt !== undefined ? (
          <time
            data-testid="remote-plan-limit-stopped"
            dateTime={unixSecondsToIso(stoppedAt)}
            className="min-w-0 break-words text-xs text-chat-foreground-secondary"
          >
            Remote access stopped {formatUnixSeconds(stoppedAt)}
          </time>
        ) : null}
      </div>
      <div className="flex flex-wrap items-center justify-center gap-2" data-testid="remote-plan-limit-actions">
        <Button
          asChild
          size="sm"
          className="bg-chat-primary text-chat-primary-foreground hover:bg-chat-primary/90"
        >
          <a
            data-testid="remote-plan-limit-upgrade"
            href={pricingUrl}
            target="_blank"
            rel="noreferrer"
          >
            Upgrade plan
          </a>
        </Button>
        {onRetry ? (
          <Button
            type="button"
            variant="outline"
            size="sm"
            data-testid="remote-plan-limit-retry"
            className="border-chat-border bg-transparent text-chat-foreground hover:bg-chat-surface-hover hover:text-chat-foreground"
            onClick={onRetry}
          >
            Retry connection
          </Button>
        ) : null}
      </div>
    </div>
  );
};
