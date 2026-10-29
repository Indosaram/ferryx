import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";
import { KeyRound } from "lucide-react";
import { cn } from "../../lib/cn";
import {
  answerSshAuthPrompt,
  extractIpcErrorMessage,
  listenSshAuthPrompts,
  type SshAuthPrompt,
  type SshAuthPromptKind,
} from "../../lib/sshHosts";
import { Button } from "../ui/button";
import { Card } from "../ui/card";
import { Input } from "../ui/input";
import { Label } from "../ui/label";

const INPUT_LABELS: Record<SshAuthPromptKind, string> = {
  password: "Password",
  passphrase: "Key passphrase",
  code: "Verification code",
  other: "Response",
};

/**
 * Shows interactive SSH auth prompts (password, key passphrase, verification code) raised by the
 * backend while a connection is in progress, one at a time in arrival order.
 */
export function SshAuthPromptCard({ className }: { className?: string }) {
  const [queue, setQueue] = useState<SshAuthPrompt[]>([]);
  const [draft, setDraft] = useState<{ id: string; value: string }>({ id: "", value: "" });
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const timersRef = useRef(new Map<string, ReturnType<typeof setTimeout>>());
  const inputRef = useRef<HTMLInputElement>(null);
  const inputId = useId();

  const dropPrompt = useCallback((id: string) => {
    const timer = timersRef.current.get(id);
    if (timer !== undefined) {
      clearTimeout(timer);
      timersRef.current.delete(id);
    }
    setQueue(current => current.filter(prompt => prompt.id !== id));
  }, []);

  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    const timers = timersRef.current;

    listenSshAuthPrompts(prompt => {
      if (disposed || timers.has(prompt.id)) return;
      timers.set(
        prompt.id,
        setTimeout(() => {
          timers.delete(prompt.id);
          setQueue(current => current.filter(entry => entry.id !== prompt.id));
        }, Math.max(0, prompt.expiresInMs)),
      );
      setQueue(current => [...current, prompt]);
    }).then(
      unsub => {
        if (disposed) unsub();
        else unsubscribe = unsub;
      },
      err => {
        if (!disposed) console.warn("Failed to subscribe to SSH auth prompts", err);
      },
    );

    return () => {
      disposed = true;
      unsubscribe?.();
      unsubscribe = null;
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
    };
  }, []);

  const current = queue[0] ?? null;
  const currentId = current?.id ?? null;
  const value = current && draft.id === current.id ? draft.value : "";

  useEffect(() => {
    if (currentId) inputRef.current?.focus();
  }, [currentId]);

  const respond = async (prompt: SshAuthPrompt, answer: string | null) => {
    setDraft({ id: "", value: "" });
    dropPrompt(prompt.id);
    setError(null);
    setSending(true);
    try {
      await answerSshAuthPrompt(prompt.id, answer);
    } catch (err) {
      setError(extractIpcErrorMessage(err, "Could not send the response to SSH."));
    } finally {
      setSending(false);
    }
  };

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!current || !value || sending) return;
    void respond(current, value);
  };

  if (!current && !error) return null;

  if (!current) {
    return (
      <Card className={cn("space-y-3 border-border bg-muted/20 p-3.5", className)} data-testid="ssh-auth-prompt">
        <p role="alert" className="text-[12px] text-destructive">{error}</p>
        <div className="flex items-center gap-2 border-t border-border pt-3">
          <span className="flex-1" />
          <Button type="button" variant="outline" size="sm" className="text-[12px]" onClick={() => setError(null)}>
            Dismiss
          </Button>
        </div>
      </Card>
    );
  }

  const isCode = current.kind === "code";
  const label = INPUT_LABELS[current.kind] ?? INPUT_LABELS.other;

  return (
    <Card className={cn("space-y-3 border-border bg-muted/20 p-3.5", className)} data-testid="ssh-auth-prompt">
      <form className="space-y-3" onSubmit={handleSubmit}>
        <div className="flex items-start gap-2">
          <KeyRound className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
          <div className="min-w-0 space-y-1">
            <p className="text-[13px] font-medium text-foreground">SSH needs your input</p>
            <p className="break-all text-[12px] text-muted-foreground" data-testid="ssh-auth-prompt-host">
              {current.host}
            </p>
            <p className="whitespace-pre-wrap break-words text-[12px] text-foreground" data-testid="ssh-auth-prompt-text">
              {current.prompt}
            </p>
          </div>
        </div>
        <div className="space-y-1">
          <Label htmlFor={inputId} className="text-[11px] font-medium">{label}</Label>
          <Input
            ref={inputRef}
            id={inputId}
            type={isCode ? "text" : "password"}
            autoComplete={isCode ? "one-time-code" : "off"}
            inputMode={isCode ? "numeric" : undefined}
            value={value}
            onChange={event => setDraft({ id: current.id, value: event.target.value })}
          />
        </div>
        {error ? <p role="alert" className="text-[12px] text-destructive">{error}</p> : null}
        <div className="flex items-center gap-2 border-t border-border pt-3">
          <span className="flex-1" />
          <Button type="button" variant="outline" size="sm" className="text-[12px]"
            onClick={() => void respond(current, null)}>
            Cancel
          </Button>
          <Button type="submit" size="sm" className="text-[12px]" disabled={!value || sending}>
            Submit
          </Button>
        </div>
      </form>
    </Card>
  );
}
