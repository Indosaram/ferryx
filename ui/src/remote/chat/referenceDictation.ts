/**
 * Probed speech-insertion adapter for the reference chat (plan T11, section 4.5).
 *
 * OS keyboard dictation stays the baseline. This module wraps SpeechRecognition /
 * webkitSpeechRecognition only after a runtime probe proves the constructor exists in
 * the current shell. It does not request a native audio permission, start a native
 * speech engine, or call a hosted transcription service.
 *
 * Partial and final transcripts are reported to the caller. Nothing here sends,
 * submits, or settles a draft. The composer inserts them with `insertAtCaret` and
 * owns the mic control. A result that arrives after `stop`, `discard`, or a target
 * change is dropped. `discard` also forgets the pending partial so it cannot be
 * finalized into another pane. A later `start` also forgets it: the new utterance
 * does not own the previous span.
 */

/** Engine codes the composer can show. Unmapped engine failures are `unknown`. */
export type DictationErrorCode =
  | "permission-denied"
  | "not-allowed"
  | "no-speech"
  | "network"
  | "unknown";

export interface DictationError {
  readonly code: DictationErrorCode;
  readonly message: string;
}

export type DictationUnavailableReason = "missing-constructor" | "not-constructible";

export interface DictationCallbacks {
  /** Interim transcript for the active target. Never a submit. */
  readonly onPartial: (text: string) => void;
  /** Final transcript for the active target. Never a submit. */
  readonly onFinal: (text: string) => void;
  readonly onError: (error: DictationError) => void;
}

export interface DictationSession {
  /** Begin recognition fenced to `targetKey`. A different key drops the previous partial. */
  start(targetKey: string): void;
  /** Stop recognition. Late results are dropped; the pending partial is kept. */
  stop(): void;
  /** Stop recognition, drop late results, and forget the pending partial. */
  discard(): void;
  /** Interim text not yet finalized. Null after `discard` or a target change. */
  readonly pendingPartial: string | null;
}

export type DictationProbe =
  | {
      readonly available: true;
      readonly create: (callbacks: DictationCallbacks) => DictationSession;
    }
  | {
      readonly available: false;
      readonly reason: DictationUnavailableReason;
    };

interface DictationAlternative {
  readonly transcript?: string;
}

interface DictationResult {
  readonly isFinal?: boolean;
  readonly length?: number;
  readonly transcript?: string;
  item?: (index: number) => DictationAlternative;
  readonly [index: number]: DictationAlternative | undefined;
}

interface DictationResultEvent {
  readonly resultIndex?: number;
  readonly results?: ArrayLike<DictationResult> & { item?: (index: number) => DictationResult };
}

interface DictationErrorEvent {
  readonly error?: string;
  readonly message?: string;
}

interface DictationEngine {
  continuous: boolean;
  interimResults: boolean;
  start: () => void;
  stop: () => void;
  abort?: () => void;
  onresult: ((event: DictationResultEvent) => void) | null;
  onerror: ((event: DictationErrorEvent) => void) | null;
  onend: (() => void) | null;
}

type DictationConstructor = new () => DictationEngine;

/**
 * Insert `text` in place of the selection and leave the caret after it.
 *
 * A collapsed selection inserts at the caret. A range replaces that span: the composer
 * passes the interim partial's span as the selection when a later partial or the final
 * transcript arrives, so the final text replaces the partial instead of appending to it.
 * Indices are clamped. This does not submit.
 */
export function insertAtCaret(
  value: string,
  selectionStart: number,
  selectionEnd: number,
  text: string,
): { readonly value: string; readonly caret: number } {
  const start = clampIndex(value, selectionStart);
  const end = clampIndex(value, selectionEnd);
  const from = Math.min(start, end);
  const to = Math.max(start, end);
  const next = value.slice(0, from) + text + value.slice(to);
  return { value: next, caret: from + text.length };
}

/**
 * Map a SpeechRecognition error onto the typed union. `service-not-allowed` and a
 * permission-denial message are `permission-denied`. Anything else is `unknown`.
 * This never throws.
 */
export function dictationErrorFrom(event: DictationErrorEvent | string | null | undefined): DictationError {
  const error = typeof event === "string" ? event : typeof event?.error === "string" ? event.error : "";
  const message = typeof event === "string" ? event : typeof event?.message === "string" ? event.message : "";
  const code = classify(normalize(error)) ?? classify(normalize(message)) ?? "unknown";
  return { code, message: message || error || "speech-recognition-error" };
}

/**
 * Probe the shell that will host the composer. `win` is `window` in the app and a fake
 * in tests. The probe constructs the candidate once; it does not call `start`, so it
 * does not ask for a microphone. SpeechRecognition wins over the webkit prefix.
 */
export function probeDictation(win: object | null | undefined): DictationProbe {
  if (win === null || win === undefined || typeof win !== "object") {
    return { available: false, reason: "missing-constructor" };
  }
  const host = win as { SpeechRecognition?: unknown; webkitSpeechRecognition?: unknown };
  if (host.SpeechRecognition === undefined && host.webkitSpeechRecognition === undefined) {
    return { available: false, reason: "missing-constructor" };
  }
  const ctor = asConstructor(host.SpeechRecognition) ?? asConstructor(host.webkitSpeechRecognition);
  if (!ctor) return { available: false, reason: "not-constructible" };
  return { available: true, create: (callbacks) => createSession(ctor, callbacks) };
}

function createSession(ctor: DictationConstructor, callbacks: DictationCallbacks): DictationSession {
  let generation = 0;
  let phase: "idle" | "listening" | "stopped" | "discarded" = "idle";
  let pendingPartial: string | null = null;
  let recognition: DictationEngine | null = null;
  let targetKey: string | null = null;

  const accepts = (token: number): boolean => token === generation && phase === "listening";

  const abandon = (dropPartial: boolean): void => {
    generation += 1;
    phase = dropPartial ? "discarded" : "stopped";
    if (dropPartial) pendingPartial = null;
    const previous = recognition;
    recognition = null;
    detach(previous);
    endEngine(previous, dropPartial);
  };

  const handleResult = (token: number, event: DictationResultEvent): void => {
    try {
      if (!accepts(token)) return;
      const read = readTranscripts(event);
      if (read.finalText) {
        pendingPartial = null;
        emitFinal(callbacks, read.finalText);
      }
      if (!accepts(token)) return;
      if (read.partialText) {
        pendingPartial = read.partialText;
        emitPartial(callbacks, read.partialText);
      }
    } catch (error) {
      if (token !== generation) return;
      abandon(false);
      emitError(callbacks, mapThrown(error));
    }
  };

  const handleError = (token: number, event: DictationErrorEvent): void => {
    try {
      if (!accepts(token)) return;
      const mapped = dictationErrorFrom(event);
      abandon(false);
      emitError(callbacks, mapped);
    } catch (error) {
      if (token !== generation) return;
      abandon(false);
      emitError(callbacks, mapThrown(error));
    }
  };

  const handleEnd = (token: number): void => {
    if (token !== generation || phase !== "listening") return;
    phase = "idle";
    const previous = recognition;
    recognition = null;
    detach(previous);
  };

  const session: DictationSession = {
    start(nextKey: string): void {
      try {
        if (phase === "listening" && targetKey === nextKey && recognition) return;
        abandon(true);
        targetKey = nextKey;
        const token = generation;
        let created: DictationEngine;
        try {
          created = new ctor();
          created.continuous = true;
          created.interimResults = true;
          created.onresult = (event) => handleResult(token, event);
          created.onerror = (event) => handleError(token, event);
          created.onend = () => handleEnd(token);
        } catch (error) {
          emitError(callbacks, mapThrown(error));
          return;
        }
        recognition = created;
        phase = "listening";
        try {
          created.start();
        } catch (error) {
          if (accepts(token)) {
            abandon(false);
            emitError(callbacks, mapThrown(error));
          }
        }
      } catch (error) {
        emitError(callbacks, mapThrown(error));
      }
    },
    stop(): void {
      try {
        abandon(false);
      } catch (error) {
        emitError(callbacks, mapThrown(error));
      }
    },
    discard(): void {
      try {
        abandon(true);
      } catch (error) {
        emitError(callbacks, mapThrown(error));
      }
    },
    get pendingPartial(): string | null {
      return pendingPartial;
    },
  };
  return session;
}

function asConstructor(value: unknown): DictationConstructor | null {
  if (typeof value !== "function") return null;
  try {
    const instance = new (value as DictationConstructor)();
    if (instance === null || typeof instance !== "object") return null;
    return value as DictationConstructor;
  } catch {
    return null;
  }
}

function detach(engine: DictationEngine | null): void {
  if (!engine) return;
  try {
    engine.onresult = null;
  } catch {
    /* a foreign engine may freeze its handlers */
  }
  try {
    engine.onerror = null;
  } catch {
    /* a foreign engine may freeze its handlers */
  }
  try {
    engine.onend = null;
  } catch {
    /* a foreign engine may freeze its handlers */
  }
}

function endEngine(engine: DictationEngine | null, discardAudio: boolean): void {
  if (!engine) return;
  try {
    if (discardAudio && typeof engine.abort === "function") engine.abort();
    else engine.stop();
  } catch {
    /* already ended, or the engine rejects stop after it has ended */
  }
}

function readTranscripts(event: DictationResultEvent | null | undefined): { finalText: string; partialText: string } {
  if (!event) return { finalText: "", partialText: "" };
  const results = event.results;
  if (!results || typeof results.length !== "number") return { finalText: "", partialText: "" };
  const resultIndex = event.resultIndex;
  const start = typeof resultIndex === "number" && Number.isFinite(resultIndex) ? Math.max(0, Math.trunc(resultIndex)) : 0;
  let finalText = "";
  let partialText = "";
  for (let index = start; index < results.length; index += 1) {
    const result = results[index] ?? results.item?.(index);
    if (!result) continue;
    const transcript = transcriptOf(result);
    if (!transcript) continue;
    if (result.isFinal) finalText += transcript;
    else partialText += transcript;
  }
  return { finalText, partialText };
}

function transcriptOf(result: DictationResult): string {
  const alternative = result[0] ?? result.item?.(0);
  const transcript = alternative?.transcript ?? result.transcript;
  return typeof transcript === "string" ? transcript : "";
}

function mapThrown(error: unknown): DictationError {
  const name = error && typeof error === "object" && "name" in error ? String((error as { name: unknown }).name) : "";
  const message =
    error && typeof error === "object" && "message" in error
      ? String((error as { message: unknown }).message)
      : "speech-recognition-error";
  if (name === "NotAllowedError") return { code: "not-allowed", message };
  if (name === "SecurityError") return { code: "permission-denied", message };
  return dictationErrorFrom({ error: name || message, message });
}

function emitPartial(callbacks: DictationCallbacks, text: string): void {
  try {
    callbacks.onPartial(text);
  } catch {
    /* a callback failure must not escape into the speech engine */
  }
}

function emitFinal(callbacks: DictationCallbacks, text: string): void {
  try {
    callbacks.onFinal(text);
  } catch {
    /* a callback failure must not escape into the speech engine */
  }
}

function emitError(callbacks: DictationCallbacks, error: DictationError): void {
  try {
    callbacks.onError(error);
  } catch {
    /* a callback failure must not escape into the speech engine */
  }
}

function normalize(value: string): string {
  return value.trim().toLowerCase().replace(/[\s_]+/g, "-");
}

function classify(value: string): DictationErrorCode | null {
  if (!value) return null;
  if (value === "service-not-allowed" || value.includes("service-not-allowed")) return "permission-denied";
  if (value === "not-allowed" || value.includes("not-allowed")) return "not-allowed";
  if (
    value === "permission-denied" ||
    value === "permission-denial" ||
    value.includes("permission-denied") ||
    value.includes("permission-denial") ||
    (value.includes("permission") && value.includes("deni"))
  ) {
    return "permission-denied";
  }
  if (value === "no-speech" || value.includes("no-speech")) return "no-speech";
  if (value === "network") return "network";
  return null;
}

function clampIndex(value: string, index: number): number {
  if (!Number.isFinite(index)) return value.length;
  const truncated = Math.trunc(index);
  if (truncated < 0) return 0;
  if (truncated > value.length) return value.length;
  return truncated;
}
