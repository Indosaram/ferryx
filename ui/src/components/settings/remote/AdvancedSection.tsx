import { Button } from "../../ui/button";
import { Input } from "../../ui/input";
import { Label } from "../../ui/label";
import { RemoteAccessSection } from "../RemoteAccessSection";

export interface AdvancedSectionProps {
  open: boolean;
  /** Hidden when the account origin is fixed by the caller. */
  showAccountServer: boolean;
  accountOrigin: string;
  accountOriginDraft: string;
  accountOriginError: string | null;
  onAccountOriginDraftChange: (value: string) => void;
  onUseAccountOrigin: () => void;
  nativeStatus?: string;
  migrationStatus?: string;
}

export function AdvancedSection(props: AdvancedSectionProps) {
  return (
    <details open={props.open} className="rounded-lg border border-border p-4">
      <summary className="cursor-pointer text-[13px] font-medium">Advanced</summary>
      <div className="mt-4 space-y-4">
        {props.showAccountServer ? (
          <div className="space-y-2 rounded-lg border border-border bg-card p-4">
            <Label htmlFor="account-server-url">Account server URL</Label>
            <p className="text-[11px] text-muted-foreground">Sign in and enroll machines through this account authority. This does not change the relay tunnel on this machine.</p>
            <div className="flex flex-wrap items-center gap-2">
              <Input id="account-server-url" type="url" value={props.accountOriginDraft} onChange={e => props.onAccountOriginDraftChange(e.target.value)} className="h-8 w-full max-w-80 text-[12px]" />
              <Button type="button" variant="outline" size="sm" disabled={props.accountOriginDraft.trim() === props.accountOrigin} onClick={props.onUseAccountOrigin}>Use account server</Button>
            </div>
            {props.accountOriginError ? <p role="alert" className="text-[11px] text-destructive">{props.accountOriginError}</p> : null}
          </div>
        ) : null}
        <div className="space-y-2">
          <h3 className="text-[13px] font-medium">Connection diagnostics</h3>
          <dl className="text-[13px]">
            <dt className="font-semibold">Native inventory</dt>
            <dd className="text-muted-foreground">{props.nativeStatus}</dd>
            <dt className="mt-2 font-semibold">Credential migration</dt>
            <dd className="text-muted-foreground">{props.migrationStatus}</dd>
          </dl>
        </div>
        <RemoteAccessSection detailsOnly />
      </div>
    </details>
  );
}
