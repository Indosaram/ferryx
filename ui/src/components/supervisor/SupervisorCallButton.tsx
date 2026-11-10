// The call entry in the supervisor header. The button owns only "is the call open"; the surface
// owns the call, so the heavy device setup happens when the owner asks for it and never on mount.
import { Phone } from "lucide-react";
import { useState } from "react";
import { createPortal } from "react-dom";

import { cn } from "../../lib/cn";
import type { UnifiedSupervisorClient, UnifiedSupervisorView } from "../../lib/unifiedSupervisor";
import type { VoiceCallPort } from "../../lib/voiceCall";
import { desktopVoicePort } from "../../lib/voiceCallPort";
import { VoiceCallSurface } from "./VoiceCallSurface";

function CallOverlay({
  view,
  client,
  projectId,
  port,
  onClose,
}: {
  view: UnifiedSupervisorView | null;
  client: UnifiedSupervisorClient;
  projectId: string | null;
  /** How this surface reaches the machine's speech commands; absent means this machine's IPC. */
  port?: () => VoiceCallPort;
  onClose: () => void;
}) {
  // One port per call: it owns the microphone stream and must be torn down with the surface.
  const [voice] = useState(() => (port ?? desktopVoicePort)());
  // The sidebar is inside scrolling/overflow containers, so the call is rendered at the body.
  return createPortal(
    <VoiceCallSurface view={view} client={client} projectId={projectId} port={voice} onClose={onClose} />,
    document.body,
  );
}

export function SupervisorCallButton({
  view,
  client,
  projectId,
  port,
  className,
}: {
  view: UnifiedSupervisorView | null;
  client: UnifiedSupervisorClient;
  projectId: string | null;
  /** The phone passes its gateway transport; the desktop omits it and uses local IPC. */
  port?: () => VoiceCallPort;
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-label="Start a voice call with the supervisor"
        title="Call the supervisor"
        data-testid="supervisor-call"
        className={cn(
          "inline-flex size-5 shrink-0 items-center justify-center rounded text-muted-foreground",
          "hover:bg-white/[0.08] hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
          className,
        )}
      >
        <Phone className="size-3" aria-hidden />
      </button>
      {open ? (
        <CallOverlay
          view={view}
          client={client}
          projectId={projectId}
          port={port}
          onClose={() => setOpen(false)}
        />
      ) : null}
    </>
  );
}

