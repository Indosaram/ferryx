import { RemoteAccessSection } from "../RemoteAccessSection";
import { RemoteDesktopHostSection } from "./RemoteDesktopHostSection";

/** Open by default: the summary text "Access to This Machine" is pinned by SettingsDialog tests. */
export function ThisMachineSection() {
  return (
    <section aria-labelledby="remote-this-machine" className="space-y-3">
      <header className="border-b border-border pb-2">
        <h2 id="remote-this-machine" className="text-[13px] font-semibold text-foreground">This machine</h2>
        <p className="text-[12px] text-muted-foreground">Let other devices reach terminals on this machine.</p>
      </header>
      <details open className="rounded-lg border border-border p-4">
        <summary className="cursor-pointer text-[13px] font-medium">Access to This Machine</summary>
        <RemoteAccessSection hideTitle />
      </details>
      <RemoteDesktopHostSection />
    </section>
  );
}
