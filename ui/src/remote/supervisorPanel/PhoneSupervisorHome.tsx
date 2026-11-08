// The phone home: the supervisor panel at full width, composed from the same PanelSurface the
// desktop sidebar uses (with `touch`: 44px targets, rem text). Status line, decisions
// (recommended option first), what the supervisor is handling (with Stop), the sessions, today's
// log (Unverified / Why / Undo), the rules line and a composer pinned above the safe area.
// A decision header or a row opens that session's terminal screen. There is no tab bar: the
// panel is the only home surface. "+" opens the Worktrees screen to start a session.
// Pull-to-refresh and the edge-swipe back (owned by the Session screen) work as before.
import { ChevronRight, Plus } from "lucide-react";
import { useRef, useState, type ReactNode } from "react";

import { Composer } from "../../components/supervisorPanel/Composer";
import { toComposerResult } from "../../components/supervisorPanel/composerResult";
import { PanelSurface } from "../../components/supervisorPanel/PanelSurface";
import { cn } from "../../lib/cn";
import type { PanelActions } from "../../lib/supervisorPanel/actions";
import type { TranscriptSource } from "../../lib/supervisorPanel/useTranscript";
import type { MentionOption } from "../../components/supervisorPanel/types";
import { PhoneHomeHeader } from "../home/PhoneHomeHeader";
import { PullToRefreshIndicator } from "../home/PullToRefreshIndicator";
import { pullContentProps, usePullToRefresh } from "../home/usePullToRefresh";
import type { PanelState } from "./usePhonePanel";
import { SupervisorCallButton } from "../../components/supervisor/SupervisorCallButton";
import type { VoiceCallPort } from "../../lib/voiceCall";
import type { UnifiedSupervisorClient, UnifiedSupervisorView } from "../../lib/unifiedSupervisor";

export type PhoneSupervisorHomeProps = {
  readonly state: PanelState;
  readonly now: number;
  readonly actions: PanelActions;
  readonly transcript?: TranscriptSource;
  readonly onRefresh: () => Promise<unknown> | void;
  /** "+": go where a session is started. Absent: no button. */
  readonly onStartSession?: () => void;
  /** The computer the setup card names. */
  readonly computerLabel?: string;
  /** The Account entry. */
  readonly headerAction?: ReactNode;
  /** The session already open on this phone; tapping it returns to the Session screen. */
  readonly resume?: { readonly title: string; readonly subtitle: string; readonly onOpen: () => void } | null;
  /** Supervisor lease cards (standby computers, lease warnings), shown above the panel. */
  readonly lease?: ReactNode;
  /**
   * The supervisor voice call: how this phone reaches the host's speech commands, and the host
   * connection the call turns a transcript into. Absent: this build offers no call entry.
   */
  readonly voice?: {
    readonly port: VoiceCallPort;
    readonly view: UnifiedSupervisorView;
    readonly client: UnifiedSupervisorClient;
    readonly projectId: string | null;
  } | null;
};

const ROW_BUTTON =
  "flex min-h-11 w-full min-w-0 items-center gap-2 rounded-lg px-3 py-2 text-left transition-colors hover:bg-chat-row-hover active:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring";

export function PhoneSupervisorHome(props: PhoneSupervisorHomeProps) {
  const { state, now, actions, headerAction, resume, onStartSession } = props;
  const scrollRef = useRef<HTMLDivElement>(null);
  const pull = usePullToRefresh(scrollRef, props.onRefresh);
  const pullContent = pullContentProps(pull);
  const [seed, setSeed] = useState<{ text: string; nonce: number } | null>(null);
  const snapshot = state.kind === "ready" ? state.snapshot : null;
  const mentions: MentionOption[] = (snapshot?.sessions ?? []).map((session) => ({
    sessionRef: session.sessionRef,
    title: session.title,
    computer: session.hostLabel,
  }));
  const trailing = (
    <div className="flex items-center gap-1">
      {onStartSession ? (
        <button
          type="button"
          data-testid="phone-panel-start-session"
          aria-label="Start a session"
          onClick={onStartSession}
          className="grid size-11 place-items-center rounded-lg text-chat-foreground transition-colors hover:bg-chat-row-hover focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
        >
          <Plus aria-hidden="true" className="size-5" />
        </button>
      ) : null}
      {headerAction}
    </div>
  );
  return (
    <div data-testid="mobile-home" data-home="panel" className="flex min-h-0 min-w-0 flex-1 flex-col bg-chat-screen pt-[env(safe-area-inset-top)] px-safe">
      <PhoneHomeHeader title="Ferryx" trailing={trailing} />
      <main data-testid="phone-supervisor-home" className="flex min-h-0 min-w-0 flex-1 flex-col">
        <div ref={scrollRef} className="relative flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-y-contain scrollbar-sleek">
          <PullToRefreshIndicator {...pull} />
          <div className={cn("flex w-full flex-col gap-5 px-4 pb-6", pullContent.className)} style={pullContent.style}>
            {props.lease}
            {props.voice ? (
              <div data-testid="phone-supervisor-call" className="flex min-h-11 items-center gap-2 px-1">
                <span className="text-sm font-medium text-chat-foreground-secondary">Talk to the supervisor</span>
                <SupervisorCallButton
                  view={props.voice.view}
                  client={props.voice.client}
                  projectId={props.voice.projectId}
                  port={() => props.voice!.port}
                />
              </div>
            ) : null}
            {state.kind === "loading" ? (
              <p data-testid="phone-panel-loading" className="px-1 text-sm text-chat-foreground-secondary">
                Checking your computers…
              </p>
            ) : snapshot ? (
              <PanelSurface
                snapshot={snapshot}
                actions={actions}
                touch
                nowMs={now}
                computerLabel={props.computerLabel}
                hideComposer
                transcript={props.transcript}
                onComposerText={(text) => setSeed((current) => ({ text, nonce: (current?.nonce ?? 0) + 1 }))}
              />
            ) : (
              <p data-testid="phone-panel-error" role="alert" className="rounded-xl bg-chat-surface px-4 py-3 text-sm text-chat-foreground-secondary">
                {state.kind === "error" ? state.message : "The supervisor is not available on this computer yet."} Pull down to try again.
              </p>
            )}
          </div>
        </div>
      </main>
      {resume ? (
        <aside aria-label="Open session" className="shrink-0 border-t border-chat-border bg-chat-header">
          <button type="button" data-testid="mobile-home-resume" onClick={resume.onOpen} className={cn(ROW_BUTTON, "rounded-none px-4")}>
            <span className="min-w-0 flex-1 truncate text-sm">
              <span className="text-chat-foreground-secondary">Back to </span>
              <span className="font-medium text-chat-foreground">{resume.title}</span>
            </span>
            <span className="min-w-0 max-w-[40%] truncate text-xs text-chat-foreground-secondary">{resume.subtitle}</span>
            <ChevronRight aria-hidden="true" className="size-4 shrink-0 text-chat-foreground-secondary" />
          </button>
        </aside>
      ) : null}
      {snapshot && !snapshot.setupRequired ? (
        <div data-testid="phone-panel-composer" className="shrink-0 border-t border-chat-border bg-chat-header px-3 py-2 pb-[max(0.5rem,env(safe-area-inset-bottom))]">
          <Composer touch sessions={mentions} seed={seed} onSend={async (text, refs) => toComposerResult(await actions.converse(text, refs))} />
        </div>
      ) : null}
    </div>
  );
}
