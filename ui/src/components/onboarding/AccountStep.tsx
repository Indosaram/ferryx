import { useCallback, useEffect, useRef, useState } from "react";

import { adoptAccountSession } from "../../lib/accountPairing";
import { getAccountEnrollmentStatus, type AccountEnrollmentStatus } from "../../lib/tauri";
import { getConfiguredAccountOrigin, getStoredAccountSessionToken } from "../../remote/accountSession";
import { AccountSignIn } from "../settings/AccountSignIn";
import { WizardRow, WizardRowList, WizardStatus } from "./WizardPrimitives";

/**
 * Signing in is the whole step. There is no enrollment code to read out and no second "link"
 * action: the account session is handed to the daemon, which enrolls this computer when it has
 * to and pairs it with the account.
 */
export type AccountStepDeps = {
  loadStatus: () => Promise<AccountEnrollmentStatus>;
  readToken: (origin: string) => string | null;
  adopt: (origin: string, token: string | null) => Promise<void>;
};

const DEFAULT_DEPS: AccountStepDeps = {
  loadStatus: getAccountEnrollmentStatus,
  readToken: getStoredAccountSessionToken,
  adopt: adoptAccountSession,
};

export type AccountStepProps = {
  origin?: string;
  deps?: Partial<AccountStepDeps>;
  onLinked?: () => void;
};

type Phase = { kind: "loading" } | { kind: "linked" } | { kind: "signed-out" };

export function AccountStep({ origin, deps, onLinked }: AccountStepProps): JSX.Element {
  const accountOrigin = origin ?? getConfiguredAccountOrigin();
  const depsRef = useRef<AccountStepDeps>({ ...DEFAULT_DEPS, ...deps });
  const onLinkedRef = useRef(onLinked);
  onLinkedRef.current = onLinked;
  const mountedRef = useRef(true);
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });

  // There is nothing to await: the session the user just obtained is the pairing. The daemon
  // enrolls and pairs from it, so the step completes the moment sign-in returns. Onboarding is
  // its own surface and Settings may never mount, so the handoff happens here too.
  const link = useCallback(() => {
    void depsRef.current.adopt(accountOrigin, depsRef.current.readToken(accountOrigin));
    setPhase({ kind: "linked" });
    onLinkedRef.current?.();
  }, [accountOrigin]);

  useEffect(() => {
    mountedRef.current = true;
    depsRef.current
      .loadStatus()
      .catch(() => ({ enrolled: false, accountOrigin: null, enrolledAt: null }))
      .then((status) => {
        if (!mountedRef.current) return;
        if (status.enrolled) {
          setPhase({ kind: "linked" });
          return;
        }
        const token = depsRef.current.readToken(accountOrigin);
        if (token) {
          link();
        } else {
          setPhase({ kind: "signed-out" });
        }
      });
    return () => {
      mountedRef.current = false;
    };
  }, [accountOrigin, link]);

  if (phase.kind === "signed-out") {
    return (
      <AccountSignIn
        origin={accountOrigin}
        onSignIn={() => link()}
      />
    );
  }

  const status = phase.kind === "linked"
    ? <WizardStatus tone="success" label="Linked" />
    : <WizardStatus tone="neutral" label="Checking…" />;

  return (
    <div data-testid="onboarding-account" data-phase={phase.kind}>
      <WizardRowList>
        <WizardRow
          testId="onboarding-account-this-computer"
          title="This computer"
          description="Linked computers show up in the worktree picker on your phone."
          status={status}
        />
      </WizardRowList>
    </div>
  );
}
