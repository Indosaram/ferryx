import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, CreditCard, ExternalLink, Trash2, UserPlus } from "lucide-react";

import { openExternalUrl } from "../../lib/browserTauri";
import {
  BillingApiError,
  fetchBillingEntitlement,
  inviteOrgMember,
  listOrgMembers,
  planLimitDetails,
  removeOrgMember,
  startBillingCheckout,
  suspensionDetails,
  toBillingApiError,
  updateBillingQuantity,
  type BillingEntitlement,
  type BillingPlan,
  type BillingQuantityChange,
  type CheckoutPlan,
  type EntitlementStatus,
  type OrgInvite,
  type OrgMember,
} from "../../lib/billingApi";
import { Alert, AlertDescription } from "../ui/alert";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Input } from "../ui/input";
import { Progress } from "../ui/progress";
import { SettingRow, SettingsHeading } from "./primitives";

const DAY_MS = 86_400_000;
const TEAM_MIN_SEATS = 2;
const MAX_PACKS = 100;

type RenewalChangeKind = "hostPacks" | "seats";

interface RenewalNotice {
  readonly change: RenewalChangeKind;
  readonly limit: number;
}

const PLAN_LABELS: Record<BillingPlan, string> = {
  free: "Free",
  pro_monthly: "Pro · Monthly",
  pro_annual: "Pro · Yearly",
  team_monthly: "Team · Monthly",
  team_annual: "Team · Yearly",
};

const STATUS_LABELS: Record<EntitlementStatus, string> = {
  ok: "Active",
  over_limit: "Over limit",
  past_due: "Payment past due",
  stopped: "Remote stopped",
};

const PRO_CHOICES: readonly { readonly plan: CheckoutPlan; readonly label: string }[] = [
  { plan: "pro_monthly", label: "Pro monthly" },
  { plan: "pro_annual", label: "Pro yearly" },
];

const TEAM_CHOICES: readonly { readonly plan: CheckoutPlan; readonly label: string }[] = [
  { plan: "team_monthly", label: "Team monthly" },
  { plan: "team_annual", label: "Team yearly" },
];

function isTeamPlan(plan: BillingPlan): boolean {
  return plan === "team_monthly" || plan === "team_annual";
}

function canManagePlan(entitlement: BillingEntitlement): boolean {
  return entitlement.role === null || entitlement.role === "owner" || entitlement.role === "admin";
}

function clampSeats(value: number): number {
  if (!Number.isFinite(value)) return TEAM_MIN_SEATS;
  return Math.max(TEAM_MIN_SEATS, Math.floor(value));
}

function clampPacks(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.min(MAX_PACKS, Math.max(0, Math.floor(value)));
}

function daysLeft(graceEndsAt: number | null, now: number): number | null {
  if (graceEndsAt === null) return null;
  return Math.max(0, Math.ceil((graceEndsAt * 1000 - now) / DAY_MS));
}

function formatDeadline(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString();
}

function usagePercent(entitlement: BillingEntitlement): number {
  if (entitlement.machineLimit <= 0) return 100;
  return Math.min(100, Math.round((entitlement.machinesUsed / entitlement.machineLimit) * 100));
}

function errorText(error: BillingApiError): string {
  if (error.code === "PLAN_LIMIT_REACHED") {
    const details = planLimitDetails(error.details);
    if (details) {
      return `The ${details.plan ?? "current"} plan allows ${details.limit ?? "?"} machines and ${details.used ?? "?"} are registered. Remove a machine or upgrade to continue.`;
    }
  }
  if (error.code === "REMOTE_SUSPENDED") {
    const details = suspensionDetails(error.details);
    if (details?.graceEndsAt) {
      return `Remote access is stopped; the grace period ended ${formatDeadline(details.graceEndsAt)}.`;
    }
    return "Remote access is stopped for this account.";
  }
  if (error.code === "ORG_ROLE_REQUIRED") return "Only the team owner can do that.";
  if (error.code === "BILLING_UNCONFIGURED") {
    return "This account server has no billing configured. Contact the operator.";
  }
  if (error.code === "UNAUTHORIZED" || error.status === 401) {
    return "Your account session expired. Sign in again in Remote settings, then retry.";
  }
  return error.message;
}

export interface PlanSectionProps {
  accountOrigin?: string;
  accountSessionToken?: string | null;
  now?: number;
  onUnavailable?: () => void;
}

export function PlanSection({
  accountOrigin,
  accountSessionToken,
  now = Date.now(),
  onUnavailable,
}: PlanSectionProps) {
  const unavailableRef = useRef(onUnavailable);
  unavailableRef.current = onUnavailable;

  const call = useMemo(
    () => ({ accountOrigin, accountSessionToken }),
    [accountOrigin, accountSessionToken],
  );

  const [entitlement, setEntitlement] = useState<BillingEntitlement | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<BillingApiError | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [actionError, setActionError] = useState<BillingApiError | null>(null);
  const [busy, setBusy] = useState(false);
  const [members, setMembers] = useState<readonly OrgMember[] | null>(null);
  const [membersError, setMembersError] = useState<string | null>(null);
  const [packsDraft, setPacksDraft] = useState(0);
  const [seatsDraft, setSeatsDraft] = useState(TEAM_MIN_SEATS);
  const [inviteEmail, setInviteEmail] = useState("");
  const [inviteSent, setInviteSent] = useState<OrgInvite | null>(null);
  const [pendingRemoveId, setPendingRemoveId] = useState<string | null>(null);
  const [renewalNotice, setRenewalNotice] = useState<RenewalNotice | null>(null);

  const activeLoadRef = useRef<AbortController | null>(null);
  const accountGenerationRef = useRef(0);

  useEffect(() => {
    accountGenerationRef.current += 1;
    setLoading(true);
    setEntitlement(null);
    setLoadError(null);
    setActionError(null);
    setMembers(null);
    setMembersError(null);
    setInviteSent(null);
    setPendingRemoveId(null);
    setRenewalNotice(null);
    setBusy(false);
  }, [accountOrigin, accountSessionToken]);

  const load = useCallback(async () => {
    activeLoadRef.current?.abort();
    const controller = new AbortController();
    activeLoadRef.current = controller;
    const currentGeneration = accountGenerationRef.current;
    const isCurrent = () =>
      activeLoadRef.current === controller && accountGenerationRef.current === currentGeneration;
    setLoading(true);
    try {
      const next = await fetchBillingEntitlement({ ...call, signal: controller.signal });
      if (!isCurrent()) return;
      setEntitlement(next);
      setLoadError(null);
      setUnavailable(false);
    } catch (error) {
      if (!isCurrent()) return;
      const billingError = toBillingApiError(error);
      if (billingError.code === "REQUEST_ABORTED") return;
      if (billingError.status === 404) {
        setUnavailable(true);
        unavailableRef.current?.();
        return;
      }
      setLoadError(billingError);
    } finally {
      if (isCurrent()) setLoading(false);
    }
  }, [call]);

  useEffect(() => {
    void load();
    return () => {
      const active = activeLoadRef.current;
      activeLoadRef.current = null;
      active?.abort();
    };
  }, [load]);

  useEffect(() => {
    if (!entitlement) return;
    setPacksDraft(entitlement.hostPacks);
    if (entitlement.seats !== null) setSeatsDraft(entitlement.seats);
  }, [entitlement]);

  const teamPlan = entitlement !== null && isTeamPlan(entitlement.plan);
  const memberRole = entitlement?.role ?? null;
  const showMembers = teamPlan && (memberRole === "owner" || memberRole === "admin");
  const canInvite = memberRole === "owner" || memberRole === "admin";
  const ownsMembers = memberRole === "owner";

  const membersGeneration = useRef(0);

  const loadMembers = useCallback(
    async (signal?: AbortSignal) => {
      const generation = membersGeneration.current + 1;
      membersGeneration.current = generation;
      const currentAccountGen = accountGenerationRef.current;
      try {
        const next = await listOrgMembers(signal ? { ...call, signal } : call);
        if (
          generation !== membersGeneration.current ||
          accountGenerationRef.current !== currentAccountGen
        ) {
          return;
        }
        setMembers(next);
        setMembersError(null);
      } catch (error) {
        if (
          generation !== membersGeneration.current ||
          accountGenerationRef.current !== currentAccountGen
        ) {
          return;
        }
        const billingError = toBillingApiError(error);
        if (billingError.code === "REQUEST_ABORTED") return;
        setMembersError(billingError.message);
      }
    },
    [call],
  );

  useEffect(() => {
    if (!showMembers) {
      setMembers(null);
      setMembersError(null);
      return;
    }
    const controller = new AbortController();
    void loadMembers(controller.signal);
    return () => controller.abort();
  }, [showMembers, loadMembers]);

  const handleCheckout = async (plan: CheckoutPlan, seats?: number) => {
    const currentAccountGen = accountGenerationRef.current;
    setBusy(true);
    setActionError(null);
    try {
      const url = await startBillingCheckout(plan, seats === undefined ? call : { ...call, seats });
      if (accountGenerationRef.current !== currentAccountGen) return;
      await openExternalUrl(url);
    } catch (error) {
      if (accountGenerationRef.current !== currentAccountGen) return;
      setActionError(toBillingApiError(error));
    } finally {
      if (accountGenerationRef.current === currentAccountGen) {
        setBusy(false);
      }
    }
  };

  const handleQuantity = async (change: BillingQuantityChange, kind: RenewalChangeKind) => {
    const currentAccountGen = accountGenerationRef.current;
    setBusy(true);
    setActionError(null);
    try {
      const next = await updateBillingQuantity(change, call);
      if (accountGenerationRef.current !== currentAccountGen) return;
      setEntitlement(next);
      setRenewalNotice({ change: kind, limit: next.machineLimit });
    } catch (error) {
      if (accountGenerationRef.current !== currentAccountGen) return;
      setActionError(toBillingApiError(error));
    } finally {
      if (accountGenerationRef.current === currentAccountGen) {
        setBusy(false);
      }
    }
  };

  const handleInvite = async () => {
    const email = inviteEmail.trim();
    if (!email) return;
    const currentAccountGen = accountGenerationRef.current;
    setBusy(true);
    setActionError(null);
    try {
      const invited = await inviteOrgMember(email, call);
      if (accountGenerationRef.current !== currentAccountGen) return;
      setInviteEmail("");
      setInviteSent(invited);
      await loadMembers();
    } catch (error) {
      if (accountGenerationRef.current !== currentAccountGen) return;
      setActionError(toBillingApiError(error));
    } finally {
      if (accountGenerationRef.current === currentAccountGen) {
        setBusy(false);
      }
    }
  };

  const handleRemove = async (userId: string) => {
    const currentAccountGen = accountGenerationRef.current;
    setBusy(true);
    setActionError(null);
    try {
      await removeOrgMember(userId, call);
      if (accountGenerationRef.current !== currentAccountGen) return;
      setPendingRemoveId(null);
      await loadMembers();
    } catch (error) {
      if (accountGenerationRef.current !== currentAccountGen) return;
      setActionError(toBillingApiError(error));
    } finally {
      if (accountGenerationRef.current === currentAccountGen) {
        setBusy(false);
      }
    }
  };

  if (unavailable) return null;

  const limitDetails = actionError ? planLimitDetails(actionError.details) : null;
  const stoppedDetails = actionError ? suspensionDetails(actionError.details) : null;
  const loadLimitDetails = loadError ? planLimitDetails(loadError.details) : null;
  const loadStoppedDetails = loadError ? suspensionDetails(loadError.details) : null;
  const graceDays = entitlement ? daysLeft(entitlement.graceEndsAt, now) : null;
  const manageUrl = entitlement?.manageUrl ?? null;

  return (
    <section aria-labelledby="settings-plan-heading" aria-busy={busy}>
      <SettingsHeading
        icon={<CreditCard />}
        title="Plan"
        description="Machines this account can reach remotely. Local terminals, worktrees, and LAN pairing are never limited by the plan."
      />
      <h2 id="settings-plan-heading" className="sr-only">
        Plan
      </h2>

      {loading && !entitlement ? (
        <div data-testid="plan-loading" className="space-y-3">
          <span className="sr-only">Loading plan</span>
          <div className="h-4 w-40 rounded bg-accent/60" />
          <div className="h-2 w-full rounded bg-accent/40" />
          <div className="h-4 w-56 rounded bg-accent/40" />
        </div>
      ) : null}

      {loadError ? (
        <Alert
          data-testid="plan-error"
          data-code={loadError.code}
          data-limit={loadLimitDetails?.limit ?? undefined}
          data-used={loadLimitDetails?.used ?? undefined}
          data-grace-ends-at={loadStoppedDetails?.graceEndsAt ?? undefined}
          className="flex flex-wrap items-start gap-2 rounded-md border border-border bg-card p-3 [&>svg]:static [&>svg~*]:pl-0"
        >
          <AlertTriangle
            className={`mt-0.5 size-3.5 shrink-0 ${
              loadError.code === "UNAUTHORIZED" ? "text-destructive" : "text-status-warning"
            }`}
          />
          <AlertDescription className="min-w-0 text-[11px] leading-normal text-muted-foreground">
            <p>{errorText(loadError)}</p>
            <Button
              type="button"
              variant="outline"
              size="sm"
              className="mt-2 h-7 text-[11px]"
              disabled={loading}
              onClick={() => void load()}
            >
              Retry
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}

      {entitlement ? (
        <>
          <div
            data-testid="plan-summary"
            data-plan={entitlement.plan}
            data-status={entitlement.status}
            className="rounded-lg border border-border bg-card p-4"
          >
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-[15px] font-semibold text-foreground">
                    {PLAN_LABELS[entitlement.plan]}
                  </span>
                  <Badge
                    variant="secondary"
                    className="rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground shadow-none"
                  >
                    {STATUS_LABELS[entitlement.status]}
                  </Badge>
                </div>
                <p className="mt-1 text-[11px] text-muted-foreground" data-testid="plan-usage">
                  {entitlement.machinesUsed} of {entitlement.machineLimit} machines used remotely
                  {entitlement.hostPacks > 0 ? ` · ${entitlement.hostPacks} extra packs` : ""}
                  {entitlement.seats !== null ? ` · ${entitlement.seats} seats` : ""}
                </p>
              </div>
              {manageUrl ? (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-7 text-[11px]"
                  onClick={() => void openExternalUrl(manageUrl)}
                >
                  Manage billing
                  <ExternalLink className="size-3" />
                </Button>
              ) : null}
            </div>
            <Progress
              className="mt-3 h-1.5"
              value={usagePercent(entitlement)}
              aria-valuenow={usagePercent(entitlement)}
              aria-label={`Remote machines used: ${entitlement.machinesUsed} of ${entitlement.machineLimit}`}
            />
          </div>

          {entitlement.status === "over_limit" || entitlement.status === "past_due" ? (
            <Alert
              data-testid="plan-grace-banner"
              data-status={entitlement.status}
              data-days-left={graceDays ?? undefined}
              data-grace-ends-at={entitlement.graceEndsAt ?? undefined}
              className="mt-4 flex flex-wrap items-start gap-2 rounded-md border border-status-warning/40 bg-status-warning/10 p-3 [&>svg]:static [&>svg~*]:pl-0"
            >
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-status-warning" />
              <AlertDescription className="min-w-0 text-[11px] leading-normal text-foreground">
                <p>
                  {graceDays === null
                    ? "Remote access is in its grace period."
                    : `Remote access stops in ${graceDays} ${graceDays === 1 ? "day" : "days"}.`}{" "}
                  {entitlement.graceEndsAt === null
                    ? "Resolve the machine count or the payment to stay connected."
                    : `Grace ends ${formatDeadline(entitlement.graceEndsAt)}.`}
                </p>
                <p className="mt-1 text-muted-foreground" data-testid="plan-local-note">
                  Local terminals, worktrees, and LAN pairing keep working.
                </p>
              </AlertDescription>
            </Alert>
          ) : null}

          {entitlement.status === "stopped" ? (
            <Alert
              variant="destructive"
              data-testid="plan-stopped-banner"
              data-status="stopped"
              data-grace-ends-at={entitlement.graceEndsAt ?? undefined}
              className="mt-4 flex flex-wrap items-start gap-2 rounded-md border border-destructive/20 bg-destructive/10 p-3 [&>svg]:static [&>svg~*]:pl-0"
            >
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
              <AlertDescription className="min-w-0 text-[11px] leading-normal">
                <p>
                  Remote access is stopped
                  {entitlement.graceEndsAt === null
                    ? "."
                    : ` since ${formatDeadline(entitlement.graceEndsAt)}.`}{" "}
                  Renew the subscription or remove machines to restore it.
                </p>
                <p className="mt-1" data-testid="plan-local-note">
                  Local terminals, worktrees, and LAN pairing keep working.
                </p>
              </AlertDescription>
            </Alert>
          ) : null}

          {actionError ? (
            <Alert
              data-testid="plan-action-error"
              data-code={actionError.code}
              data-limit={limitDetails?.limit ?? undefined}
              data-used={limitDetails?.used ?? undefined}
              data-grace-ends-at={stoppedDetails?.graceEndsAt ?? undefined}
              className="mt-4 flex flex-wrap items-start gap-2 rounded-md border border-destructive/20 bg-destructive/10 p-3 [&>svg]:static [&>svg~*]:pl-0"
            >
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-destructive" />
              <AlertDescription className="min-w-0 text-[11px] leading-normal text-destructive">
                {errorText(actionError)}
              </AlertDescription>
            </Alert>
          ) : null}

          <div className="mt-8 space-y-3">
            <h3 className="text-[12px] font-semibold">Change Plan</h3>
            {canManagePlan(entitlement) ? (
              <div className="border-y border-border">
                <SettingRow
                  label="Pro"
                  description="10 machines included, plus 5 for each additional pack."
                >
                  <div className="flex flex-wrap items-center justify-end gap-2">
                    {PRO_CHOICES.map((choice) =>
                      entitlement.plan === choice.plan ? (
                        <Badge
                          key={choice.plan}
                          variant="secondary"
                          className="rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground shadow-none"
                        >
                          {choice.label} · current
                        </Badge>
                      ) : (
                        <Button
                          key={choice.plan}
                          type="button"
                          variant="outline"
                          size="sm"
                          className="h-7 text-[11px]"
                          disabled={busy}
                          onClick={() => void handleCheckout(choice.plan)}
                        >
                          {choice.label}
                        </Button>
                      ),
                    )}
                  </div>
                </SettingRow>

                <SettingRow
                  label="Team"
                  description="10 machines per seat (minimum 2 seats), plus 5 for each additional pack."
                >
                  <div className="flex flex-wrap items-center justify-end gap-2">
                    <Input
                      aria-label="Team seats"
                      type="number"
                      inputMode="numeric"
                      min={TEAM_MIN_SEATS}
                      value={seatsDraft}
                      onChange={(event) => setSeatsDraft(clampSeats(Number(event.target.value)))}
                      className="h-7 w-20 rounded-md px-2 text-[12px] md:text-[12px]"
                    />
                    {teamPlan ? (
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="h-7 text-[11px]"
                        disabled={busy || seatsDraft === entitlement.seats}
                        onClick={() => void handleQuantity({ seats: seatsDraft }, "seats")}
                      >
                        Apply seats
                      </Button>
                    ) : (
                      TEAM_CHOICES.map((choice) => (
                        <Button
                          key={choice.plan}
                          type="button"
                          variant="outline"
                          size="sm"
                          className="h-7 text-[11px]"
                          disabled={busy}
                          onClick={() => void handleCheckout(choice.plan, seatsDraft)}
                        >
                          {choice.label}
                        </Button>
                      ))
                    )}
                </div>
              </SettingRow>

                {entitlement.plan !== "free" ? (
                  <SettingRow
                    label="Additional machine packs"
                    description="Each pack adds 5 machines. Extra packs are charged from your next renewal."
                  >
                    <div className="flex flex-wrap items-center justify-end gap-2">
                      <Input
                        aria-label="Additional machine packs"
                        type="number"
                        inputMode="numeric"
                        min={0}
                        max={MAX_PACKS}
                        value={packsDraft}
                        onChange={(event) => setPacksDraft(clampPacks(Number(event.target.value)))}
                        className="h-7 w-20 rounded-md px-2 text-[12px] md:text-[12px]"
                      />
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        className="h-7 text-[11px]"
                        disabled={busy || packsDraft === entitlement.hostPacks}
                        onClick={() => void handleQuantity({ hostPacks: packsDraft }, "hostPacks")}
                      >
                        Apply packs
                      </Button>
                    </div>
                  </SettingRow>
                ) : null}
              </div>
            ) : (
              <p
                data-testid="plan-managed-note"
                className="text-[11px] text-muted-foreground"
              >
                Only the team owner or an admin can change the plan.
              </p>
            )}
            {renewalNotice ? (
              <p
                role="status"
                data-testid="plan-renewal-note"
                data-charge="next-renewal"
                data-change={renewalNotice.change}
                data-limit={renewalNotice.limit}
                className="text-[11px] text-muted-foreground"
              >
                Machine limit is now {renewalNotice.limit}. The extra charge starts on your next
                renewal.
              </p>
            ) : null}
          </div>

          {teamPlan ? (
            <div className="mt-8 space-y-3">
              <h3 className="text-[12px] font-semibold">Team Members</h3>
              {showMembers ? (
                <>
                  {membersError ? (
                    <p role="alert" className="text-[11px] text-destructive">
                      Team members unavailable. Reopen this section to retry.
                    </p>
                  ) : null}
                  <Card
                    data-testid="plan-members-card"
                    className="divide-y divide-border rounded-lg border border-border bg-card shadow-none"
                  >
                    {members === null ? (
                      <div className="p-4 text-center text-[12px] text-muted-foreground">Loading members</div>
                    ) : members.length === 0 ? (
                      <div className="p-4 text-center text-[12px] text-muted-foreground">
                        No members yet. Invite teammates so their machines join the team pool.
                      </div>
                    ) : (
                      members.map((member) => (
                        <div
                          key={member.userId}
                          data-testid="plan-member-row"
                          data-role={member.role}
                          data-user-id={member.userId}
                          className="flex flex-wrap items-center justify-between gap-3 p-3"
                        >
                          <div className="min-w-0">
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="truncate text-[13px] font-medium text-foreground">
                                {member.email}
                              </span>
                              <Badge
                                variant="secondary"
                                className="rounded bg-muted px-1.5 py-0.5 font-mono text-[11px] uppercase text-muted-foreground shadow-none"
                              >
                                {member.role}
                              </Badge>
                            </div>
                          </div>
                          {ownsMembers && member.role !== "owner" ? (
                            pendingRemoveId === member.userId ? (
                              <div className="flex items-center gap-1.5">
                                <Button
                                  type="button"
                                  variant="destructive"
                                  size="sm"
                                  aria-label={`Confirm remove ${member.email}`}
                                  disabled={busy}
                                  onClick={() => void handleRemove(member.userId)}
                                  className="h-7 px-2 text-[11px] font-medium"
                                >
                                  Confirm Remove
                                </Button>
                                <Button
                                  type="button"
                                  variant="outline"
                                  size="sm"
                                  onClick={() => setPendingRemoveId(null)}
                                  className="h-7 px-2 text-[11px] text-muted-foreground hover:bg-accent"
                                >
                                  Cancel
                                </Button>
                              </div>
                            ) : (
                              <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                aria-label={`Remove ${member.email}`}
                                onClick={() => setPendingRemoveId(member.userId)}
                                className="inline-flex h-7 items-center gap-1 px-2 text-[11px] text-muted-foreground hover:bg-accent hover:text-destructive"
                              >
                                <Trash2 className="size-3" />
                                Remove
                              </Button>
                            )
                          ) : null}
                        </div>
                      ))
                    )}
                  </Card>
                  {canInvite ? (
                    <div className="flex flex-wrap items-center gap-2">
                      <Input
                        aria-label="Invite member email"
                        type="email"
                        placeholder="teammate@example.com"
                        value={inviteEmail}
                        onChange={(event) => setInviteEmail(event.target.value)}
                        className="h-8 w-full max-w-72 rounded-md px-2 text-[12px] md:text-[12px]"
                      />
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        disabled={busy || !inviteEmail.trim()}
                        onClick={() => void handleInvite()}
                        className="h-7 text-[11px]"
                      >
                        <UserPlus className="size-3" />
                        Invite
                      </Button>
                    </div>
                  ) : null}
                  {inviteSent ? (
                    <p
                      role="status"
                      data-testid="plan-invite-sent"
                      data-email={inviteSent.email}
                      data-expires-at={inviteSent.expiresAt}
                      className="text-[11px] text-muted-foreground"
                    >
                      Invitation sent to {inviteSent.email}. They join the team only after opening
                      the emailed link and signing in; it expires on{" "}
                      {formatDeadline(inviteSent.expiresAt)}.
                    </p>
                  ) : null}
                </>
              ) : (
                <p className="text-[11px] text-muted-foreground">
                  Team membership is managed by the team owner.
                </p>
              )}
            </div>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
