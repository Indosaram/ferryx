import { useCallback, useRef, useState } from "react";
import { Loader2, Mic, Square } from "lucide-react";
import { cn } from "../../lib/cn";
import type { VoiceCallPort, VoiceCallStatus } from "../../lib/voiceCall";
import { conversationAvailability, runConversationTurn, type ConversationTurn, type ConversationHost } from "../../lib/voiceConversation";

/** The seam the composer needs to draw a talk button; the surface supplies the port and its send. */
export type VoiceConversationProps = VoiceConversationButtonProps;

export interface VoiceConversationButtonProps {
  readonly port: VoiceCallPort;
  readonly host: ConversationHost;
  readonly status: VoiceCallStatus | null;
  readonly speakReplies?: boolean;
  readonly disabled?: boolean;
  readonly className?: string;
  readonly onTurn?: (turn: ConversationTurn) => void;
}

type Phase = "idle" | "recording" | "working" | "playing";

export function VoiceConversationButton({
  port,
  host,
  status,
  speakReplies = true,
  disabled = false,
  className,
  onTurn,
}: VoiceConversationButtonProps) {
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState<string | null>(null);
  const busy = useRef(false);

  const unavailable = conversationAvailability(status);
  const blocked = disabled || unavailable !== null;

  const start = useCallback(async () => {
    if (!status || busy.current || blocked) return;
    busy.current = true;
    setError(null);
    setPhase("recording");
    try {
      const result = await runConversationTurn({ port, status, host, speak: speakReplies });
      if (result.error) setError(result.error);
      else if (result.turn) onTurn?.(result.turn);
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "The voice turn failed.");
    } finally {
      busy.current = false;
      setPhase("idle");
    }
  }, [blocked, host, onTurn, port, speakReplies, status]);

  const stop = useCallback(() => {
    port.stopRecording();
    port.stopPlayback();
    setPhase("idle");
  }, [port]);

  const label =
    phase === "recording" ? "Stop recording" : phase === "playing" ? "Stop playback" : "Talk to this conversation";
  const title = unavailable ?? (phase === "recording" ? "Stop and send what you said" : label);

  return (
    <div className={cn("flex min-w-0 flex-col items-end gap-1", className)}>
      <button
        type="button"
        data-testid="voice-conversation-button"
        aria-label={label}
        aria-pressed={phase === "recording"}
        title={title}
        disabled={blocked || phase === "working"}
        onClick={() => (phase === "recording" || phase === "playing" ? stop() : void start())}
        className={cn(
          "flex size-11 shrink-0 items-center justify-center rounded-full transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:opacity-40",
          phase === "recording" ? "bg-chat-danger text-chat-screen" : "text-chat-foreground hover:bg-chat-surface-hover",
        )}
      >
        {phase === "working" ? (
          <Loader2 aria-hidden="true" className="size-5 animate-spin" />
        ) : phase === "recording" ? (
          <Square aria-hidden="true" className="size-3.5 fill-current" />
        ) : (
          <Mic aria-hidden="true" className="size-5" />
        )}
      </button>
      {error !== null && (
        <span data-testid="voice-conversation-error" role="alert" className="max-w-56 text-right text-xs text-chat-danger">
          {error}
        </span>
      )}
    </div>
  );
}
