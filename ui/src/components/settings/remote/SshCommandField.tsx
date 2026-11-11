import { useId, useState, type ClipboardEvent } from "react";
import { CheckCircle2 } from "lucide-react";
import { looksLikeSshTarget, parseSshTarget, type ParsedSshTarget } from "../../../lib/sshTarget";
import type { HostFormData } from "../AddMachineModal";
import { Input } from "../../ui/input";
import { Label } from "../../ui/label";

export function applySshTarget(form: HostFormData, target: ParsedSshTarget): HostFormData {
  const previousDefaultLabel = form.label.trim() === "" || form.label.trim() === form.hostname.trim();
  return {
    ...form,
    label: previousDefaultLabel ? target.hostname : form.label,
    hostname: target.hostname,
    username: target.username ?? "",
    port: String(target.port ?? 22),
    jumpHost: target.jumpHost ?? "",
    identityFile: target.identityFile ?? form.identityFile,
    authMethod: target.identityFile ? "key" : form.authMethod,
  };
}

export function hostnamePasteHandler(apply: (target: ParsedSshTarget) => void) {
  return (event: ClipboardEvent<HTMLInputElement>) => {
    const text = event.clipboardData.getData("text");
    if (!looksLikeSshTarget(text)) return;
    const target = parseSshTarget(text);
    if (!target) return;
    event.preventDefault();
    apply(target);
  };
}

function describe(target: ParsedSshTarget): string {
  const user = target.username ? `${target.username}@` : "";
  const port = target.port && target.port !== 22 ? `:${target.port}` : "";
  const via = target.jumpHost ? ` via ${target.jumpHost}` : "";
  return `${user}${target.hostname}${port}${via}`;
}

export function SshCommandField({ disabled, onParsed, compact }: {
  disabled?: boolean;
  onParsed: (target: ParsedSshTarget) => void;
  compact?: boolean;
}) {
  const id = useId();
  const [text, setText] = useState("");
  const [result, setResult] = useState<ParsedSshTarget | null>(null);
  const invalid = text.trim() !== "" && result === null;
  return (
    <div className="space-y-1">
      <Label htmlFor={id} className={compact ? "text-[12px]" : "text-[11px] font-medium"}>SSH command or address</Label>
      <Input
        id={id}
        aria-label="SSH command or address"
        aria-describedby={`${id}-hint`}
        aria-invalid={invalid || undefined}
        disabled={disabled}
        autoComplete="off"
        autoCapitalize="off"
        spellCheck={false}
        className={compact ? "h-7 font-mono text-[12px]" : "font-mono text-[12px]"}
        value={text}
        placeholder="ssh -J bastion user@10.0.0.5 -p 2222"
        onChange={(event) => {
          const value = event.target.value;
          setText(value);
          const parsed = value.trim() ? parseSshTarget(value) : null;
          setResult(parsed);
          // Typing only previews; a paste, Enter or leaving the field fills the form, so a
          // half-typed command never overwrites fields edited by hand.
          if (parsed && (event.nativeEvent as InputEvent).inputType === "insertFromPaste") onParsed(parsed);
        }}
        onBlur={() => { if (result) onParsed(result); }}
        onKeyDown={(event) => {
          if (event.key !== "Enter") return;
          event.preventDefault();
          if (result) onParsed(result);
        }}
      />
      <p id={`${id}-hint`} role="status" className={invalid ? "text-[11px] text-destructive" : "flex items-center gap-1 text-[11px] text-muted-foreground"}>
        {result ? (
          <>
            <CheckCircle2 className="size-3 shrink-0 text-status-success" aria-hidden="true" />
            <span className="min-w-0 break-all">{describe(result)}</span>
          </>
        ) : invalid ? (
          "Not an SSH command or user@host address."
        ) : (
          "Paste an ssh command, ssh:// link or user@host:port to fill in the fields below."
        )}
      </p>
    </div>
  );
}
