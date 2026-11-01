import React from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "../components/ui/button";
import { isIosCompanion } from "../lib/clientRuntime";
import { handleExternalLinkClick } from "../lib/externalLinks";
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

const DATE_FORMAT = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

function formatUnixSeconds(value: number): string {
  return DATE_FORMAT.format(new Date(value * 1000));
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
  /** Leaves a limited account; without it the notice would be a dead end. */
  onSignOut?: () => void;
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
  onSignOut,
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
      aria-labelledby="remote-plan-limit-title"
      className="flex min-h-0 min-w-0 flex-1 flex-col items-center justify-center gap-3 overflow-y-auto bg-chat-screen px-4 py-6 text-center text-chat-foreground"
    >
      <AlertTriangle aria-hidden="true" className="size-6 shrink-0 text-status-warning" />
      <div className="flex w-full max-w-sm min-w-0 flex-col items-center gap-2">
        <h2
          id="remote-plan-limit-title"
          role="alert"
          className="text-base font-semibold tracking-tight text-chat-foreground"
        >
          {suspended ? "Remote access suspended" : "Plan limit reached"}
        </h2>
        <p className="min-w-0 break-words text-sm leading-relaxed text-chat-foreground-secondary">
          {description}
        </p>
        {limit !== undefined || used !== undefined ? (
          <p
            data-testid="remote-plan-limit-usage"
            data-limit={limit}
            data-used={used}
            className="min-w-0 break-words text-sm text-chat-foreground-secondary"
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
            className="min-w-0 break-words text-sm text-chat-foreground-secondary"
          >
            {suspended ? "Grace period ended" : "Grace period ends"}{" "}
            {formatUnixSeconds(graceEndsAt)}
          </time>
        ) : null}
        {stoppedAt !== undefined ? (
          <time
            data-testid="remote-plan-limit-stopped"
            dateTime={unixSecondsToIso(stoppedAt)}
            className="min-w-0 break-words text-sm text-chat-foreground-secondary"
          >
            Remote access stopped {formatUnixSeconds(stoppedAt)}
          </time>
        ) : null}
      </div>
      <p
        data-testid="remote-plan-limit-coming-soon"
        className="min-w-0 max-w-sm break-words text-sm text-chat-foreground-secondary"
      >
        Paid plans with more computers are coming soon.
        {/* The iOS companion shows no outbound purchase link (App Store guideline 3.1.1). */}
        {isIosCompanion() ? null : (
          <>
            {" "}
            <a
              data-testid="remote-plan-limit-pricing"
              href={pricingUrl}
              target="_blank"
              rel="noreferrer"
              onClick={handleExternalLinkClick}
              className="inline-flex min-h-11 items-center underline underline-offset-2 hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              See planned pricing
            </a>
          </>
        )}
      </p>
      <div className="flex flex-wrap items-center justify-center gap-2" data-testid="remote-plan-limit-actions">
        {onRetry ? (
          <Button
            type="button"
            data-testid="remote-plan-limit-retry"
            className="h-11 px-4 text-sm"
            onClick={onRetry}
          >
            Retry connection
          </Button>
        ) : null}
        {onSignOut ? (
          <Button
            type="button"
            variant="outline"
            data-testid="remote-plan-limit-sign-out"
            className="h-11 border-chat-border bg-transparent px-4 text-sm text-chat-foreground hover:bg-chat-surface-hover hover:text-chat-foreground"
            onClick={onSignOut}
          >
            Sign out
          </Button>
        ) : null}
      </div>
    </div>
  );
};
