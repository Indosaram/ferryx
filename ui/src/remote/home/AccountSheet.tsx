// The Account sheet: who is signed in, Sign out, and permanent account deletion behind a
// typed-email confirmation. The server compares the email; this screen only reports its answer.
import { LogOut, Trash2, UserRound } from "lucide-react";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";

import { AccountSessionError } from "../accountSession";
import { MobileSheet, MobileSheetItem } from "../chat/MobileSheet";
import { userFacingError } from "../userFacingError";
import { useOverlayHistory } from "./usePhoneNavigation";

const ACCOUNT_EMAIL_KEY = "ferryx.account.email";

/** The email the account signed in with, as the login answer reported it; null when unknown. */
export function getStoredAccountEmail(): string | null {
  try {
    return window.localStorage.getItem(ACCOUNT_EMAIL_KEY);
  } catch {
    return null;
  }
}

export function storeAccountEmail(email: string | null): void {
  try {
    if (email && email.trim()) window.localStorage.setItem(ACCOUNT_EMAIL_KEY, email.trim());
    else window.localStorage.removeItem(ACCOUNT_EMAIL_KEY);
  } catch {
    // Storage is unavailable (private mode): the sheet simply shows no email.
  }
}

const DELETE_ERRORS: Record<string, string> = {
  CONFIRMATION_MISMATCH: "That email does not match this account. Type the email you signed in with.",
  SUBSCRIPTION_ACTIVE: "This account still has an active subscription. Cancel it first, then delete the account.",
  UNAUTHORIZED: "Your sign-in expired. Sign in again, then retry.",
};

function deleteErrorText(err: unknown): { code: string; text: string } {
  const code = err instanceof AccountSessionError ? err.code : "DELETE_ACCOUNT_FAILED";
  const text = DELETE_ERRORS[code] ?? userFacingError(err, { action: "delete the account" });
  return { code, text };
}

export type AccountSheetProps = {
  readonly open: boolean;
  readonly onOpenChange: (open: boolean) => void;
  readonly email: string | null;
  readonly onSignOut: () => void;
  /** Calls deleteAccount; resolves on success and throws the structured error otherwise. */
  readonly onDelete: (confirmEmail: string) => Promise<void>;
  /** Runs after a confirmed deletion: the caller signs out to the login page. */
  readonly onDeleted: () => void;
  /** Opens the confirm step directly (QA harness only). */
  readonly initialStep?: "summary" | "confirm";
};

export function AccountSheet({ open, onOpenChange, email, onSignOut, onDelete, onDeleted, initialStep = "summary" }: AccountSheetProps) {
  const [step, setStep] = useState<"summary" | "confirm">(initialStep);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ code: string; text: string } | null>(null);
  const inputId = useId();
  const errorId = useId();
  const inputRef = useRef<HTMLInputElement | null>(null);
  const deleteOpenRef = useRef<HTMLButtonElement | null>(null);
  // The step swap unmounts the focused control; hand focus to its replacement inside the sheet.
  const focusAfterSwapRef = useRef(false);
  useOverlayHistory(open, () => onOpenChange(false));

  useEffect(() => {
    if (!focusAfterSwapRef.current) return;
    focusAfterSwapRef.current = false;
    (step === "confirm" ? inputRef : deleteOpenRef).current?.focus();
  }, [step]);

  const goTo = (next: "summary" | "confirm") => {
    focusAfterSwapRef.current = true;
    setStep(next);
  };

  useEffect(() => {
    if (open) return;
    setStep(initialStep);
    setTyped("");
    setError(null);
  }, [initialStep, open]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || !typed.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await onDelete(typed.trim());
    } catch (err) {
      setBusy(false);
      setError(deleteErrorText(err));
      return;
    }
    setBusy(false);
    onOpenChange(false);
    onDeleted();
  };

  return (
    <MobileSheet
      open={open}
      onOpenChange={onOpenChange}
      title={step === "confirm" ? "Delete account" : "Account"}
      ariaLabel={step === "confirm" ? "Delete account" : "Account"}
      testId="account-sheet"
    >
      {step === "summary" ? (
        <>
          <div className="flex min-h-12 items-center gap-3 px-3 py-2">
            <UserRound aria-hidden="true" className="size-5 shrink-0 text-chat-foreground-secondary" />
            <span className="min-w-0 flex-1">
              <span className="block text-sm text-chat-foreground-secondary">Signed in as</span>
              <span data-testid="account-email" className="block break-all text-base text-chat-foreground">{email ?? "Email unavailable on this device"}</span>
            </span>
          </div>
          <MobileSheetItem
            icon={LogOut}
            label="Sign out"
            testId="account-sign-out"
            onSelect={() => {
              onOpenChange(false);
              onSignOut();
            }}
          />
          <div className="mt-6 border-t border-chat-border pt-2">
            <button
              ref={deleteOpenRef}
              type="button"
              data-testid="account-delete-open"
              onClick={() => goTo("confirm")}
              className="flex min-h-12 w-full items-center gap-3 rounded-lg px-3 py-2 text-left text-chat-danger transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              <Trash2 aria-hidden="true" className="size-5 shrink-0" />
              <span className="text-base">Delete account…</span>
            </button>
          </div>
        </>
      ) : (
        <form data-testid="account-delete-form" onSubmit={(event) => void submit(event)} className="flex flex-col gap-3 px-2 pb-2 pt-1">
          <p className="text-sm leading-snug text-chat-foreground">
            This permanently deletes your Ferryx account, unlinks every computer and signs you out everywhere. It cannot be undone.
          </p>
          <label htmlFor={inputId} className="text-sm text-chat-foreground-secondary">
            {email ? (
              <>
                Type <span className="break-all font-medium text-chat-foreground">{email}</span> to confirm
              </>
            ) : (
              "Type your account email to confirm"
            )}
          </label>
          <input
            ref={inputRef}
            id={inputId}
            data-testid="account-delete-email"
            type="email"
            inputMode="email"
            autoComplete="off"
            autoCapitalize="none"
            spellCheck={false}
            value={typed}
            onChange={(event) => setTyped(event.target.value)}
            aria-invalid={error ? true : undefined}
            aria-describedby={error ? errorId : undefined}
            className="h-11 w-full rounded-lg border border-chat-border bg-chat-screen px-3 text-base text-chat-foreground placeholder:text-chat-foreground-secondary focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
          {error ? (
            <p id={errorId} data-testid="account-delete-error" data-code={error.code} role="alert" className="rounded-lg bg-chat-danger/10 px-3 py-2.5 text-sm leading-snug text-chat-danger">
              {error.text}
            </p>
          ) : null}
          <div className="flex gap-2 pt-1">
            <button
              type="button"
              onClick={() => {
                goTo("summary");
                setError(null);
              }}
              className="flex h-11 flex-1 items-center justify-center rounded-lg bg-chat-surface-raised text-base font-medium text-chat-foreground transition-colors hover:bg-chat-surface-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            >
              Cancel
            </button>
            <button
              type="submit"
              data-testid="account-delete-confirm"
              disabled={busy || !typed.trim()}
              className="flex h-11 flex-1 items-center justify-center rounded-lg bg-chat-danger text-base font-medium text-chat-screen transition-colors hover:bg-chat-danger/90 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-50"
            >
              {busy ? "Deleting…" : "Delete account"}
            </button>
          </div>
        </form>
      )}
    </MobileSheet>
  );
}
