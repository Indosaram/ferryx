// The supervisor call: a full-screen surface where the owner speaks and the supervisor answers
// with its own receipt. The orb is the whole state indicator, the transcript is the evidence, and
// the typed composer stays available when speech is not.
import { AudioLines, Mic, PhoneOff, Square } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { cn } from "../../lib/cn";
import { useModalDialog } from "../useModalDialog";
import type { UnifiedSupervisorClient, UnifiedSupervisorView } from "../../lib/unifiedSupervisor";
import { answerVoiceTurn, type VoiceCallPort, type VoiceCallStatus } from "../../lib/voiceCall";

type Phase = "opening" | "unavailable" | "idle" | "listening" | "transcribing" | "thinking" | "speaking" | "error";

const PHASE_LABEL: Record<Phase, string> = {
  opening: "Connecting",
  unavailable: "Not available",
  idle: "Ready",
  listening: "Listening",
  transcribing: "Transcribing",
  thinking: "Thinking",
  speaking: "Speaking",
  error: "Stopped",
};

const ORB_TONE: Record<Phase, string> = {
  opening: "bg-muted",
  unavailable: "bg-muted",
  idle: "bg-primary/20",
  listening: "bg-status-success/25",
  transcribing: "bg-status-working/25",
  thinking: "bg-status-warning/25",
  speaking: "bg-primary/40",
  error: "bg-destructive/30",
};

type Line = { id: number; who: "owner" | "supervisor"; text: string };

export function VoiceCallSurface({
  view,
  client,
  projectId,
  port,
  onClose,
}: {
  view: UnifiedSupervisorView | null;
  client: UnifiedSupervisorClient;
  projectId: string | null;
  port: VoiceCallPort;
  onClose: () => void;
}) {
  const [phase, setPhase] = useState<Phase>("opening");
  const [status, setStatus] = useState<VoiceCallStatus | null>(null);
  const [lines, setLines] = useState<Line[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const nextId = useRef(1);
  const alive = useRef(true);
  const busy = useRef(false);
  // Closing the call is always allowed: the surface tears down capture and playback on unmount.
  const { dialogRef } = useModalDialog({ onEscape: onClose });

  const say = useCallback((who: Line["who"], text: string) => {
    if (!text.trim()) return;
    setLines((current) => [...current, { id: nextId.current++, who, text }]);
  }, []);

  useEffect(() => {
    alive.current = true;
    void port
      .status()
      .then((next) => {
        if (!alive.current) return;
        setStatus(next);
        setPhase(next.available ? "idle" : "unavailable");
        if (!next.available) setError(next.reason ?? "Voice is not available on this computer.");
      })
      .catch((cause: unknown) => {
        if (!alive.current) return;
        setPhase("unavailable");
        setError(cause instanceof Error ? cause.message : String(cause));
      });
    return () => {
      alive.current = false;
      port.stopRecording();
      port.stopPlayback();
    };
  }, [port]);

  /** One turn: record, transcribe, ask the hub, speak its answer. */
  const listen = useCallback(async () => {
    if (busy.current || !status?.available) return;
    busy.current = true;
    setError(null);
    setPhase("listening");
    try {
      const wav = await port.record(status);
      if (!alive.current) return;
      setPhase("transcribing");
      const transcript = await port.transcribe(wav);
      if (!alive.current) return;
      if (!transcript.trim()) {
        setPhase("idle");
        return;
      }
      say("owner", transcript);
      setPhase("thinking");
      const turn = await answerVoiceTurn({ transcript, client, view, projectId });
      if (!alive.current) return;
      if (turn.mutated) client.refresh();
      say("supervisor", turn.speech);
      // Without a paid key there is no synthesis: the turn still happened and the answer is
      // readable above. Falling through to speak() would surface a provider error instead.
      if (!turn.speech.trim() || !status.speakAvailable) {
        setPhase("idle");
        return;
      }
      setPhase("speaking");
      const spoken = await port.speak(turn.speech);
      if (!alive.current) return;
      await port.play(spoken);
      if (!alive.current) return;
      setPhase("idle");
    } catch (cause) {
      if (!alive.current) return;
      const message = cause instanceof Error ? cause.message : String(cause);
      if (message === "nothing-recorded") {
        // A take with no speech is not an error the owner needs to read.
        setPhase("idle");
        return;
      }
      setError(message);
      setPhase("error");
    } finally {
      busy.current = false;
    }
  }, [client, port, projectId, say, status, view]);

  /** The orb: start a take, stop one, or interrupt the spoken answer. */
  const tapOrb = () => {
    if (phase === "listening") {
      port.stopRecording();
      return;
    }
    if (phase === "speaking") {
      port.stopPlayback();
      setPhase("idle");
      return;
    }
    if (phase === "idle" || phase === "error") void listen();
  };

  const sendTyped = async () => {
    const text = draft.trim();
    if (!text || busy.current) return;
    setDraft("");
    say("owner", text);
    busy.current = true;
    setPhase("thinking");
    try {
      const turn = await answerVoiceTurn({ transcript: text, client, view, projectId });
      if (!alive.current) return;
      if (turn.mutated) client.refresh();
      say("supervisor", turn.speech);
      setPhase("idle");
    } catch (cause) {
      if (!alive.current) return;
      setError(cause instanceof Error ? cause.message : String(cause));
      setPhase("error");
    } finally {
      busy.current = false;
    }
  };

  const orbBusy = phase === "opening" || phase === "transcribing" || phase === "thinking";
  const orbLabel = phase === "speaking" ? "Stop speaking" : phase === "listening" ? "Stop listening" : "Start talking";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm">
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label="Supervisor call"
        data-testid="voice-call"
        data-phase={phase}
        className="flex h-full max-h-[720px] w-full max-w-2xl animate-enter flex-col overflow-hidden rounded-2xl border border-border bg-popover shadow-elevation-4"
      >
        <header className="flex items-center gap-2 border-b border-border px-4 py-3 text-sm font-semibold text-foreground">
          <AudioLines className="size-4 text-muted-foreground" aria-hidden />
          <span>Supervisor call</span>
          <span className="ml-auto font-mono text-[10px] font-normal text-muted-foreground" data-testid="voice-call-phase">
            {PHASE_LABEL[phase]}
          </span>
        </header>

        <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-5 px-6 py-8">
          <button
            type="button"
            onClick={tapOrb}
            disabled={orbBusy || phase === "unavailable"}
            aria-label={orbLabel}
            data-testid="voice-call-orb"
            className={cn(
              "relative flex size-40 shrink-0 items-center justify-center rounded-full transition-transform",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60",
              ORB_TONE[phase],
              phase === "listening" && "animate-pulse",
            )}
          >
            <span className="absolute inset-4 rounded-full border border-white/10" aria-hidden />
            {phase === "speaking" ? (
              <Square className="size-10 text-foreground" aria-hidden />
            ) : (
              <Mic className="size-10 text-foreground" aria-hidden />
            )}
          </button>

          <p className="text-[11px] leading-snug text-muted-foreground" data-testid="voice-call-hint">
            {phase === "listening"
              ? "Speak. It stops when you pause."
              : phase === "speaking"
                ? "Tap to interrupt."
                : phase === "unavailable"
                  ? "Speech is not configured here. Type below instead."
                  : "Tap and speak a task, or say \"status\"."}
          </p>

          {error ? (
            <p role="alert" data-testid="voice-call-error" className="max-w-md text-center text-[11px] leading-snug text-destructive">
              {error}
            </p>
          ) : null}

          {lines.length > 0 ? (
            <ol
              data-testid="voice-call-transcript"
              className="flex max-h-52 w-full flex-col gap-1.5 overflow-y-auto scrollbar-sleek text-[12px] leading-snug"
            >
              {lines.map((line) => (
                <li
                  key={line.id}
                  data-who={line.who}
                  className={cn(
                    "max-w-[85%] rounded-lg px-3 py-2",
                    line.who === "owner" ? "self-end bg-primary/15 text-foreground" : "self-start bg-card text-foreground/90",
                  )}
                >
                  {line.text}
                </li>
              ))}
            </ol>
          ) : null}
        </div>

        <footer className="flex items-center gap-2 border-t border-border px-4 py-3">
          <input
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                event.preventDefault();
                void sendTyped();
              }
            }}
            aria-label="Type instead of speaking"
            placeholder="Type instead of speaking"
            data-testid="voice-call-input"
            className="min-w-0 flex-1 rounded-md border border-border bg-background px-3 py-2 text-[12px] text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          />
          <button
            type="button"
            onClick={() => void sendTyped()}
            disabled={!draft.trim() || orbBusy}
            data-testid="voice-call-send"
            className="rounded-md bg-primary px-3 py-2 text-[12px] font-medium text-primary-foreground disabled:opacity-50"
          >
            Send
          </button>
          <button
            type="button"
            onClick={onClose}
            aria-label="End the call"
            data-testid="voice-call-end"
            className="inline-flex size-9 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
          >
            <PhoneOff className="size-4" aria-hidden />
          </button>
        </footer>
      </div>
    </div>
  );
}

