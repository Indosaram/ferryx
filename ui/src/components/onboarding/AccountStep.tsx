import { useCallback, useEffect, useRef, useState } from "react";

import {
  enrollThisMachine,
  getAccountEnrollmentStatus,
  type AccountEnrollmentStatus,
} from "../../lib/tauri";
import {
  AccountSessionError,
  getConfiguredAccountOriginUnchecked,
  getStoredAccountSessionToken,
  issueEnrollmentCode,
} from "../../remote/accountSession";
import { AccountSignIn } from "../settings/AccountSignIn";
import { Button } from "../ui/button";
import { WizardRow, WizardRowList, WizardStatus } from "./WizardPrimitives";

export type AccountStepDeps = {
  loadStatus: () => Promise<AccountEnrollmentStatus>;
  issueCode: (origin: string, token: string) => Promise<{ code: string }>;
  enroll: (origin: string, code: string) => Promise<AccountEnrollmentStatus>;
  readToken: (origin: string) => string | null;
};

const DEFAULT_DEPS: AccountStepDeps = {
  loadStatus: getAccountEnrollmentStatus,
  issueCode: issueEnrollmentCode,
  enroll: enrollThisMachine,
  readToken: getStoredAccountSessionToken,
};

export type AccountStepProps = {
  origin?: string;
  deps?: Partial<AccountStepDeps>;
  onLinked?: () => void;
};

type Phase =
  | { kind: "loading" }
  | { kind: "linked" }
  | { kind: "signed-out" }
  | { kind: "signed-in"; token: string }
  | { kind: "linking" }
  | { kind: "failed"; token: string; code: string; message: string };

function describeError(error: unknown): { code: string; message: string } {
  if (error instanceof AccountSessionError) {
    return { code: error.code, message: error.message };
  }
  if (error && typeof error === "object" && "code" in error) {
    const typed = error as { code: unknown; message?: unknown };
    return {
      code: String(typed.code),
      message: typeof typed.message === "string" ? typed.message : String(typed.code),
    };
  }
  return {
    code: "ENROLL_FAILED",
    message: error instanceof Error ? error.message : "Could not link this computer.",
  };
}

export function AccountStep({ origin, deps, onLinked }: AccountStepProps): JSX.Element {
  const accountOrigin = origin ?? getConfiguredAccountOriginUnchecked();
  const depsRef = useRef<AccountStepDeps>({ ...DEFAULT_DEPS, ...deps });
  const onLinkedRef = useRef(onLinked);
  onLinkedRef.current = onLinked;
  const mountedRef = useRef(true);
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });

  const link = useCallback(
    async (token: string) => {
      setPhase({ kind: "linking" });
      try {
        const { code } = await depsRef.current.issueCode(accountOrigin, token);
        const status = await depsRef.current.enroll(accountOrigin, code);
        if (!mountedRef.current) return;
        if (!status.enrolled) {
          setPhase({
            kind: "failed",
            token,
            code: "ENROLL_FAILED",
            message: "The account did not confirm this computer.",
          });
          return;
        }
        setPhase({ kind: "linked" });
        onLinkedRef.current?.();
      } catch (error) {
        if (!mountedRef.current) return;
        setPhase({ kind: "failed", token, ...describeError(error) });
      }
    },
    [accountOrigin],
  );

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
          void link(token);
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
        onSignIn={(token) => void link(token)}
      />
    );
  }

  const status =
    phase.kind === "linked" ? (
      <WizardStatus tone="success" label="Linked" />
    ) : phase.kind === "failed" ? (
      <WizardStatus tone="warning" label="Not linked" />
    ) : (
      <WizardStatus tone="neutral" label={phase.kind === "linking" ? "Linking…" : "Checking…"} />
    );

  return (
    <div data-testid="onboarding-account" data-phase={phase.kind}>
      <WizardRowList>
        <WizardRow
          testId="onboarding-account-this-computer"
          title="This computer"
          description={
            phase.kind === "failed"
              ? `${phase.code}: ${phase.message}`
              : "Linked computers show up in the worktree picker on your phone."
          }
          status={status}
          action={
            phase.kind === "failed" ? (
              <Button
                size="sm"
                variant="outline"
                data-testid="onboarding-account-retry"
                onClick={() => void link(phase.token)}
              >
                Retry
              </Button>
            ) : null
          }
        />
      </WizardRowList>
    </div>
  );
}
