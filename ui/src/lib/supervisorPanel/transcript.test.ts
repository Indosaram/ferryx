import { afterEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  desktop: false,
  save: vi.fn(async (_name: string, _text: string) => "/home/me/Downloads/saved.txt"),
}));
vi.mock("../tauri", () => ({
  isTauriRuntime: () => tauri.desktop,
  readSupervisorTranscriptPage: vi.fn(),
  saveSupervisorTranscriptFile: tauri.save,
}));

import {
  exportTranscriptFile,
  formatTranscript,
  isReportMessage,
  parseTranscriptPage,
  readTranscriptAfter,
  reportExcerpt,
  type TranscriptMessage,
  type TranscriptPage,
} from "./transcript";

afterEach(() => {
  tauri.desktop = false;
  tauri.save.mockClear();
  vi.restoreAllMocks();
});

function message(id: number, role: TranscriptMessage["role"], text: string, requestId = `r${id}`): TranscriptMessage {
  return { id, role, text, evidence: [], requestId, createdAtMs: Date.UTC(2026, 10, 3, 9, 0, id) };
}

const PAIR = [message(1, "owner", "Retry every 5 min"), message(2, "supervisor", "Done: retrying every 5 min.", "reply:r1")];

describe("transcript page", () => {
  it("parses the stored page wire shape and refuses a malformed row", () => {
    const row = { id: 1, role: "owner", text: "hi", evidence: [], requestId: "a", createdAtMs: 5 };
    expect(parseTranscriptPage({ messages: [row], nextCursor: 1, hasMore: false })).toEqual({ messages: [row], nextCursor: 1, hasMore: false });
    expect(parseTranscriptPage({ messages: [{ id: 1, role: "stranger", text: "x", requestId: "a" }] })).toBeNull();
    expect(parseTranscriptPage({ rows: [] })).toBeNull();
  });

  it("reads every page after the cursor and stops when the cursor does not advance", async () => {
    const pages: TranscriptPage[] = [
      { messages: [PAIR[0]], nextCursor: 1, hasMore: true },
      { messages: [PAIR[1]], nextCursor: 2, hasMore: false },
    ];
    const load = vi.fn(async (cursor: number) => pages[cursor]);
    const read = await readTranscriptAfter(load, 0);
    expect(read.messages.map((m) => m.id)).toEqual([1, 2]);
    expect(read.cursor).toBe(2);
    expect(load.mock.calls.map(([cursor]) => cursor)).toEqual([0, 1]);

    const stuck = vi.fn(async () => ({ messages: [], nextCursor: 0, hasMore: true }));
    await readTranscriptAfter(stuck, 0);
    expect(stuck).toHaveBeenCalledTimes(1);
  });
});

describe("report excerpt", () => {
  const sections = Array.from({ length: 40 }, (_, i) => `## Section ${i + 1}\n\nMigration check ${i + 1}.`);
  const report = ["# F3 long report tok1", "", ...sections, "", "End of report tok1"].join("\n");

  it("shows the beginning and the last line of a long report, and a short one whole", () => {
    const excerpt = reportExcerpt(report);
    expect(excerpt.head.split("\n")[0]).toBe("F3 long report tok1");
    expect(excerpt.tail).toBe("End of report tok1");
    expect(reportExcerpt("one\ntwo")).toEqual({ head: "one\ntwo", tail: null });
  });

  it("recognizes a stored worker report by its request id", () => {
    expect(isReportMessage(message(3, "supervisor", report, "report:lane_event:9"))).toBe(true);
    expect(isReportMessage(PAIR[1])).toBe(false);
  });
});

describe("transcript export", () => {
  it("writes prompt and Reply rows, oldest first, in the stored words", () => {
    const text = formatTranscript(PAIR);
    expect(text).toContain("Owner prompt\nRetry every 5 min");
    expect(text).toContain("Supervisor reply\nDone: retrying every 5 min.");
    expect(text.indexOf("Retry every 5 min")).toBeLessThan(text.indexOf("Done: retrying"));
  });

  it("downloads a text file in the remote web", async () => {
    const create = vi.fn((_blob: Blob) => "blob:transcript");
    Object.assign(URL, { createObjectURL: create, revokeObjectURL: vi.fn() });
    let download: string | null = null;
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(function (this: HTMLAnchorElement) {
      download = this.download;
    });
    const result = await exportTranscriptFile(PAIR, Date.UTC(2026, 10, 3, 9, 30, 0));
    expect(result).toEqual({ ok: true, where: "supervisor-transcript-2026-11-03T09-30-00-000Z.txt" });
    expect(download).toBe("supervisor-transcript-2026-11-03T09-30-00-000Z.txt");
    const blob = create.mock.calls[0][0];
    expect(blob.type).toContain("text/plain");
    expect(blob.size).toBe(new TextEncoder().encode(formatTranscript(PAIR)).length);
  });

  it("saves through the desktop command and reports a refusal", async () => {
    tauri.desktop = true;
    expect(await exportTranscriptFile(PAIR, 0)).toEqual({ ok: true, where: "/home/me/Downloads/saved.txt" });
    expect(tauri.save.mock.calls[0][1]).toContain("Done: retrying every 5 min.");
    tauri.save.mockRejectedValueOnce(new Error("disk full"));
    expect(await exportTranscriptFile(PAIR, 0)).toEqual({ ok: false, message: "disk full" });
  });
});
