/**
 * Dictation adapter tests (plan T11).
 *
 * Authored, NOT EXECUTED - the execution override forbids running any check during authoring.
 * Deferred command: bun run --cwd ui test -- src/remote/chat/referenceDictation.test.ts
 *
 * The fake SpeechRecognition is driven synchronously. No timers.
 */
import { describe, expect, it } from "vitest";
import {
  dictationErrorFrom,
  insertAtCaret,
  probeDictation,
  type DictationCallbacks,
  type DictationError,
  type DictationSession,
} from "./referenceDictation";

interface FakeRecognition {
  continuous: boolean;
  interimResults: boolean;
  onresult: ((event: unknown) => void) | null;
  onerror: ((event: unknown) => void) | null;
  onend: (() => void) | null;
  startCount: number;
  stopCount: number;
  abortCount: number;
  start(): void;
  stop(): void;
  abort(): void;
}

function recognitionHost(): { Fake: new () => FakeRecognition; instances: FakeRecognition[] } {
  const instances: FakeRecognition[] = [];
  class Fake implements FakeRecognition {
    continuous = false;
    interimResults = false;
    onresult: ((event: unknown) => void) | null = null;
    onerror: ((event: unknown) => void) | null = null;
    onend: (() => void) | null = null;
    startCount = 0;
    stopCount = 0;
    abortCount = 0;
    constructor() {
      instances.push(this);
    }
    start(): void {
      this.startCount += 1;
    }
    stop(): void {
      this.stopCount += 1;
    }
    abort(): void {
      this.abortCount += 1;
    }
  }
  return { Fake, instances };
}

function result(transcript: string, isFinal: boolean): unknown {
  return { resultIndex: 0, results: [{ isFinal, 0: { transcript }, length: 1 }] };
}

function latest(instances: readonly FakeRecognition[]): FakeRecognition {
  const found = instances.at(-1);
  if (!found) throw new Error("recognition was not constructed");
  return found;
}

function openSession(callbacks: DictationCallbacks): {
  session: DictationSession;
  instances: FakeRecognition[];
  live: () => FakeRecognition;
} {
  const { Fake, instances } = recognitionHost();
  const probe = probeDictation({ SpeechRecognition: Fake });
  if (!probe.available) throw new Error(probe.reason);
  const session = probe.create(callbacks);
  return { session, instances, live: () => latest(instances) };
}

describe("probed dictation", () => {
  it("ui-reference-dictation-1: an unavailable probe hides the mic and does not construct", () => {
    expect(probeDictation(undefined)).toEqual({ available: false, reason: "missing-constructor" });
    expect(probeDictation(null)).toEqual({ available: false, reason: "missing-constructor" });
    expect(probeDictation({})).toEqual({ available: false, reason: "missing-constructor" });

    let constructed = 0;
    class Broken {
      constructor() {
        constructed += 1;
        throw new Error("illegal");
      }
    }
    expect(probeDictation({ SpeechRecognition: Broken })).toEqual({
      available: false,
      reason: "not-constructible",
    });
    expect(constructed).toBe(1);
    expect(probeDictation({ SpeechRecognition: "SpeechRecognition" })).toEqual({
      available: false,
      reason: "not-constructible",
    });
    expect(probeDictation({ webkitSpeechRecognition: () => undefined })).toEqual({
      available: false,
      reason: "not-constructible",
    });
  });

  it("ui-reference-dictation-1b: a constructible webkit prefix is accepted, and the standard name wins", () => {
    const { Fake: Webkit, instances: webkitInstances } = recognitionHost();
    const webkitOnly = probeDictation({ webkitSpeechRecognition: Webkit });
    expect(webkitOnly.available).toBe(true);
    if (!webkitOnly.available) return;
    expect(webkitInstances).toHaveLength(1);
    expect(webkitInstances[0]?.startCount).toBe(0);

    const { Fake: Standard, instances: standardInstances } = recognitionHost();
    const both = probeDictation({ SpeechRecognition: Standard, webkitSpeechRecognition: Webkit });
    expect(both.available).toBe(true);
    if (!both.available) return;
    const session = both.create({ onPartial() {}, onFinal() {}, onError() {} });
    session.start("pane-a");
    expect(standardInstances).toHaveLength(2);
    expect(webkitInstances).toHaveLength(1);
    expect(latest(standardInstances).continuous).toBe(true);
    expect(latest(standardInstances).interimResults).toBe(true);
    expect(latest(standardInstances).startCount).toBe(1);
  });

  it("ui-reference-dictation-2: partial then final text replaces the interim span and does not submit", () => {
    let value = "Say ";
    let spanStart = value.length;
    let spanEnd = value.length;
    let caret = value.length;
    const partials: string[] = [];
    const finals: string[] = [];
    const { session, live } = openSession({
      onPartial(text) {
        partials.push(text);
        const next = insertAtCaret(value, spanStart, spanEnd, text);
        value = next.value;
        caret = next.caret;
        spanStart = caret - text.length;
        spanEnd = caret;
      },
      onFinal(text) {
        finals.push(text);
        const next = insertAtCaret(value, spanStart, spanEnd, text);
        value = next.value;
        caret = next.caret;
        spanStart = caret;
        spanEnd = caret;
      },
      onError(error) {
        throw new Error(error.code);
      },
    });

    session.start("pane-a");
    const engine = live();
    engine.onresult?.(result("hel", false));
    expect(partials).toEqual(["hel"]);
    expect(session.pendingPartial).toBe("hel");
    expect(value).toBe("Say hel");
    expect(caret).toBe(7);

    engine.onresult?.(result("hello", false));
    expect(value).toBe("Say hello");
    expect(caret).toBe(9);
    expect(session.pendingPartial).toBe("hello");

    engine.onresult?.(result("hello world", true));
    expect(finals).toEqual(["hello world"]);
    expect(value).toBe("Say hello world");
    expect(caret).toBe(15);
    expect(session.pendingPartial).toBeNull();
    expect("submit" in session || "send" in session).toBe(false);
  });

  it("ui-reference-dictation-3: a late result after the target changes is dropped", () => {
    const partials: string[] = [];
    const finals: string[] = [];
    const { session, live } = openSession({
      onPartial(text) {
        partials.push(text);
      },
      onFinal(text) {
        finals.push(text);
      },
      onError() {},
    });
    session.start("pane-a");
    const first = live();
    first.onresult?.(result("for a", false));
    expect(session.pendingPartial).toBe("for a");
    const late = first.onresult;
    session.start("pane-b");
    expect(session.pendingPartial).toBeNull();
    late?.(result("for a pane", true));
    expect(finals).toEqual([]);
    expect(partials).toEqual(["for a"]);
    live().onresult?.(result("for b", false));
    expect(partials).toEqual(["for a", "for b"]);
    expect(session.pendingPartial).toBe("for b");
    expect(finals).toEqual([]);
  });

  it("ui-reference-dictation-4: discard drops the pending partial and a late final", () => {
    const finals: string[] = [];
    const { session, live } = openSession({
      onPartial() {},
      onFinal(text) {
        finals.push(text);
      },
      onError() {},
    });
    session.start("pane-a");
    const engine = live();
    engine.onresult?.(result("partial words", false));
    expect(session.pendingPartial).toBe("partial words");
    const late = engine.onresult;
    session.discard();
    expect(session.pendingPartial).toBeNull();
    expect(engine.abortCount).toBe(1);
    late?.(result("partial words done", true));
    expect(finals).toEqual([]);
    expect(insertAtCaret("partial words", 0, "partial words".length, "")).toEqual({ value: "", caret: 0 });
  });

  it("ui-reference-dictation-5: permission, no-speech, and network map to the typed union and do not throw", () => {
    expect(dictationErrorFrom({ error: "not-allowed", message: "denied by user" })).toEqual({
      code: "not-allowed",
      message: "denied by user",
    });
    expect(dictationErrorFrom({ error: "service-not-allowed", message: "blocked" })).toEqual({
      code: "permission-denied",
      message: "blocked",
    });
    expect(dictationErrorFrom({ error: "permission-denied", message: "Permission denial" })).toEqual({
      code: "permission-denied",
      message: "Permission denial",
    });
    expect(dictationErrorFrom({ message: "Permission denial" })).toEqual({
      code: "permission-denied",
      message: "Permission denial",
    });
    expect(dictationErrorFrom({ error: "no-speech", message: "silence" })).toEqual({
      code: "no-speech",
      message: "silence",
    });
    expect(dictationErrorFrom({ error: "network", message: "offline" })).toEqual({
      code: "network",
      message: "offline",
    });
    expect(dictationErrorFrom({ error: "audio-capture", message: "no mic" })).toEqual({
      code: "unknown",
      message: "no mic",
    });
    expect(dictationErrorFrom(null)).toEqual({ code: "unknown", message: "speech-recognition-error" });

    const errors: DictationError[] = [];
    const finals: string[] = [];
    const { session, live } = openSession({
      onPartial() {
        throw new Error("partial boom");
      },
      onFinal(text) {
        finals.push(text);
        throw new Error("final boom");
      },
      onError(error) {
        errors.push(error);
        throw new Error("error boom");
      },
    });
    expect(() => session.start("pane-a")).not.toThrow();
    const engine = live();
    const late = engine.onresult;
    expect(() => engine.onerror?.({ error: "not-allowed", message: "denied by user" })).not.toThrow();
    expect(errors).toEqual([{ code: "not-allowed", message: "denied by user" }]);
    late?.(result("secret", true));
    expect(finals).toEqual([]);
    expect(() => session.stop()).not.toThrow();
    expect(() => session.discard()).not.toThrow();

    const denied: DictationError[] = [];
    const { Fake } = recognitionHost();
    class DenyStart extends Fake {
      override start(): void {
        throw Object.assign(new Error("permission blocked"), { name: "NotAllowedError" });
      }
    }
    const probe = probeDictation({ SpeechRecognition: DenyStart });
    if (!probe.available) throw new Error(probe.reason);
    const failing = probe.create({
      onPartial() {},
      onFinal() {},
      onError(error) {
        denied.push(error);
      },
    });
    expect(() => failing.start("pane-a")).not.toThrow();
    expect(denied).toEqual([{ code: "not-allowed", message: "permission blocked" }]);

    const secured: DictationError[] = [];
    class SecurityStart extends Fake {
      override start(): void {
        throw Object.assign(new Error("Permission denial"), { name: "SecurityError" });
      }
    }
    const securityProbe = probeDictation({ SpeechRecognition: SecurityStart });
    if (!securityProbe.available) throw new Error(securityProbe.reason);
    securityProbe
      .create({
        onPartial() {},
        onFinal() {},
        onError(error) {
          secured.push(error);
        },
      })
      .start("pane-a");
    expect(secured).toEqual([{ code: "permission-denied", message: "Permission denial" }]);
  });

  it("ui-reference-dictation-6: caret insertion replaces a selection and a prior partial span", () => {
    expect(insertAtCaret("keep SELECTED tail", 5, 13, "chosen")).toEqual({
      value: "keep chosen tail",
      caret: 11,
    });
    expect(insertAtCaret("abcdef", 5, 2, "Q")).toEqual({ value: "abQf", caret: 3 });
    expect(insertAtCaret("Say ", 4, 4, "hel")).toEqual({ value: "Say hel", caret: 7 });
    expect(insertAtCaret("Say hel", 4, 7, "hello world")).toEqual({
      value: "Say hello world",
      caret: 15,
    });
    expect(insertAtCaret("ab", 10, 12, "c")).toEqual({ value: "abc", caret: 3 });
  });

  it("ui-reference-dictation-7: stop drops a late result and keeps the pending partial", () => {
    const finals: string[] = [];
    const { session, live } = openSession({
      onPartial() {},
      onFinal(text) {
        finals.push(text);
      },
      onError() {},
    });
    session.start("pane-a");
    const engine = live();
    engine.onresult?.(result("keep me", false));
    const late = engine.onresult;
    session.stop();
    expect(session.pendingPartial).toBe("keep me");
    expect(engine.stopCount).toBe(1);
    expect(engine.abortCount).toBe(0);
    late?.(result("keep me please", true));
    expect(finals).toEqual([]);
    expect(session.pendingPartial).toBe("keep me");
  });
});
