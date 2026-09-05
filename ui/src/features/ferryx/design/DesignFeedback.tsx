import { useEffect, useReducer, useRef, useState } from "react";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { Label } from "../../../components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "../../../components/ui/select";
import type { TargetRef } from "../../../lib/scopedContracts";
import type { DesignIdentity } from "./model";
import { DesignSession } from "./session";

export interface DesignFeedbackProps {
  session: DesignSession;
  identity: DesignIdentity;
  targets: readonly { target: TargetRef; label: string; supportsImages: boolean }[];
  /** Acquire/release the EXISTING BrowserPane mask; never set native visibility directly. */
  maskPreview: (masked: boolean) => void;
}
export function DesignFeedback({ session, identity, targets, maskPreview }: DesignFeedbackProps) {
  const [, render] = useReducer(n => n + 1, 0);
  const [armed, setArmed] = useState(false), [preview, setPreview] = useState(false);
  const [targetKey, setTargetKey] = useState(""), [note, setNote] = useState("");
  const [busy, setBusy] = useState(false), [error, setError] = useState(""), [status, setStatus] = useState("");
  const [imageUrl, setImageUrl] = useState<string>();
  const toggle = useRef<HTMLButtonElement>(null), noteInput = useRef<HTMLInputElement>(null);
  useEffect(() => session.subscribe(() => { render(); if (session.capture) { setPreview(true); setArmed(false); } }), [session]);
  useEffect(() => {
    if (!session.capture) return;
    const url = URL.createObjectURL(new Blob([session.capture.png.slice()], { type: "image/png" })); setImageUrl(url);
    return () => URL.revokeObjectURL(url);
  }, [session.capture]);
  useEffect(() => { maskPreview(preview); if (preview) noteInput.current?.focus(); return () => maskPreview(false); }, [preview, maskPreview]);
  useEffect(() => () => { void session.cancel().catch(e => console.error("Design cancellation failed", e)); }, [session, identity.browserId, identity.webviewLabel, identity.generation, identity.viewportRevision]);
  const run = async (action: () => Promise<void>) => { setBusy(true); setError(""); try { await action(); } catch (e) { setError(e instanceof Error ? e.message : String(e)); } finally { setBusy(false); } };
  const cancel = () => run(async () => { await session.cancel(); setPreview(false); setArmed(false); toggle.current?.focus(); });
  const begin = (mode: "element" | "rectangle") => run(async () => { await session.begin(identity, mode); setArmed(true); setStatus(""); });
  return <section aria-label="Design feedback" className="min-w-0 text-foreground" onKeyDown={e => { if (e.key === "Escape" && !busy) { e.stopPropagation(); void cancel(); } }}>
    <div className="flex flex-wrap items-center gap-2">
      <Button ref={toggle} size="sm" variant="outline" data-testid="design-mode-toggle" aria-pressed={armed} disabled={busy} onClick={() => void (armed ? cancel() : begin("element"))}>Design mode</Button>
      <Button size="sm" variant="ghost" data-testid="design-element" disabled={busy} onClick={() => void begin("element")}>Select element</Button>
      <Button size="sm" variant="ghost" data-testid="design-area" disabled={busy} onClick={() => void begin("rectangle")}>Select area</Button>
    </div>
    {armed && <p className="py-2 text-xs text-muted-foreground">Select in the page. Escape cancels.</p>}
    {preview && session.capture && <div data-testid="design-preview" className="mt-3 grid min-w-0 gap-3 rounded-md border border-border bg-card p-3">
      <h3 className="text-sm font-medium">Design feedback</h3>
      {imageUrl && <img src={imageUrl} alt="Selected browser viewport crop" className="max-h-64 max-w-full object-contain" />}
      <p className="break-all font-mono text-xs text-muted-foreground">{session.capture.selection.element?.selector ?? "Viewport area"}</p>
      {session.capture.selection.element?.contextUnavailable && <p role="status" className="text-xs text-muted-foreground">Frame internals are unavailable. The image contains visible viewport pixels.</p>}
      <Label htmlFor="design-note">Note</Label><Input ref={noteInput} id="design-note" data-testid="design-note" value={note} disabled={busy || !!session.draft} onChange={e => setNote(e.target.value)} />
      <Label htmlFor="design-target">Send to agent</Label>
      <Select value={targetKey} onValueChange={setTargetKey} disabled={busy || !!session.draft}>
        <SelectTrigger id="design-target" data-testid="design-target"><SelectValue placeholder="Choose an agent explicitly" /></SelectTrigger>
        <SelectContent>{targets.map(({ target, label, supportsImages }) => <SelectItem key={JSON.stringify(target)} value={JSON.stringify(target)} disabled={!supportsImages}>{label} ({target.hostId}){!supportsImages ? " - images unsupported" : ""}</SelectItem>)}</SelectContent>
      </Select>
      {targets.length === 0 && <p role="status" className="text-xs text-muted-foreground">No available agent targets. Your selection is retained.</p>}
      {session.draft && <p className="text-xs text-muted-foreground">Confirmed for {session.draft.target.hostId} / {session.draft.target.backendSessionId}. The note and attachment are locked.</p>}
      <div className="flex flex-wrap justify-end gap-2">
        <Button variant="ghost" size="sm" disabled={busy} onClick={() => void cancel()}>Cancel</Button>
        {!session.draft && <Button size="sm" disabled={busy || !targets.some(t => JSON.stringify(t.target) === targetKey && t.supportsImages)} onClick={() => void run(async () => {
          const chosen = targets.find(t => JSON.stringify(t.target) === targetKey && t.supportsImages);
          if (!chosen) throw new Error("TARGET_EXPIRED");
          await session.confirm(chosen.target, note, crypto.randomUUID());
        })}>{busy ? "Transferring image..." : "Confirm draft"}</Button>}
        <Button data-testid="design-send" size="sm" disabled={busy || !session.draft || !!status} onClick={() => void run(async () => { const receipt = await session.send(); setStatus(receipt.stage); })}>{busy ? "Sending..." : "Send feedback"}</Button>
      </div>
    </div>}
    {(error || session.error) && <p role="alert" className="py-2 text-xs text-destructive">{error || session.error?.message}</p>}
    {status && <p role="status" className="py-2 text-xs">Delivery: {status}</p>}
  </section>;
}
