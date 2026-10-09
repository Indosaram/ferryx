import { useCallback, useEffect, useRef, useState } from "react";
import { Mic, Square } from "lucide-react";
import { cn } from "../../lib/cn";
import {
  VOICE_STATUS,
  appendTranscript,
  createBrowserRecognition,
  type VoicePhaseState,
  type VoiceRecognitionFactory,
  type VoiceTranscriptEvent,
  VoiceToDraftAdapter,
} from "./voiceToDraft";

export interface VoiceDictationControlProps {
  /** Stable identity of the target the draft belongs to (e.g. draftKey). */
  readonly targetKey: string;
  /** Bumped by the composer whenever the draft is sent or cleared. */
  readonly draftRevision?: number;
  readonly value: string;
  readonly onChange: (next: string) => void;
  readonly createRecognition?: VoiceRecognitionFactory;
  readonly disabled?: boolean;
  readonly className?: string;
}

/**
 * Microphone control that dictates into an editable chat draft.
 *
 * Contract: start only on explicit user activation; interim results are
 * preview-only; only final results commit into `value`, fenced by target key
 * and draft revision so a late result can never reach another target or a
 * newer draft. This component never sends: it only reports draft edits.
 * Vendor-managed speech processing is disclosed while listening.
 */
export function VoiceDictationControl({
  targetKey,
  draftRevision = 0,
  value,
  onChange,
  createRecognition = createBrowserRecognition,
  disabled = false,
  className,
}: VoiceDictationControlProps) {
  const targetKeyRef = useRef(targetKey);
  const draftRevisionRef = useRef(draftRevision);
  const valueRef = useRef(value);
  const onChangeRef = useRef(onChange);

  targetKeyRef.current = targetKey;
  draftRevisionRef.current = draftRevision;
  valueRef.current = value;
  onChangeRef.current = onChange;

  const [adapter] = useState(
    () =>
      new VoiceToDraftAdapter({
        createRecognition,
        onPhase: (state) => setPhaseState(state),
        onInterim: (event) => handleInterim(event),
        onFinal: (event) => handleFinal(event),
        targetKey,
        draftRevision,
      })
  );
  const [phaseState, setPhaseState] = useState<VoicePhaseState>(adapter.state);
  const [interim, setInterim] = useState<string | null>(null);

  const isCurrent = useCallback(
    (event: VoiceTranscriptEvent) =>
      event.targetKey === targetKeyRef.current &&
      event.draftRevision === draftRevisionRef.current,
    []
  );

  const handleInterim = useCallback(
    (event: VoiceTranscriptEvent) => {
      if (!isCurrent(event)) return;
      setInterim(event.text);
    },
    [isCurrent]
  );

  const handleFinal = useCallback(
    (event: VoiceTranscriptEvent) => {
      if (!isCurrent(event)) return;
      setInterim(null);
      onChangeRef.current(appendTranscript(valueRef.current, event.text));
    },
    [isCurrent]
  );

  useEffect(() => {
    return () => {
      adapter.dispose();
    };
  }, [adapter]);

  useEffect(() => {
    adapter.setTargetKey(targetKey);
    adapter.setDraftRevision(draftRevision);
  }, [adapter, targetKey, draftRevision]);

  useEffect(() => {
    if (disabled && adapter.listening) adapter.cancel();
  }, [adapter, disabled]);

  useEffect(() => {
    if (phaseState.phase !== "listening") setInterim(null);
  }, [phaseState.phase]);

  const listening = phaseState.phase === "listening";
  const unsupported = phaseState.phase === "unsupported";
  const statusText = phaseState.message;

  const handleToggle = useCallback(() => {
    if (disabled || unsupported) return;
    if (adapter.listening) adapter.cancel();
    else adapter.start();
  }, [adapter, disabled, unsupported]);

  return (
    <div className={cn("flex min-w-0 flex-col gap-1", className)}>
      <div className="flex items-center gap-2">
        <button
          type="button"
          data-testid="voice-dictation-button"
          aria-label={
            unsupported
              ? "Voice input is not supported in this browser"
              : listening
                ? "Stop voice dictation"
                : "Start voice dictation"
          }
          aria-pressed={listening}
          disabled={disabled || unsupported}
          title={
            unsupported
              ? VOICE_STATUS.unsupported
              : listening
                ? VOICE_STATUS.listening
                : "Start voice dictation"
          }
          onClick={handleToggle}
          className={cn(
            "flex size-9 shrink-0 items-center justify-center rounded-full border border-chat-border bg-chat-composer-surface text-chat-foreground-tertiary transition-colors hover:bg-chat-surface-hover hover:text-chat-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-40",
            listening && "border-chat-danger text-chat-danger"
          )}
        >
          {listening ? (
            <Square className="size-4" aria-hidden="true" />
          ) : (
            <Mic className="size-4" aria-hidden="true" />
          )}
        </button>
        {interim && (
          <span
            data-testid="voice-dictation-interim"
            className="min-w-0 flex-1 truncate text-xs italic text-chat-foreground-tertiary"
          >
            {interim}
          </span>
        )}
      </div>
      {statusText && (
        <span
          role="status"
          data-testid="voice-dictation-status"
          className="text-xs text-chat-foreground-secondary"
        >
          {statusText}
        </span>
      )}
    </div>
  );
}
