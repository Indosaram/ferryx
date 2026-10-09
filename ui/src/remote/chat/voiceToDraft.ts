/**
 * Voice-to-draft adapter for the mobile managed-chat composer.
 *
 * Provider: the browser Web Speech API (`SpeechRecognition` /
 * `webkitSpeechRecognition`). This is the only speech path available in the
 * product today that needs no first-party service, no API key and no new paid
 * dependency, so it is the concrete provider this module binds to.
 *
 * Privacy / network contract (stated explicitly, no on-device claim):
 * - This module performs no network I/O of its own.
 * - Recognition itself is browser-vendor managed. In Chromium browsers the
 *   captured audio is streamed to the vendor's speech service (Google for
 *   Chrome, Microsoft for Edge); Safari's implementation is Apple-managed.
 *   Whether any of those vendors process on-device or remotely is their
 *   decision and can change per platform, so this code never claims
 *   local-only or on-device processing. The UI discloses vendor processing at
 *   the moment recognition starts.
 * - Microphone capture starts only from an explicit user activation of the
 *   dictation control; nothing here starts recognition on its own.
 *
 * Delivery contract:
 * - Only `isFinal` results are surfaced through `onFinal`; interim results go
 *   to `onInterim` and are preview-only, never draft commits.
 * - Every emitted event is fenced by session id, target key and draft
 *   revision. Results that arrive after `cancel()`, after the session ended,
 *   or after the target/draft fence moved are dropped, never queued.
 * - This module cannot send anything: it has no send surface. Transcripts are
 *   handed to the caller as draft edits and sending stays an explicit user
 *   action in the composer.
 */

export type VoiceDictationPhase =
  | "unsupported"
  | "idle"
  | "listening"
  | "denied"
  | "failed";

export interface SpeechRecognitionResultLike {
  readonly isFinal: boolean;
  readonly transcript: string;
}

export interface SpeechRecognitionEventLike {
  readonly resultIndex: number;
  readonly results: ArrayLike<SpeechRecognitionResultLike>;
}

export interface SpeechRecognitionErrorEventLike {
  readonly error: string;
  readonly message?: string;
}

export interface SpeechRecognitionLike {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  onresult: ((event: SpeechRecognitionEventLike) => void) | null;
  onerror: ((event: SpeechRecognitionErrorEventLike) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
  abort(): void;
}

export type SpeechRecognitionCtor = new () => SpeechRecognitionLike;

export interface SpeechRecognitionGlobal {
  readonly SpeechRecognition?: SpeechRecognitionCtor;
  readonly webkitSpeechRecognition?: SpeechRecognitionCtor;
}

export interface VoiceTranscriptEvent {
  /** Monotonic session id; increments on every start. */
  readonly sessionId: number;
  /** Fence captured when the session started. */
  readonly targetKey: string;
  /** Draft revision captured when the session started. */
  readonly draftRevision: number;
  readonly text: string;
}

export interface VoicePhaseState {
  readonly phase: VoiceDictationPhase;
  readonly message: string | null;
}

export type VoiceRecognitionFactory = () => SpeechRecognitionLike | null;

export interface VoiceToDraftOptions {
  readonly createRecognition: VoiceRecognitionFactory;
  readonly onPhase: (state: VoicePhaseState) => void;
  readonly onInterim: (event: VoiceTranscriptEvent) => void;
  readonly onFinal: (event: VoiceTranscriptEvent) => void;
  readonly targetKey: string;
  readonly draftRevision?: number;
}

/** Honest copy for every non-listening phase. UI renders these verbatim. */
export const VOICE_STATUS = {
  unsupported: "Voice input isn't available in this browser.",
  listening:
    "Listening — audio is processed by your browser's speech service.",
  deniedPermission: "Microphone permission denied.",
  deniedService: "Speech recognition is blocked for this page.",
  noSpeech: "No speech detected.",
  network: "Speech service unreachable. Check your connection.",
  startFailed: "Voice input could not start.",
} as const;

export function detectSpeechRecognition(
  scope?: SpeechRecognitionGlobal
): SpeechRecognitionCtor | null {
  const host =
    scope ??
    (globalThis as typeof globalThis & SpeechRecognitionGlobal | undefined);
  if (!host) return null;
  const ctor = host.SpeechRecognition ?? host.webkitSpeechRecognition ?? null;
  return typeof ctor === "function" ? ctor : null;
}

/** Default factory: browser API when present, `null` means unsupported. */
export function createBrowserRecognition(): SpeechRecognitionLike | null {
  const ctor = detectSpeechRecognition();
  if (!ctor) return null;
  try {
    return new ctor();
  } catch {
    return null;
  }
}

/**
 * Appends a committed (final) transcript to the current draft text without
 * clobbering what the user already typed. Never mutates or sends.
 */
export function appendTranscript(current: string, next: string): string {
  const tail = next.trim();
  if (!tail) return current;
  const head = current;
  if (!head) return tail;
  if (/[\s\n]$/.test(head)) return `${head}${tail}`;
  return `${head} ${tail}`;
}

interface VoiceSession {
  readonly id: number;
  readonly targetKey: string;
  readonly draftRevision: number;
}

export class VoiceToDraftAdapter {
  private readonly createRecognition: VoiceRecognitionFactory;
  private readonly onPhase: (state: VoicePhaseState) => void;
  private readonly onInterim: (event: VoiceTranscriptEvent) => void;
  private readonly onFinal: (event: VoiceTranscriptEvent) => void;

  private recognition: SpeechRecognitionLike | null = null;
  private active: VoiceSession | null = null;
  private sessionSequence = 0;
  private targetKey: string;
  private draftRevision: number;
  private phaseState: VoicePhaseState;

  constructor(options: VoiceToDraftOptions) {
    this.createRecognition = options.createRecognition;
    this.onPhase = options.onPhase;
    this.onInterim = options.onInterim;
    this.onFinal = options.onFinal;
    this.targetKey = options.targetKey;
    this.draftRevision = options.draftRevision ?? 0;
    this.recognition = this.instantiate();
    this.phaseState = this.recognition
      ? { phase: "idle", message: null }
      : { phase: "unsupported", message: VOICE_STATUS.unsupported };
  }

  get supported(): boolean {
    return this.recognition !== null;
  }

  get state(): VoicePhaseState {
    return this.phaseState;
  }

  get listening(): boolean {
    return this.active !== null;
  }

  /** Current session id, or null when no session is active. */
  get currentSessionId(): number | null {
    return this.active?.id ?? null;
  }

  /**
   * Moves the target fence. A listening session started against another
   * target is cancelled immediately so late results cannot cross targets.
   */
  setTargetKey(targetKey: string): void {
    if (this.targetKey === targetKey) return;
    this.targetKey = targetKey;
    if (this.active && this.active.targetKey !== targetKey) this.cancel();
  }

  /**
   * Moves the draft fence (e.g. after the draft was sent or cleared). A
   * listening session started against the older revision is cancelled so a
   * late result cannot land in the newer draft.
   */
  setDraftRevision(draftRevision: number): void {
    if (this.draftRevision === draftRevision) return;
    this.draftRevision = draftRevision;
    if (this.active && this.active.draftRevision !== draftRevision) this.cancel();
  }

  /**
   * Starts a new session. Returns false when the service is unsupported or a
   * session is already active. Must be reached from a user activation so the
   * browser can attach its own microphone permission prompt.
   */
  start(): boolean {
    if (this.phaseState.phase === "unsupported") return false;
    if (this.active) return false;
    if (!this.recognition) this.recognition = this.instantiate();
    const recognition = this.recognition;
    if (!recognition) {
      this.setPhase("unsupported", VOICE_STATUS.unsupported);
      return false;
    }

    const session: VoiceSession = {
      id: ++this.sessionSequence,
      targetKey: this.targetKey,
      draftRevision: this.draftRevision,
    };

    recognition.continuous = true;
    recognition.interimResults = true;
    recognition.onresult = (event) => this.handleResult(session, event);
    recognition.onerror = (event) => this.handleError(session, event);
    recognition.onend = () => this.handleEnd(session);

    try {
      recognition.start();
    } catch {
      this.detach(recognition);
      this.setPhase("failed", VOICE_STATUS.startFailed);
      return false;
    }

    this.active = session;
    this.setPhase("listening", VOICE_STATUS.listening);
    return true;
  }

  /** Cancels the active session; any later result is fenced out. */
  cancel(): void {
    const recognition = this.recognition;
    this.active = null;
    if (recognition) this.detach(recognition);
    if (recognition && this.phaseState.phase === "listening") {
      try {
        recognition.abort();
      } catch {
        void 0;
      }
    }
    if (this.phaseState.phase !== "unsupported") {
      this.setPhase("idle", null);
    }
  }

  /** Cancel and drop handlers; safe to call from component unmount. */
  dispose(): void {
    this.cancel();
    if (this.recognition) {
      this.detach(this.recognition);
      this.recognition = null;
    }
  }

  private instantiate(): SpeechRecognitionLike | null {
    try {
      return this.createRecognition();
    } catch {
      return null;
    }
  }

  private detach(recognition: SpeechRecognitionLike): void {
    recognition.onresult = null;
    recognition.onerror = null;
    recognition.onend = null;
  }

  private isCurrent(session: VoiceSession): boolean {
    const active = this.active;
    if (!active) return false;
    return (
      active.id === session.id &&
      active.targetKey === session.targetKey &&
      active.draftRevision === session.draftRevision &&
      active.targetKey === this.targetKey &&
      active.draftRevision === this.draftRevision
    );
  }

  private handleResult(
    session: VoiceSession,
    event: SpeechRecognitionEventLike
  ): void {
    if (!this.isCurrent(session)) return;
    const results = event.results;
    if (!results) return;
    const from = Math.max(0, event.resultIndex ?? 0);
    for (let i = from; i < results.length; i += 1) {
      const result = results[i];
      if (!result) continue;
      const text = (result.transcript ?? "").trim();
      if (!text) continue;
      const payload: VoiceTranscriptEvent = {
        sessionId: session.id,
        targetKey: session.targetKey,
        draftRevision: session.draftRevision,
        text,
      };
      if (result.isFinal) this.onFinal(payload);
      else this.onInterim(payload);
    }
  }

  private handleError(
    session: VoiceSession,
    event: SpeechRecognitionErrorEventLike
  ): void {
    if (!this.isCurrent(session)) return;
    const code = event.error ?? "unknown";
    const recognition = this.recognition;
    this.active = null;
    if (recognition) this.detach(recognition);

    if (code === "not-allowed") {
      this.setPhase("denied", VOICE_STATUS.deniedPermission);
    } else if (code === "service-not-allowed") {
      this.setPhase("denied", VOICE_STATUS.deniedService);
    } else if (code === "aborted") {
      this.setPhase("idle", null);
    } else if (code === "no-speech") {
      this.setPhase("failed", VOICE_STATUS.noSpeech);
    } else if (code === "network") {
      this.setPhase("failed", VOICE_STATUS.network);
    } else {
      this.setPhase("failed", `${VOICE_STATUS.startFailed} (${code})`);
    }
  }

  private handleEnd(session: VoiceSession): void {
    if (!this.isCurrent(session)) return;
    this.active = null;
    if (this.phaseState.phase === "listening") this.setPhase("idle", null);
  }

  private setPhase(phase: VoiceDictationPhase, message: string | null): void {
    this.phaseState = { phase, message };
    this.onPhase(this.phaseState);
  }
}
